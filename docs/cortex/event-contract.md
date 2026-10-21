<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve <-> cortex event contract (v1)

Status: 2026-10-21 +02:00, branch `swarm/cortex-event-driven`. Code:
`crates/nerve-cortex-contract` (serde + serde_json only). The event-driven
nerve (`swarm/nerve-event-driven`, `openmind_nerve::evt`) already defines the
wire types; the cortex **consumes them as they are**. Nothing here asks the
nerve to change its wire format. Three small items are proposed for the
nerve worker to adopt (section 5).

## 1. nerve -> cortex: events

One JSON object per NDJSON line, the nerve's `evt::NerveEvent` (schema `v` 1):

```json
{"v":1,"seq":42,"ts":"2026-10-21T18:40:00+02:00","t":1792600800,"source":"journal",
 "kind":"unit.failed","severity":"high","unit":"tuner.service","group":"other",
 "key":"unit.failed:tuner.service","msg":"Failed with result 'exit-code'",
 "data":{"unit_result":"exit-code","exit_code":"exited","exit_status":"1"},"count":1}
```

The older B33 envelope (`{ts, source, kind, severity, data}`, polling nerve
`GET /events`, `openmind_schema::NerveEvent`) is accepted too: the nerve-only
fields are optional and `unit` / `seq` fall back to `data.unit` / `data.seq`.

| field | cortex use |
|---|---|
| `seq` | stream cursor (`since=`) and dedupe key (`source`+`kind`+`seq`) |
| `kind` | `EventKind` (table below) |
| `severity` | `info`/`low` -> info, `medium`/`warn` -> warn, `high`/`error` -> error, `critical` -> critical; unknown -> warn |
| `unit` | subject `unit:<unit>`; `.service` appended if no suffix; `-` and `-.slice` are not actionable |
| `data.path` / `data.metric` | subject `threshold:<path>` for `disk.crossed` / `cert.crossed` |
| `data.exit_status` / `exit_code` | abnormal-exit check |
| `data.result`, `data.source` | `action.result`: `rate-limited` / `circuit-open` open the cortex circuit; our own echoes are ignored |
| `count` | informational (the nerve already folded duplicates) |

| nerve kind | cortex `EventKind` | used by rule |
|---|---|---|
| `unit.failed`, `unit.start_failed`, `unit.spawn_failed` | UnitCrash | R3 restart / R4 unknown unit / R2 flapping |
| `unit.restart_scheduled` | RestartScheduled | R3b (systemd restarts it itself; counts toward flapping) |
| `unit.start_limit` | StartLimit | R2-start-limit (circuit + escalate) |
| `unit.exit`, `proc.exit` | ProcessExit | R9 (alert on abnormal exit) |
| `oom.kill` | Oom | R3 when it names a unit, else R99 |
| `unit.started` / `unit.stopped` / `unit.finished` | UnitState | R5 recovered (info) |
| `task.done` / `task.failed` | TaskFinished | R6 |
| `slot.done` / `slot.failed` / `slot.started` | SlotFinished | R6 |
| `disk.crossed`, `cert.crossed`, `threshold.crossed` | Threshold | R7 |
| `process.unclassified` (inventory) | UnclassifiedProcess | R8 |
| `action.result` | Remediation | R10 |
| `circuit.open` | CircuitOpen | R10 |
| `stream.keepalive`, `stream.gap` | Housekeeping | none (not counted as events) |
| `cortex.alert`, `cortex.escalation` | CortexAlert | dropped (no feedback loop) |
| anything else (`kernel.error`, `file.changed`, ...) | Other | R99 |

### Transports

| # | transport | idle cost | default |
|---|---|---|---|
| 1 | **subscribe** `GET <nerve>/events/stream?since=SEQ` (held open, NDJSON) | 0: the cortex blocks in `read()`; the nerve writes one keepalive line per 5 min | on when `nerve_url` is set |
| 2 | push `POST <cortex>/v1/events` (one object, JSON array or NDJSON, max 1000) | 0 | always on |
| 3 | unix socket (`socket` / `CORTEX_SOCKET`), NDJSON lines | 0 | off |
| 4 | pull `GET <nerve>/events?n=200` every `pull_every_s` (polling nerve only) | one loopback GET per tick | off |
| 5 | file `gaia-cortex replay FILE` | n/a | CLI |

Reconnect: backoff 1 s -> 60 s, resume with `since=<last seq>` (the nerve
replays from its ring, or sends `stream.gap`). A nerve without
`/events/stream` (404, polling mode) is retried every 10 min only.

## 2. cortex -> nerve: action requests

`POST <nerve>/actions`, body = `ActionRequest`. The nerve's
`evt::actions::parse` reads `action`, `unit`, `reason`, `request_id`,
`source`, `dry_run`; the other fields are for audit and ignored by it.

```json
{"v":1,"request_id":"dec-3-inc-1a2b3c4d","source":"gaia-cortex","ts":"2026-10-21T18:40:02+02:00",
 "incident":"inc-1a2b3c4d","action":"restart","unit":"tuner.service","dry_run":true,
 "rule":"R3-known-crash","decided_by":"rules","reason":"tuner.service entered failed state (failure 1 in 900s); restart via nerve allowlist"}
```

- `action` is always `restart` (the type has no other variant: no stop,
  disable, delete, rollback or command). `note` (nerve-side) is not used;
  alerts go to `POST /event` instead.
- `dry_run` is `true` unless the cortex runs with `actions = "live"`. The
  nerve still runs nothing unless **also** `NERVE_EVT_ACTIONS=live` **and**
  its reflex mode is live: three independent switches.
- Reply (any HTTP status, read by `ActionResult::from_nerve`):
  `{"request_id", "result": "dry-run"|"executed"|"refused"|"rate-limited"|"failed", "would_run"?, "why"?, "error"?}`
  with 200 / 403 / 429 / 500. 404/405 = nerve without `/actions`: the
  cortex simulates a dry-run request locally and refuses a live one.
- A dry-run request answered `executed`/`failed` is treated as a contract
  violation (refused + escalated).
- `refused`, `rate-limited`, `failed` and `circuit-open` escalate to a human.

## 3. cortex -> nerve: alerts

`POST <nerve>/event` with `{"source":"gaia-cortex","kind":"cortex.alert"|"cortex.escalation","severity":...,"data":{...}}`.
They appear on the stream and are dropped on ingest by kind.

## 4. cortex API (loopback, default 127.0.0.1:8674)

`GET /health`, `/stats`, `/incidents`, `/decisions?n=`, `/alerts?n=`,
`/escalations`; `POST /v1/events`, `POST /escalations/ack?id=`.

## 5. Proposed for the nerve worker (optional, non-breaking)

1. `circuit.open` event (`unit`, `until`) when the nerve's own breaker opens
   (today the cortex infers it from `action.result` = `rate-limited`).
2. Inventory: emit unclassified processes as kind `process.unclassified`
   with `data.pid`, `data.comm` (the cortex maps `inventory.unclassified` too).
3. Keep `request_id` <= 64 chars echoed verbatim (the cortex uses
   `dec-<n>-<incident>`, about 20 chars).
