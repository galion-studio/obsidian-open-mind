<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# gaia-cortex: the event-driven decision layer

Status 2026-10-21 +02:00: branch `swarm/cortex-event-driven`, **not deployed**.
Crates: `crates/gaia-cortex` (binary `gaia-cortex`), `crates/nerve-cortex-contract`.
Contract with the nerve: [event-contract.md](event-contract.md).

## What it is

The nerve observes and (within its allowlist) acts; the cortex decides. The
cortex never runs a command itself: its only lever is an action request to
the nerve's `POST /actions`, and the only action it can express is
`restart <unit>`. Everything it decides is written down
(`decisions.jsonl`, `alerts.jsonl`, `escalations.jsonl`) and served on a
loopback API.

## Event-driven, idle = zero

- The subscriber thread blocks in `read()` on the nerve's `/events/stream`
  (the nerve writes one keepalive line per 5 min; keepalives are not events).
- The decider thread blocks on a channel (`recv()`); there is no timer, no
  periodic cycle, no polling (unless the optional pull adapter for an old
  polling nerve is switched on, `pull_every_s`, default 0).
- The LLM advisor is only reachable from a decision about an incident, so
  no events = no model calls by construction. Even with events, deterministic
  rules handle every normal case; the advisor is only consulted for
  ambiguous incidents at severity >= error **and** only if enabled.

## Pipeline

```text
nerve /events/stream ─┐
POST /v1/events ──────┼─> dedupe (source+kind+seq, 8192 fingerprints; own echoes dropped)
unix socket ──────────┘     │
                            v
                  burst coalescing (first event + coalesce_ms, default 1.5 s)
                            │
                  subjects: unit:<u> | threshold:<path> | proc:<comm> | task:<id> | slot:<id> | host:<h>
                            │
                  correlate (openmind_nerve::incident::Correlator, T3.2; stack-wide fold)
                            │
                  rules (deterministic, crates/gaia-cortex/src/rules.rs)
                            │ ambiguous + severity >= error + advisor enabled + compiled
                            ├──────> LLM advisor (choices: none | alert | escalate | restart:<allowlisted unit>)
                            v
     ┌──────────────────────┼─────────────────────────┐
  action request         alert sink               escalate-to-human
  POST nerve /actions    alerts.jsonl, stderr,    escalations.jsonl, GET /escalations,
  (dry-run default)      nerve POST /event,       POST /escalations/ack?id=,
                         optional outbox file     nerve POST /event (cortex.escalation)
```

Reused, not rewritten: `incident::Correlator` / `correlate_with` (T3.2
correlation incl. the `stack-wide` fold), `protected_hosts::ProtectedHosts`
(D-T42-4 never-touch hostnames: VPS, DNS, mail, backup),
`reflex::ALLOW` (default restart allowlist) and `reflex::DENY` (never-touch
unit list) from `openmind-nerve`. The base branch contains every cortex
branch listed in the task (merge-all 6bcb882, t32 ae604dc, t42 1ce9737,
protected-hosts 60312ac, t53 55f07ff, t52 c2f7cda).

## Rules (first match wins)

| rule | when | outcome |
|---|---|---|
| R0-stack-wide | 3+ subjects hard-failing in one burst (correlator `stack-wide`) | alert + escalate, no action (host/network problem) |
| R1-protected | unit/host/path protected (below) | **alert only, never act** |
| R2-circuit-open | `flap_failures` (3) failures in `flap_window_s` (900 s), counting every occurrence the nerve folded (`count`, repeat watch), or failure while open | open circuit for `circuit_open_s` (3600 s), escalate once, no action |
| R2-start-limit | `unit.start_limit` | circuit + escalate |
| R3-known-crash | crash / OOM / failed state of a restart-allowed unit | restart request to the nerve (dry-run) |
| R3-cap | ... cortex cap `restarts_per_unit_hour` (2 = the nerve executor cap) used, in every mode | circuit + escalate |
| R3-cap-total | ... `restarts_total_hour` (6 = the nerve total cap) restarts asked for all units in the last hour | escalate, no circuit, no action |
| R3b-systemd-restarting | failure + `unit.restart_scheduled` | alert, no extra restart (counts toward flapping) |
| R4-unknown-unit | failure of a unit not on the restart allowlist | alert + escalate |
| R5-recovered | unit active again / lifecycle info | nothing |
| R6-task-failed | task/slot `ok=false` | alert |
| R7-threshold | threshold: warn -> alert; error+ -> escalate (advisor-eligible) | |
| R8-unclassified | unclassified process: warn -> alert; error+ -> escalate (advisor-eligible) | |
| R9-process-exit | abnormal exit, warn+ | alert |
| R10-remediation | nerve `action.result` rate-limited / circuit-open, `circuit.open` | circuit + escalate once |
| R99-default | other: error+ escalate (advisor-eligible), warn alert | |

