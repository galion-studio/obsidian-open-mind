<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: nerve-t24-wire-gaps-2 (wave-4 slot 277)

Branch `swarm/nerve-t24-wire-gaps-2`, base `swarm/nerve-t24-wire-gaps` `29a4d41`
(slot 264). Times are Europe/Zagreb (CET +01:00). Box only. No live deploy,
no CF DNS, no live HADES, no secrets.

## What was wired

Next highest-value remaining gaps from slot 264, one crate per commit.
Prefer real bins we own over third-party. Pattern matches slot 264:
`nerve-client` started + heartbeat; `observe_request` where HTTP; no-op
unless `NERVE_URL` is set.

| source (nerve-client) | crate | signals |
|---|---|---|
| galion-origin | `crates/galion-origin` | started, heartbeat, observe_request |
| room | `crates/room` | started, heartbeat, observe_request |
| hades-gate | `crates/hades-gate` | started, heartbeat, observe_request |
| galion-guard | `crates/galion-guard` | started, heartbeat |
| galion-backup | `crates/galion-backup` | started (+ heartbeat on `run`) |
| galion-deploy | `crates/galion-deploy` | started (+ heartbeat on deploy) |

No new third-party crates; `nerve-client` is in-workspace. See
`docs/third-party/nerve-t24-wire-gaps-2.md`.

## Classification (remaining / careful)

| bin | status | note |
|---|---|---|
| guardian-tick | still GAP | Uses `openmind-nerve` lib file writer (`Writer` → `nerve.jsonl`), D-T24-3. Do **not** add a parallel `nerve-client` path until that migration is designed; file sink is intentional today. |
| desk-hermes | still GAP | One-shot mindpack printer for galion-desk; lower value than live services. |
| galion-boot | still GAP | Boot/keep CLI; event-worthy later. |
| openmind-discovery | still GAP | LAN discovery CLI; announce path could take nerve later. |
| persona-plex-relay | still GAP | Scaffold; opens no sockets. |
| galion-voice-serve | still GAP | Autobin of `galion-voice-cpu`; heavy model load; wire with voice stack. |

hades-gate wiring is **crate-only on the box**. No live HADES unit touch,
no disable/enable against production, no CF DNS.

Side effect: `gaia` / `gaia-dummy` now reach nerve-client via the
`hades-gate` workspace edge. The redundant `gaia:gaia-dummy` probe-only
allowlist entry was dropped.

## Box real test

```
GALION_REAL=1 REAL_WIRE_GAPS2_PORT_BASE=21470 \
  cargo test --release -p openmind-nerve --test real_wire_gaps \
  six_more_services_push_started_and_heartbeat -- --nocapture
```

Stack (ports 21470-21479):

| port | process |
|---|---|
| 21470 | openmind-nerve serve |
| 21471 | galion-origin |
| 21472 | room --listen |
| 21473 | hades-gate serve (temp keygen + config under /tmp) |
| (none) | galion-guard run (dry-run + file source) |

The test asserts each of the four long-running sources has a `started`
event and a `heartbeat` in `GET /api/v1/signals`. galion-backup and
galion-deploy are covered by repo-check (direct nerve-client); one-shot
CLI exit can race the push worker, so they are not asserted in this
process stack.

## repo-check nerve gaps

After wiring, `repo-check --only nerve --nerve-gaps` reports **6 gaps**
(was 12 on slot 264): the six crates above move from GAP to covered.

## Evidence (2026-10-27 17:54 CET +01:00)

`GALION_REAL=1 REAL_WIRE_GAPS2_PORT_BASE=21470 cargo test --release -p openmind-nerve --test real_wire_gaps six_more_services_push_started_and_heartbeat -- --nocapture`

```
== 2026-10-27T17:54:12+01:00 gaps-2 stack up on base 21470; waiting for heartbeats
-- pushed sources
{"dropped":0,"source":"galion-guard","total":3}
{"dropped":0,"source":"galion-origin","total":3}
{"dropped":0,"source":"hades-gate","total":3}
{"dropped":0,"source":"room","total":3}
== 2026-10-27T17:54:12+01:00 PASS: six crates wired; four long-running proven (slot 277)
test six_more_services_push_started_and_heartbeat ... ok
```

`repo-check --only nerve --nerve-gaps`: `nerve coverage: 37 bins, 18 covered, 13 probe-only, 6 gap(s)` (was 12 covered / 12 gap on slot 264).
