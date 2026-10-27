<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: nerve-t24-wire-gaps (wave-4 slot 264)

Branch `swarm/nerve-t24-wire-gaps`, base `swarm/nerve-t24-coverage-gate` `457bc87`
(slot 226). Times are Europe/Zagreb (CET +01:00). Box only. No live deploy,
no CF DNS, no HADES, no secrets.

## What was wired

Highest-value live-first gaps from slot 226 D-T24-2, one crate per commit:

| source (nerve-client) | crate | signals |
|---|---|---|
| openmind-queue | `crates/openmind-queue` | started, heartbeat, observe_request |
| galion-one | `crates/galion-one` | started, heartbeat |
| galion-health | `crates/galion-health` | started, heartbeat, observe_request |
| galion-desk | `crates/galion-desk` | started, heartbeat, observe_request |
| link-watch | `crates/link-watch` | started, heartbeat |
| tuner | `crates/tuner` | started, heartbeat, observe_request |

Every call is a no-op unless `NERVE_URL` is set (same contract as
openmind-memory / hermes-rs). No new third-party crates; `nerve-client` is
in-workspace.

## Box real test

```
GALION_REAL=1 REAL_WIRE_GAPS_PORT_BASE=21310 \
  cargo test --release -p openmind-nerve --test real_wire_gaps -- --nocapture
```

Stack (ports 21310-21319):

| port | process |
|---|---|
| 21310 | openmind-nerve serve |
| 21311 | openmind-queue serve |
| 21312 | galion-one (`ONE_BIND`) |
| 21313 | galion-one mouth |
| 21314 | galion-health |
| 21315 | galion-desk |
| 21316 | tuner |
| (none) | link-watch (file writer; still pushes) |

The test asserts each of the six sources has a `started` event and a
`heartbeat` in `GET /api/v1/signals`.

## repo-check nerve gaps

After wiring, `repo-check --only nerve --nerve-gaps` should report 12 gaps
(was 19): the six live-first services move from GAP to covered. Remaining gaps (not this slot): desk-hermes, galion-backup, galion-boot, galion-deploy, galion-guard, galion-origin, galion-voice-serve, guardian-tick, hades-gate, openmind-discovery, persona-plex-relay, room.

## Owner leftovers from D-T24-2

Classify / wire or probe-only the remaining 12. guardian-tick still uses
openmind-nerve lib file writer (D-T24-3).

## Evidence (2026-10-27 17:42 CET +01:00)

`GALION_REAL=1 REAL_WIRE_GAPS_PORT_BASE=21310 cargo test --release -p openmind-nerve --test real_wire_gaps -- --nocapture`

```
== 2026-10-27T17:42:34+01:00 stack up on base 21310; waiting for heartbeats
-- pushed sources
{"dropped":0,"source":"galion-desk","total":3}
{"dropped":0,"source":"galion-health","total":3}
{"dropped":0,"source":"galion-one","total":3}
{"dropped":0,"source":"link-watch","total":3}
{"dropped":0,"source":"openmind-queue","total":3}
{"dropped":0,"source":"tuner","total":3}
== 2026-10-27T17:42:34+01:00 PASS: six live-first services wired
test six_live_first_services_push_started_and_heartbeat ... ok
```

`repo-check --only nerve --nerve-gaps`: `nerve coverage: 37 bins, 12 covered, 13 probe-only, 12 gap(s)` (was 4 covered / 19 gap on slot 226).