Duplicate alerts for the same subject+rule within `decision_cooldown_s`
(120 s) are suppressed (counted in `decisions_suppressed`). Restart
decisions are never suppressed by the cooldown; they are bounded by the
cortex caps (2/unit/h, 6/h total: the same numbers as the nerve's own caps, which the nerve applies only to EXECUTED restarts, so in dry-run the cortex caps are the only ones; slot 235) and the circuit.

## Safety constraints (Mate, 2026-10-21) and where they live

| constraint | enforcement |
|---|---|
| actions dry-run until proven | `actions = "dry-run"` default; request `dry_run: true`; nerve also needs `NERVE_EVT_ACTIONS=live` + reflex live |
| never remove/stop the VPS, DNS, mail, backup | `guard::PROTECTED_UNITS` (named/bind9/unbound/dnsmasq/resolved, postfix/exim/dovecot, backup/restic/borg, reboot/poweroff/shutdown/halt/kexec) + `ProtectedHosts` |
| never stop sshd, cloudflared, networking | `ssh`, `cloudflared`, `network*`, `netplan`, `dhcp`; cloudflared is removed from the restart allowlist even though the nerve reflex ALLOW has it |
| never touch /opt/galion/openmind | any event naming the path -> R1 alert only |
| no deletes | the action type is restart only |
| HADES security mode unchanged | the cortex has no HADES interface; `hades` units are protected |
| no OpenRouter, directly or via Hermes | advisor endpoint guard (below), nerve link is loopback-only |

## LLM advisor (OFF)

- **Not compiled in by default**: the HTTP transport is behind the cargo
  feature `llm-advisor` (default off). Without it the advisor cannot send
  a byte, and `llm.enabled = true` is rejected at config load. Building
  with the feature and enabling it needs **Mate's approval**.
- Config (`llm` section, defaults): `enabled=false`,
  `endpoint=https://api.river.ai/v1`, `model=deepseek-ai/DeepSeek-V4.1-Flash`,
  `key_env=RIVER_API_KEY` (the NAME only; the key is read at call time,
  passed to curl on stdin, never logged; not wired anywhere today),
  prices $0.12 / $0.0024 (cached) / $0.48 per M tokens,
  `budget_usd_total=15` (hard ceiling, clamped), `calls_per_hour=6`,
  `max_tokens=400`, `max_prompt_chars=6000`, `min_severity=error`.
- Order of checks before any call: enabled -> endpoint guard (refuses
  anything containing `openrouter`, any Hermes gateway `hermes`/`:8642`,
  non-https except loopback) -> severity floor -> calls/hour (persisted) ->
  budget (spent + worst case <= cap) -> key present. Worst case is
  reserved in `llm-spend.json` before the call and settled to the reported
  usage after; a crash mid-call counts the reservation as spent; an
  unreadable ledger refuses (fail closed).
- Output: one JSON line choosing one offered choice; anything else
  (including any command text) becomes `escalate`. A chosen restart passes
  every rule-side guard again (allowlist, protected, circuit).
- Compatible with `swarm/no-openrouter-nerve-cortex` 49163e0: the advisor
  lives in `gaia-cortex`, not in `openmind-nerve/src` (the scan target of
  `tests/no_openrouter.rs`); the nerve keeps `LLM_ENABLED=false`.

## Config

JSON file (`--config` / `CORTEX_CONFIG`) + env overrides `CORTEX_LISTEN`,
`CORTEX_SOCKET`, `CORTEX_DATA_DIR`, `CORTEX_NERVE_URL`, `CORTEX_NERVE_STREAM`,
`CORTEX_PULL_EVERY_S`, `CORTEX_ACTIONS`, `CORTEX_COALESCE_MS`,
`CORTEX_OUTBOX`, `CORTEX_INCIDENT_QUIET_S` (one-shot incidents, default
1800 s), `CORTEX_INCIDENT_STICKY_MAX_S` (default 0 = never),
`CORTEX_LLM_ENABLED`. `gaia-cortex config` prints the
effective config. Listen and nerve URL must be loopback.

## Going live (not done; needs approval)

1. Nerve: deploy the event-mode nerve (`NERVE_MODE=event`) first; it is the
   only actuator.
2. Cortex unit (`gaia-cortex.service`, user `galion-cortex` or DynamicUser,
   `CORTEX_NERVE_URL=http://127.0.0.1:8670`, data dir
   `/var/lib/gaia-cortex`, `actions=dry-run`), observe decisions for days.
3. Prove dry-run decisions against reality, then Mate decides on live
   (`actions=live` + nerve `NERVE_EVT_ACTIONS=live` + reflex live).
4. Advisor: only after a separate approval (feature build + key wiring).

## Chaos (T5.1, 2026-10-21 +02:00)

`crates/gaia-cortex/tests/real_t51_chaos.rs` (`GALION_REAL=1`,
`GALION_NERVE_BIN`) runs the release cortex against the real event-mode
nerve on box ports 20970-20979: unit kill, restart loop, protected pieces,
grouping, malformed bursts, slow and hung nerve, nerve restart mid-incident,
LLM advisor off. Results, findings and the fixes they led to:
[../real-tests/cortex-t51-chaos.md](../real-tests/cortex-t51-chaos.md).

## API under load (slot 236, 2026-10-22 +02:00)

Reads never wait for the decider: every GET answers from a snapshot
published after each batch when the engine is busy (`busy: true`,
`snapshot_age_ms`, header `x-cortex-busy: 1`); ingest never takes the engine
lock; `POST /escalations/ack` waits at most 2 s, then 503. The API server
evicts the oldest connection still sending its request when all 32 slots
are taken (`galion-common` `Limits::evict_idle`), so an idle-connection
flood no longer gives 503s. Before/after p50/p99 numbers:
[../real-tests/cortex-api-load.md](../real-tests/cortex-api-load.md).
## Incident lifetime and nerve incident backfill (slot 237, 2026-10-22 +02:00)

- **Backfill on every stream (re)connect.** Before streaming, the cortex
  reads `GET /stats` (seq) and then `GET /incidents` from the nerve. Every
  open nerve incident whose cause it does not hold becomes a synthetic
  `incident.opened` (`data.backfill = true`) decided by R11 (alert,
  critical = escalate, never an action); every held cause the nerve no longer
  has open becomes a synthetic `incident.resolved` ("reconciled"). The first
  stream starts at the seq read before the snapshot, so nothing falls between
  the two. Fixes the live miss (mouth `inc-d7320b38`, gaia-cortex
  `inc-5d566881` opened before the cortex started never reached it).
- **Nerve incidents are keyed by cause.** bb287cc publishes
  `incident.opened {"id": <cause>, "cause": <cause>}` and
  `incident.resolved {"id": <cause>}` while `/incidents` shows `inc-...`
  ids. The cortex fills in `cause` on resolves, drops an `opened` for a cause
  it holds (replay after backfill, restarted nerve), and follows a restarted
  nerve's new `inc-...` id without a second alert.
- **An incident resolves only when its own signal clears** (T5.1 F3). A
  subject whose last burst was above info stays *active* and is carried into
  every later burst under the cause it was grouped into (not hard: carried
  signals never fold into a new `stack-wide`; decisions still use the burst
  alone). It clears on: unit back to active/running or cleanly
  inactive/dead; nerve `incident.resolved`; threshold/other info burst. The
  incident resolves in that burst (`resolved_by: cleared`). One-shot subjects
  (process exits, tasks, unclassified, other) expire after
  `incident_quiet_s` of their own silence (`expired`); units, nerve
  incidents and thresholds never expire unless `incident_sticky_max_s` > 0
  (`stale`). `/incidents` gains `active`, `nerve_incidents` and per-resolved
  `resolved_by`; `/stats` gains `nerve_incidents_*`, `subjects_active`,
  `incidents_resolved_{cleared,expired,stale}`.
- Known limit: the nerve publishes start/stop only for lifecycle-tracked
  groups (nerve, cortex, gaia, hermes, console, galion-os, swarm, plus
  cloudflared, tuner-quick). A failed postfix/ssh unit therefore stays open
  until `incident_sticky_max_s` (owner decision D-237-2).

Tests: `tests/incident_lifecycle.rs` (11) and the real
`tests/real_t237_backfill.rs`; record:
[../real-tests/cortex-incident-backfill.md](../real-tests/cortex-incident-backfill.md).
