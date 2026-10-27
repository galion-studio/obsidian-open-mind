<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: nerve-wire-gaps-2 (wave-4 slot 327)

Branch `swarm/nerve-wire-gaps-2`. Base: `swarm/guardian-tick-wire` `6e6cae43`
(slot 297, pushed). It descends from the row's `nerve-t24-wire-gaps` `404eacb2`
through slot 289 `97fb7204`. Times are Europe/Zagreb (CET +01:00). No deploy, no
live service change, no Cloudflare DNS, no HADES change, no secrets.

## Why the row was re-scoped

Row 327 asked to wire "the remaining 12 nerve-client coverage gaps". That count
was out of date. Slots 277 (12->6), 289 (6->1) and 297 (1->0) had already
closed them, and `repo-check --only nerve --nerve-gaps` on the base reads
`37 bins, 24 covered, 13 probe-only, 0 gap(s)`. The gate only checks
dependencies, though: a crate counts as covered as soon as it depends on
`nerve-client`. So this slot checked the rest of the row, "started+heartbeat,
observe_request where it serves HTTP", to see whether the signals really reach
a nerve. Two real holes showed up:

1. **One-shot CLIs lose `started`.** nerve-client queues signals for a
   background worker. desk-hermes, persona-plex-relay and the one-shot paths of
   galion-backup, galion-boot and galion-deploy exit straight after
   `event("started")`, so the worker is killed before the POST happens.
   Control run on the base binaries: **4/5 lost on the box and 4/5 lost on the
   VPS**. desk-hermes only gets through by luck, because its mindpack read is
   slower.
2. **galion-one never reports requests.** galion-one is the live :8091 main
   and :8002 mouth server. It pushed `started` and a heartbeat but never called
   `observe_request`, so its 5xx responses and slow requests were never pushed
   as signals.

## What changed

| crate | change |
|---|---|
| `repo-check` | New wiring-depth check `nerve::check_wiring`, printed with `--nerve-gaps`. It covers every package that has a bin and depends on nerve-client directly, and scans the non-test part of `src/**/*.rs`. Each package must (a) send `nerve_client::event(.., "started", ..)`, (b) call `observe_request` if it serves HTTP (`TcpListener::bind` / `Server::http` outside `#[cfg(test)]`), and (c) call `start_heartbeat` or `nerve_client::flush`. A miss is a **warning**, so CI is not broken; the dependency gate stays the hard error. A reasoned opt-out is `// nerve-wiring: exempt <rule> <reason>` in src. 6 new unit tests in `tests/nerve_wiring.rs`. |
| `galion-one` | New `src/obs.rs` (thread-local per-connection record). `write_response` and `begin_audio` record the status and the time to the response head, and both accept loops call `obs::finish("galion-one")`, which calls `observe_request`. Mouth requests are named `mouth GET /health` and so on. Time-to-head is used rather than total time, so a long streamed voice reply does not count as "slow". Fast 2xx responses push nothing. 4 unit tests. |
| `desk-hermes`, `persona-plex-relay` | `nerve_client::flush(500 ms)` before exit. persona-plex-relay's `main` becomes a wrapper around `run()`. |
| `galion-backup` | `exit()` helper (flush + `process::exit`) used by `die`, `run` and `verify`, plus a flush at the end of `main` for snapshot, schema and migrate-baseline. |
| `galion-boot`, `galion-deploy` | Flush before `process::exit(code)`. |
| `openmind-nerve` tests | New `tests/real_wire_depth.rs` (GALION_REAL, ports 21970-21979). |

Without `NERVE_URL`, `flush` returns at once and `observe_request` is one
atomic load, so behaviour is unchanged. With a dead nerve, flush is bounded:
every one-shot still exits in under 70 ms (measured).

## repo-check gap count

