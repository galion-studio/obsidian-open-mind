<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# galion-one: ONE-PROBE port, dropped features, cutover differences

- Slug / branch: `one-probe` / `swarm/one-probe`, based on GitHub main 13a296c
- Written: 2026-10-20T19:05+02:00 (Europe/Zagreb, CEST)
- Crate: `crates/galion-one` (the portable one: pure-Rust read-only SQLite
  reader, no FFI, no system libsqlite, std only)

## 1. What changed on this branch

| change | source | where |
|---|---|---|
| ONE-PROBE: the `one` self probe dials `ONE_BIND`; the mouth client dials `GALION_MOUTH`, else our own `ONE_MOUTH_BIND`; a mouth equal to our own mouth bind counts as local (no POST to our own `/synthesize`) | VPS 2f5a730 | `src/probe.rs`, `sample()`, `mouth_addr()`, `mouth_remote()` |
| Wildcard binds (`0.0.0.0`, `::`) are dialled on loopback | new | `probe::dial_addr` |
| Every health pulse reports the port it actually dialled (was: fixed 8642/8643/8645 even when `GALION_MIND/EAR/EYE` moved them) | new | `sample()` |
| `GALION_MEMORY` (`host:port`, default `127.0.0.1:8420`) moves the memory-gateway probe; the gateway `/proc` lookup only runs on loopback | new (same class of bug as ONE-PROBE) | `probe::memory_target`, `memory_section()` |
| Pairing fails closed: a missing/empty/unreadable device store means nobody is paired | ws/ui 15cdb53 (live since Oct 18) | `token_ok()` + `pairing_tests` |
| `GET /api/v1/services` (unpaired, same JSON as live), with no `systemctl` shell-out | ws/ui f69a916 (live) | `src/services.rs` |

Port of the VPS env model: VPS 2f5a730 reads `Settings` built by B06
(`openmind-config`). Main has no `Settings`, so `probe.rs` resolves the same
values from env only (`ONE_BIND`, `ONE_MOUTH_BIND`, `GALION_MOUTH`), with
identical precedence and defaults. When no config file exists (the VPS today:
`/etc/openmind/openmind.toml` is absent, checked 19:00 CEST), the result is
the same as the VPS code.

Tests: 39 unit tests in galion-one (was 25; +10 probe, +3 services, +1 pairing),
plus the gated real-life test `tests/real_one_probe.rs` (`GALION_REAL=1`) and
the static-musl scenario `tests/real_one_static.rs` (was `scripts/real/one-probe.sh`).

## 2. VPS-only / live-only galion-one features and whether they were kept

Compared: main 13a296c vs VPS `vps-main-20261020` (2f5a730) vs `ws/ui` 6e2a4ce
(the source of the live binary `/opt/galion/openmind/dist/bin-ws/galion-one`).

| feature | where | status | note |
|---|---|---|---|
| ONE-PROBE self probe / mouth client follow the binds | VPS 2f5a730 | **ported** | see §1 |
| Fail-closed pairing | ws/ui 15cdb53 (live) | **ported** | main was fail-OPEN: with no store at `ONE_DEVICE_STORE`, every paired route was open. A cutover from live to main without this would have opened the admin. |
| `GET /api/v1/services` | ws/ui f69a916 (live) | **ported** | `systemctl is-active link-watch-rs` replaced by a `/proc/*/comm == "link-watch"` scan (VPS comm checked: `link-watch`). |
| B06 config file (`$OPENMIND_CONFIG`, default `/etc/openmind/openmind.toml`): bind `127.0.0.1:{ports.one}`, `GALION_HOME = paths.base`, hermes dir `paths.base/hermes`, `OPENMIND_*` overrides, unreadable config = exit 1; 7 tests | VPS 6741b40 | **not ported, worth keeping** | Needs the `openmind-config` path dependency (pure Rust, in-workspace). It is a no-op on the VPS today (file absent). Recommended as a follow-up ticket; `probe.rs` takes an env closure, so a `Settings` layer can feed it. |
| Legacy Python TTS worker removed | VPS ae72938 | not needed | main already has no Python (`speech.rs`). |
| Raw libsqlite3 FFI + `build.rs` pinning x86_64 glibc paths | VPS | **dropped on purpose** | regression; main's `sqlite_ro.rs` replaces it. |
| `sha256sum` shell-out | VPS | **dropped on purpose** | main has `sha256.rs`. |
| Hard-coded `/root/galion-secrets/admin-device.json`, `/root/.hermes/.env` | VPS | main is better | main has `ONE_DEVICE_STORE`, `ONE_HERMES_ENV`. |
| Unknown args (`--help`) start the servers | VPS | main is better | main runs `cli.rs` subcommands and exits. |

