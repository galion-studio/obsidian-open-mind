<!--
SPDX-License-Identifier: MIT
SPDX-FileCopyrightText: 3000 Galion / galion-studio
Copyright (c) 3000 Galion / galion-studio
-->

# cortex /health: real nerve reachability (slot 347, F-337-1)

Branch `swarm/cortex-health-nerve-live` off `cortex-live-merge-3` 73583abf
(the build running live on the VPS as `gaia-cortex-73583ab-llm`). NOT deployed.

## Problem (F-337-1, slot 337)

`GET /health` `"nerve": true` only meant `nerve_url.is_some()`. Box smoke:
`true` with nothing listening on the nerve port. Same blind-spot class as
F-313-2: a dead nerve link looked healthy to anyone reading `/health`.

## Fix

- New `crates/gaia-cortex/src/contact.rs`: `NerveContact`, lock-free atomics
  (last ok / last fail in unix ms, counters, stream state, last error text).
- Recorded by every existing nerve path, no new polling (idle = zero work):
  stream subscriber (200 connect, every line incl. the nerve's 300 s
  keepalives = ok; stream end = fail), `GET /stats` / `/incidents` on
  (re)connect, the pull loop, `POST /actions` and `POST /event` (decider and
  courier fork share the same record; any HTTP < 500 = the nerve answered).
- `/health`: `nerve` = configured AND newest attempt succeeded AND last
  success <= `stale_s` old. Added `nerve_configured` and `nerve_contact`
  `{state, last_ok, age_s, last_fail, fail_age_s, last_error, ok, fail, stream, stale_s}`.
  Computed live on each request, including the busy-snapshot path.
- `nerve_stale_s` / `CORTEX_NERVE_STALE_S`: 0 (default) = auto: 660 s with the
  stream on (2 missed keepalives + margin = the stream idle timeout), else
  3 x `pull_every_s` (min 30 s), else 660 s.
- Backward compatible: `nerve` keeps its name and type; it is now honest.

## Tests (box, 2026-10-28 ~03:30 CET +01:00)

- `cargo test -p gaia-cortex`: all green (lib 36 incl. 7 new `contact` tests;
  `binary_e2e` 3 incl. 2 new: dead nerve port -> `nerve:false unreachable`
  with the connect error, no URL -> `none`; the stream test now asserts
  `nerve:true`, `stream:connected`, `stale_s:660`).
- fmt --check, clippy `--all-targets -D warnings` clean; no-python ok;
  repo-check; x86_64 + aarch64 musl release build, aarch64-android/armv7 check.

### GALION_REAL test `tests/real_health_nerve.rs` (ports 22170-22173)

Real release `gaia-cortex` (this branch) + real release `openmind-nerve
serve --mode event` built from `swarm/nerve-merge-6` 9baa450c (nerve deploy
candidate) + negative control: release `gaia-cortex` 73583abf (base, = live).
3 runs, all passed (run 1 record: `cortex-health-nerve-live/run1.json`).

| phase | new cortex | base 73583abf |
|---|---|---|
| P0 cortex up, nerve not running | `nerve:false` `unreachable`, `last_error` "connect: Connection refused" | `nerve:true` (the bug) |
| P1 nerve starts | `true`, stream `connected`, after 808 / 812 / 811 ms (reconnect backoff) | true |
| P2 nerve SIGKILL | `false` `unreachable` stream `down` at the first read after kill (0 ms; stream EOF) | still `true` 1.5 s later |
| P3 nerve restarted, same port | `true` again after 455 / 458 / 455 ms | - |
| P4 nerve SIGSTOP (hung, socket open) | `CORTEX_NERVE_STALE_S=5` cortex: `stale` after 5012 / 4977 / 5010 ms, stream still `connected`; default cortex still `true` (660 s window, documented limit) | - |
| P4 end SIGCONT + SIGKILL | both `false` | - |

## Live VPS (read-only, 2026-10-28 03:33 CET +01:00)

- `gaia-cortex` active, ExecStart `/opt/galion/rust/bin/gaia-cortex-73583ab-llm`,
  `CORTEX_NERVE_URL=http://127.0.0.1:8670`.
- `GET :8674/health`: `"nerve":true`, mode live, events_in 110, incidents_open 2,
  escalations_open 1 (no `nerve_contact`: old build).
- `GET :8670/health`: nerve status ok, system degraded, cycle 46; `/stats` 200.
  So the live `true` happens to be right today, but the old field cannot
  tell; after deploy, `nerve_contact.age_s` / `stream` will show it.
- Nothing was changed, restarted or written on the VPS.

## Limits / owner notes

- A hung nerve with the stream socket open reads `true` up to 660 s by default
  (keepalive is 300 s). Lower `CORTEX_NERVE_STALE_S` only if the nerve
  keepalive is lowered too, else a quiet healthy nerve would flap to `stale`.
- Consumers that read `/health.nerve` (console, watchers) now see `false` when
  the link is really down; that is the point, but expect new red where it was
  falsely green.
- Deploy: ship together with the next cortex build (D-343/cortex-live-merge-4
  line); no config change needed.
