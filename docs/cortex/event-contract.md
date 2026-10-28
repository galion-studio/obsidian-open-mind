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
| `count` | occurrences: the nerve folds duplicates of one `key` inside its dedupe window (600 s) into the stored event and only raises its ring `count`; the cortex counts every occurrence toward the flap rule and re-reads the ring (`GET /evt`, transport 1b) for the failures it decided on (slot 235) |

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
| 1b | **repeat watch** `GET <nerve>/evt?since=<oldest watched seq - 1>&n=1000` every `repeat_poll_s` (30 s; `CORTEX_REPEAT_POLL_S`, 0 = off), only while a unit failure decided on in the last `flap_window_s` is watched; a `count` rise becomes a repeat event (`count` = new occurrences, `data.repeat = {of_seq, total, new}`) | 0 when nothing is watched (condvar); at most one loopback GET per `repeat_poll_s` while a crash loop is open | on with the stream |
| 2 | push `POST <cortex>/v1/events` (one object, JSON array or NDJSON, max 1000; extra items are counted `invalid`, not silently dropped) | 0 | always on |
| 3 | unix socket (`socket` / `CORTEX_SOCKET`), NDJSON lines | 0 | off |
| 4 | pull `GET <nerve>/events?n=200` every `pull_every_s` (polling nerve only) | one loopback GET per tick | off |
| 5 | file `gaia-cortex replay FILE` | n/a | CLI |

Reconnect: backoff 1 s -> 60 s, resume with `since=<last seq>` (the nerve
replays from its ring, or sends `stream.gap`). Before every (re)connect the cortex
reads the nerve's `GET /stats` (`started`, `seq`): a different `started` or a
`seq` below the cursor means the nerve restarted (its seq starts at 1 again),
so the cortex resumes from `since=0` and forgets the old seq fingerprints
(T5.1 chaos S8; before the fix the cortex was blind to the new nerve until
its seq passed the old cursor). A nerve without
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
- Request written but no reply (read timeout, reset; a hung or slow nerve):
  decision `action.by = "unknown"` (dry-run: status `simulated`; live:
  `failed`). It counts toward the cortex cap and is escalated, never
  retried: in the T5.1 hung-nerve run the nerve logged the request after
  SIGCONT. Only a refused connect is `by = "local"` (never delivered).
- `refused`, `rate-limited`, `failed` and `circuit-open` escalate to a human.

## 3. cortex -> nerve: alerts

`POST <nerve>/event` with `{"source":"gaia-cortex","kind":"cortex.alert"|"cortex.escalation","severity":...,"data":{...}}`.
They appear on the stream and are dropped on ingest by kind.

## 4. cortex API (loopback, default 127.0.0.1:8674)

`GET /health`, `/stats`, `/incidents`, `/decisions?n=`, `/alerts?n=`,
`/escalations`; `POST /v1/events`, `POST /escalations/ack?id=`.
`/health` never waits for the decider: while a decision is in progress it
answers from the last snapshot with `busy: true` (the other GETs still wait
for the engine lock; see `docs/real-tests/cortex-t51-chaos.md`).

`/health` `nerve` is real reachability (slot 347, F-337-1), not "a nerve URL
is configured": true only when the newest nerve contact attempt (stream
connect, any stream line incl. keepalives, `/stats`, `/incidents`, pulls,
`POST /actions`, `POST /event`) succeeded and the last success is at most
`nerve_stale_s` old (`CORTEX_NERVE_STALE_S`, 0 = auto: 660 s with the stream,
3 x `pull_every_s` min 30 s with pulls only). Also `nerve_configured` and
`nerve_contact` `{state: ok|stale|unreachable|unknown|none, last_ok, age_s,
last_fail, fail_age_s, last_error, ok, fail, stream: connected|down|off,
stale_s}`. Read live on every request, also from the busy snapshot path.
A killed nerve reads false at once (the stream breaks); a hung nerve that
holds the socket open reads false after `stale_s`. Real test:
`docs/real-tests/cortex-health-nerve-live.md`.

## 5. Proposed for the nerve worker (optional, non-breaking)

1. `circuit.open` event (`unit`, `until`) when the nerve's own breaker opens
   (today the cortex infers it from `action.result` = `rate-limited`).
2. Inventory: emit unclassified processes as kind `process.unclassified`
   with `data.pid`, `data.comm` (the cortex maps `inventory.unclassified` too).
3. Keep `request_id` <= 64 chars echoed verbatim (the cortex uses
   `dec-<n>-<incident>`, about 20 chars).