GH-only features that the VPS/live builds lack and a cutover gains: `cli.rs`
(set-model / trace), `ONE_WARM_MIND=0`, `ONE_DEVICE_STORE`/`ONE_HERMES_ENV`,
`persona.rs`, `speech.rs`, `sqlite_ro.rs` (WAL-aware), models refresh backoff.

## 3. Differences that matter for a future cutover (live -> this build)

Live was probed READ-ONLY (GET only, no cookie, no pairing, no POST) on
2026-10-20 at 18:56:42 and 19:00:13 CEST (VPS 127.0.0.1:8091 / :8002) and
19:00:00 CEST (https://one.galion.app). Live unit: `galion-one.service`,
pid 1373612, active since 2026-10-18 22:13:56 CEST, binary
`/opt/galion/openmind/dist/bin-ws/galion-one` (static, 0 NEEDED). The box
build ran with fixture data on 127.0.0.1:19050/19051. Full outputs:
[real-tests/one-probe.md](real-tests/one-probe.md).

Same on both (status, content type, JSON shape):
`/ /index.html /nerve /admin /space /mark.svg /font/jost.woff2` (200; bodies are
files under `GALION_HOME/one`, not in the binary), `/health` (200 `ok\n`),
`/link`, `/me` (`{"you":false}`), `/api/v1/awake`, `/api/v1/catalog`,
`/api/v1/services`, and 401 `{"error":"pair"}` for `/api/v1/analytics*`,
`/session`, `/files`, `/files/<id>`, `/model`, `/models`; 404 `not found\n` for
unknown paths (also `GET /pair`). Mouth `/health` 503 `loaded:false` on both.

Differences and cutover notes:
1. **Pairing**: live and this branch fail closed; GitHub main 13a296c fails
   OPEN. Do not cut over to main without this branch (or 15cdb53).
2. **Self probe / mouth client**: only differ when `ONE_BIND` /
   `ONE_MOUTH_BIND` are moved. With the live defaults the `one` pulse dials
   8091 and the mouth 8002 exactly as live. A spare/shadow instance of this
   build no longer dials the live 8091/8002 (live's would).
3. **Reported ports** in `/api/v1/analytics/services` and `/api/v1/services`
   are the dialled ports. Identical to live with default env.
4. **link-watch-rs** in `/api/v1/services`: live asks systemd for the unit;
   this build looks for a process named `link-watch`. They disagree only if
   the unit is active with no such process, or a stray `link-watch` runs
   outside the unit (LIVE.md lists strays for other services, not this one).
5. **Memory probe**: `GALION_MEMORY` is new; unset = live behaviour.
6. **Config file (B06)**: neither live nor this build reads
   `/etc/openmind/openmind.toml`; VPS main would. It is absent today.
7. **Paired routes** (`/api/v1/analytics*`, `/session`, `/files`, `/model`,
   `/models`, `POST /admin/say|/hear|/reply|/files|/model|/voice`) were NOT
   compared against live: that needs a pairing cookie, which this task must
   not obtain. Their code is unchanged between ws/ui (live) and main apart from
   the port values above, so shapes are expected to match.
8. **Public edge**: one.galion.app returns the same routes and JSON, but its
   HTML is ~938 bytes larger than 8091 and `/` and `/index.html` hash
   differently, so something on the edge rewrites HTML. Not galion-one; keep
   in mind when diffing pages after a cutover.
9. **Size**: live x86_64 binary 828,640 bytes; this build 832,768
   (static-pie), aarch64 736,472 (static).

## 4. Open issues found while reviewing (not fixed here)

- The crate still shells out: `curl` (edge probe of 5 public hosts every 15 s,
  and the OpenRouter models refresh), `pdftotext`, `ps`, `nvidia-smi`. The
  task wording says "no shell-outs"; this branch adds none, but removing
  these needs owner decisions (TLS without curl means a TLS crate).
- A spare instance still probes fixed `127.0.0.1:8080` (console, no env
  knob), still curls the public hosts, and `memory_db_paths()` always scans
  `/opt/galion/memory-tencentdb` and `/var/lib/memory-tencentdb` and reads
  `/etc/memory-gateway.env` (read-only). On the VPS a spare would read the
  live memory DB files read-only.
- `POST /pair` with no store creates one (empty `codeHash`, `fails:1`) at
  `ONE_DEVICE_STORE`. Still closed, but it writes into the secrets directory
  on a default install. Same on live.
- `POST /hear` and `/reply` unpaired answer 200 `{"error":"pair"}`, not 401
  (same on live; the page relies on it).