| check | base 6e6cae43 | this branch |
|---|---|---|
| dependency coverage (`nerve coverage`) | 0 gap(s) | 0 gap(s) |
| wiring depth (`nerve wiring`), new | **3 wiring gap(s)**: desk-hermes heartbeat_or_flush, galion-one observe_request, persona-plex-relay heartbeat_or_flush | **0 wiring gap(s)**, 19 direct packages, all `wired` |

`repo-check --only nerve --nerve-gaps` 2026-10-28T00:17:21+01:00: `nerve wiring:
19 direct packages, 0 with gaps, 0 wiring gap(s)`, `0 error(s), 0 warning(s) -> PASS`.

The three static gaps were the ones the check could see. The one-shot paths
of galion-backup, galion-boot and galion-deploy passed statically, because
those crates also start a heartbeat on their long-running paths. The real test
below caught and covers them.

## Box real test (2026-10-28 00:19 CET)

`GALION_REAL=1 NERVE_WIRING_BIN_DIR=$T/release WIRE_DEPTH_CONTROL_BIN_DIR=<base 6e6cae43 release> cargo test --release -p openmind-nerve --test real_wire_depth -- --nocapture`

The test runs a real openmind-nerve on 21970 and real galion-one on 21971/21972
(mouth). The dead nerve is 21979.

```
== 2026-10-28T00:19:43+01:00 nerve up on 21970
one-shot desk-hermes          exit=0      6 ms started_delivered=1
one-shot persona-plex-relay   exit=0      6 ms started_delivered=1
one-shot galion-backup        exit=1      6 ms started_delivered=1
one-shot galion-boot          exit=2      6 ms started_delivered=1
one-shot galion-deploy        exit=2      6 ms started_delivered=1
CONTROL  desk-hermes          exit=0      1 ms started_delivered=1
CONTROL  persona-plex-relay   exit=0      0 ms started_delivered=0
CONTROL  galion-backup        exit=1      1 ms started_delivered=0
CONTROL  galion-boot          exit=2      0 ms started_delivered=0
CONTROL  galion-deploy        exit=2      0 ms started_delivered=0
CONTROL lost started: 4/5
dead-nerve desk-hermes             0 ms
dead-nerve persona-plex-relay      5 ms
dead-nerve galion-backup           6 ms
dead-nerve galion-boot             6 ms
dead-nerve galion-deploy           6 ms
galion-one total before=5 after 5 fast 200s=5
mouth /health -> 503 {"ok":false,"status":"unavailable",...}
galion-one pushed: recent = slow_request "GET /api/v1/awake -> 200 in 449 ms (threshold 300 ms)",
  http_5xx "mouth GET /health -> 503" (x3), nerve_client_dropped 0
== 2026-10-28T00:19:44+01:00 PASS: wire depth (slot 327)
test result: ok. 1 passed; 0 failed
```

The slow request is real, not mocked: the client holds the connection open
for 450 ms before sending the request line, with `NERVE_SLOW_MS=300`.

## VPS throwaway run (2026-10-28 00:27 CET, as `nobody`)

Static x86_64-musl binaries (this branch and the base control) went to
`/var/tmp/wd327-<ts>`. Everything ran as `nobody` against a throwaway nerve on
127.0.0.1:21970, with galion-one on 21971/21972 and a dead nerve on 21979.
Nothing was built on the VPS, the live services and `/opt/galion/openmind` were
not touched, and the folder, processes and ports were cleaned up afterwards
(log: `procs cleaned`, `dir removed`).

