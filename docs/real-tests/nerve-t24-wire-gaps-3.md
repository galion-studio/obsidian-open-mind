<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: nerve-t24-wire-gaps-3 (wave-4 slot 289)

Branch `swarm/nerve-t24-wire-gaps-3`, base `swarm/nerve-t24-wire-gaps-2` `404eacb`
(slot 277). Times are Europe/Zagreb (CET +01:00). Box only. No live deploy,
no CF DNS, no live HADES, no secrets.

## What was wired

Next highest-value remaining gaps from slot 277, one crate per commit.
Prefer real bins we own over third-party. Pattern matches slots 264/277:
`nerve-client` started + heartbeat; `observe_request` where HTTP; no-op
unless `NERVE_URL` is set.

| source (nerve-client) | crate | signals |
|---|---|---|
| galion-voice-serve | `crates/galion-voice-cpu` (bin) | started, heartbeat, observe_request |
| openmind-discovery | `crates/openmind-discovery` | started; heartbeat on serve-route/announce; observe_request on serve-route |
| galion-boot | `crates/galion-boot` | started; heartbeat on supervise |
| desk-hermes | `crates/desk-hermes` | started |
| persona-plex-relay | `crates/persona-plex-relay` | started |

No new third-party crates; `nerve-client` is in-workspace. See
`docs/third-party/nerve-t24-wire-gaps-3.md`.

## Classification (remaining / careful)

| bin | status | note |
|---|---|---|
| guardian-tick | still GAP | Uses `openmind-nerve` lib file writer (`Writer` → `nerve.jsonl`), D-T24-3. Do **not** add a parallel `nerve-client` path until that migration is designed; file sink is intentional today. |

## Box real test

```
GALION_REAL=1 REAL_WIRE_GAPS3_PORT_BASE=21570 \
  cargo test --release -p openmind-nerve --test real_wire_gaps \
  five_more_services_push_started_and_heartbeat -- --nocapture
```

Stack (ports 21570-21579):

| port | process |
|---|---|
| 21570 | openmind-nerve serve |
| 21571 | galion-voice-serve (`--stt none --tts none`) |
| 21572 | openmind-discovery serve-route |
| (none) | galion-boot supervise (empty services, `--ticks 120`) |

The test asserts each of the three long-running sources has a `started`
event and a `heartbeat` in `GET /api/v1/signals`. desk-hermes and
persona-plex-relay are covered by repo-check (direct nerve-client); one-shot
CLI exit can race the push worker, so they are not asserted in this
process stack.

## repo-check nerve gaps

After wiring, `repo-check --only nerve --nerve-gaps` reports **1 gap**
(was 6 on slot 277): the five crates above move from GAP to covered;
guardian-tick remains (D-T24-3).

## Evidence (2026-10-27 18:03 CET +01:00)

`GALION_REAL=1 REAL_WIRE_GAPS3_PORT_BASE=21570 cargo test --release -p openmind-nerve --test real_wire_gaps five_more_services_push_started_and_heartbeat -- --nocapture`

```
== 2026-10-27T18:03:47+01:00 gaps-3 stack up on base 21570; waiting for heartbeats
-- pushed sources
{"dropped":0,"source":"galion-boot","total":3}
{"dropped":0,"source":"galion-voice-serve","total":3}
{"dropped":0,"source":"openmind-discovery","total":3}
== 2026-10-27T18:03:47+01:00 PASS: five crates wired; three long-running proven (slot 289)
test five_more_services_push_started_and_heartbeat ... ok
```

`repo-check --only nerve --nerve-gaps`: `nerve coverage: 37 bins, 23 covered, 13 probe-only, 1 gap(s)` (was 18 covered / 6 gap on slot 277). Remaining GAP: guardian-tick (D-T24-3).
