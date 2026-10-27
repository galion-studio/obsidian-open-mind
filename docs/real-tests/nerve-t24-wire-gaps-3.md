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

## Evidence

See CALL-RECORD DONE line for this slot (Europe/Zagreb CET +01:00).