```
== 2026-10-28 00:27:21 +0100 host=ip50
new desk-hermes exit=0 31ms started_delivered=1
new persona-plex-relay exit=0 25ms started_delivered=1
new galion-backup exit=1 21ms started_delivered=1
new galion-boot exit=2 32ms started_delivered=1
new galion-deploy exit=2 38ms started_delivered=1
ctl desk-hermes exit=0 34ms started_delivered=1
ctl persona-plex-relay exit=0 23ms started_delivered=0
ctl galion-backup exit=1 33ms started_delivered=0
ctl galion-boot exit=2 12ms started_delivered=0
ctl galion-deploy exit=2 19ms started_delivered=0
dead-nerve new desk-hermes 23ms
dead-nerve new galion-boot 21ms
galion-one total before=4 after 5 fast GETs=4
mouth /health -> 503
"source":"galion-one","kind":"event","name":"slow_request","value":451.0,"msg":"GET /api/v1/awake -> 200 in 451 ms (threshold 300 ms)"
"source":"galion-one","kind":"error","name":"http_5xx","msg":"mouth GET /health -> 503"  (x3)
procs cleaned / dir removed
```

Two earlier VPS attempts at 00:26 CET showed `started_delivered=0` for
everything. The cause was a broken parser in the run script: the musl
openmind-nerve emits JSON keys in insertion order, not alphabetical order. The
raw `/api/v1/signals` dump from the second attempt shows that the new
binaries' `started` had in fact arrived. Both attempts were cleaned up the same
way.

## Live VPS read-only probes (2026-10-28 00:20 CET)

* Live nerve `/api/v1/signals` (galion-nerve, openmind-nerve-9d1a4f3): only
  `hermes-rs` and `openmind-memory` push, plus a stale `gaia-cortex` (last
  push 19:56 CET).
* `systemctl show -p Environment`, checked only for whether `NERVE_URL` is
  present (values not printed): `NERVE_URL` is **not set** on galion-one,
  galion-desk, galion-backup or guardian-tick. So today these services push
  nothing to the live nerve, whatever their builds can do.
* Live galion-one (`/opt/galion/rust/bin/galion-one`, pid 304) listens on
  127.0.0.1:8091 (main, `/health` 200) and 127.0.0.1:8002 (mouth, `/health`
  **503**). The live nerve `/state` carries incident `inc-865945dd` [error]:
  "galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 6.0 h"
  (chatterbox, status loading). The nerve sees this through its probe only.

## Gates

* `cargo fmt --all --check` ok.
* `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
* `cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`: 269 suites, **1270 passed, 0 failed**, 6 ignored. sandbox_vps_only: no sandbox tests ran on the box.
  * Pre-existing flake, not from this branch: `room` `tests::default_and_fixture_do_not_open_a_socket` compares process-wide socket fds while sibling tests run, and failed once in a `-p room --lib` run on the box (1 of 3 runs).
* `repo-check` 6 new wiring tests plus the existing nerve tests: all pass.
* x86_64-musl and aarch64-musl release builds ok (zig cc for sqlite C).
  galion-one is 861 536 B on x86_64 and 762 952 B on aarch64.
* `cargo check` for aarch64-linux-android and armv7-musleabihf ok.
* `scripts/no-python.sh` ok. `scripts/spdx-scan.sh`: 0 missing.

## Owner decisions

* **D-327-1 (deploy, owner):** turn on `NERVE_URL=http://127.0.0.1:8670` for
  galion-one, galion-desk, galion-backup and guardian-tick through drop-ins,
  after this branch is built and deployed. Until then the wiring here has no
  live effect. NO deploy was done in this slot.
* **D-327-2 (noise):** with `NERVE_URL` set on live galion-one, every nerve
  probe of the stuck mouth (`:8002/health` 503) also pushes an `http_5xx` error.
  The hub's per-source rate cap bounds this. It adds pushed-error weight to the
  existing inc-865945dd rather than opening a new incident. If you want fewer
  duplicate signals, exempt mouth `/health` from `observe_request`. The
  recommendation is to keep it: it is the truth while the mouth is stuck.
* **D-327-3 (gate strictness):** wiring misses are warnings for now. Once this
  is merged they can be made errors with `--level nerve=error`, but that would
  also turn dependency-allowlist warnings into errors. A separate check id
  would be cleaner (follow-up).
