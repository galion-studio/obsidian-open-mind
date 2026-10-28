<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve event contract (event mode, schema v1)

Branch `swarm/nerve-event-driven`, crate `openmind-nerve`, module `src/evt/`.
Consumer: the event-driven cortex (`swarm/cortex-event-driven`). This file is
the contract between the two; adding fields is non-breaking, removing or
renaming a field or kind bumps `v`.

## Turning it on

`openmind-nerve serve --mode event` or `NERVE_MODE=event`. Without it the
binary is the unchanged polling nerve (live today: `NERVE_INTERVAL=30`, 120 full
cycles an hour). All existing routes (`/health`, `/state`, `/events`,
`POST /event`, `POST /signal`, ...) answer the same way in both modes.

## Ownership split (nerve vs cortex)

- The **nerve** senses, dedupes, debounces, publishes, and executes allowlisted
  actions. It does **not** spend: default escalation mode is `delegate`
  (batch published on the stream, zero cost).
- The **cortex** subscribes to the stream, owns the LLM advisor and the whole
  budget (~$15), and sends action requests back with `POST /actions`.
- The nerve's own `openai` escalation slot exists only as a fallback, is
  disabled by default and must stay off while the cortex owns the budget, so
  the two never both spend.

## Wire format: one event (`NerveEvent`)

One JSON object per line (NDJSON). Example (captured on the VPS sandbox):

```json
{"v":1,"seq":14,"ts":"2026-10-21T18:42:25+02:00","t":1792600945,"source":"journal",
 "kind":"unit.failed","severity":"high","unit":"nerve-evt-fail.service","group":"nerve",
 "key":"unit.failed:nerve-evt-fail.service","msg":"nerve-evt-fail.service: Failed with result 'exit-code'.",
 "data":{"message_id":"d9b373ed55a64feb8242e02dbe79a49c","unit_result":"exit-code","exit_code":null,
         "exit_status":null,"job_type":null,"job_result":null,"n_restarts":null,"gaia":null},"count":1}
```

| field | type | meaning |
|---|---|---|
| `v` | u32 | schema version, 1 |
| `seq` | u64 | monotonic per nerve process, starts at 1 (resets on nerve restart: watch for `nerve.started`) |
| `ts` | string | Europe/Zagreb RFC 3339 with offset |
| `t` | u64 | unix seconds |
| `source` | string | `journal`, `inotify`, `pidfd`, `threshold`, `http`, `engine`, `inventory`, `escalation`, `actions`, `nerve` |
| `kind` | string | see the kinds table |
| `severity` | string | `info` < `low` < `medium` < `high` < `critical` |
| `unit` | string? | systemd unit when known |
| `group` | string | `nerve`, `cortex`, `gaia`, `hermes`, `console`, `sandbox`, `galion-os`, `swarm`, `infra`, `transient`, `user`, `other` |
| `key` | string | dedupe key; identical keys within `NERVE_EVT_DEDUPE` (600 s) are one event |
| `msg` | string | human text, max 300 chars, no secrets |
| `data` | object | kind-specific fields |
| `count` | u32 | duplicates folded in so far (an event re-read from `/evt` may show a higher count than when streamed) |

## Kinds

| kind | source | severity | when |
|---|---|---|---|
| `unit.exit` | journal | high (stack), medium (other), low (transient/sandbox) | main process exited non-clean (`EXIT_STATUS` in data and key) |
| `unit.failed` | journal | same rule | "Failed with result" |
| `unit.start_failed` | journal | same rule | job failed |
| `unit.spawn_failed` | journal | same rule | exec failed |
| `unit.restart_scheduled` | journal | same rule | systemd restart counter moved |
| `unit.start_limit` | journal | critical (stack) | `start-limit-hit` = failed restart, systemd gave up |
| `oom.kill` | journal | critical (stack, `-.slice`, kernel) / low (sandbox) | OOM killer |
| `kernel.error` | journal | medium | kernel prio err+ (none inside the LXC) |
| `unit.started` / `unit.stopped` | journal | medium | lifecycle of tracked units (stack groups + cloudflared + tuner-quick + `NERVE_EVT_UNITS`); `unit.started` also for any unit with an open unit fault incident (it joins the journal filter while faulted, slot 246) |
| `unit.finished` | journal | info | tracked unit "Deactivated successfully" (task finished) |
| `unit.stopping` | journal | info | systemd started a stop job for a tracked or faulted unit ("Stopping ...", `JOB_TYPE` stop/restart, `_PID` 1). `data {message_id, job_type, invocation_id, planned: true, gaia}`, key `unit.stopping:<unit>:<inv8>`. The engine remembers it 1 h per unit (slot 310) |
| `unit.planned_stop` | journal | info (clean exit: status 0/143/130/15, TERM, INT; result exit-code/signal/success), low (unclean exit), medium (other result, e.g. timeout) | the `unit.exit` or `unit.failed` of the same unit and `INVOCATION_ID` that a `unit.stopping` opened (TTL 300 s, closed by the next `unit.started`). Replaces those kinds, so consumers that count `unit.failed`/`unit.exit` (cortex R3, unit fault incidents) ignore planned stops. `data` keeps the original fields plus `planned: true`, `planned_kind` (`unit.exit`/`unit.failed`), `stop_job`, `stopping_t`, `invocation_id`; key `unit.planned_stop:<unit>:<was>:<inv8>` (slot 310, F2 of slot 271) |
| `proc.exit` | pidfd | medium | a tracked service main process exited (independent of journald) |
| `task.done` / `task.failed` | inotify | medium / high | `*.done|*.ok` / `*.failed|*.fail|*.error` written into a watched dir |
| `file.changed` / `file.deleted` | inotify | low | other files in a watched dir |
| `slot.started` / `slot.done` / `slot.failed` | inotify, http | info / medium / high | new line in a watched record file (`START|RESUME`, `DONE`, `FAILED`), or `POST /event` |
| `disk.crossed` | threshold | medium/high/critical up, info down | disk use crossed 85/90/95 % (`NERVE_EVT_DISK_PCT`) |
| `cert.crossed` | threshold | medium/high/critical | cert expiry crossed 30/14/3/0 days (`NERVE_EVT_CERTS`) |
| `incident.opened` / `incident.resolved` | engine | from incident sev / info | the engine cycle correlated signals into an incident. `data`: opened `{id, cause, components, next, opened}`, resolved `{id, cause, components, opened, resolved_by: "engine"}` (`id` = cause). Keys `incident.opened:<cause>:<opened>` / `incident.resolved:<cause>:<opened>`, so a cause that opens again inside the dedupe window is a new incident (slot 246) |
| `incident.opened` / `incident.resolved` | units | failure sev / info | unit fault incident (slot 246): the first failure event (`unit.exit/failed/start_failed/spawn_failed/restart_scheduled/start_limit`, `oom.kill`) of a non-transient, non-sandbox unit opens one incident, `id` = `cause` = `unit:<unit>` (no top-level `unit` field, so a consumer files it under the cause); later failures only count. Resolved once the unit runs again and stays up `NERVE_EVT_RECOVER_HOLD` s (default 30) without a new failure: `data {id, cause, unit, resolved_by: journal:started\|journal:finished\|inventory:active\|inventory:inactive\|inventory:gone, state, opened, open_s, faults}`. Also listed in `GET /incidents` (`source: "units"`) and `GET /stats` `units` |
| `unit.state` | units | info | the faulted unit is running again (`data.to` / `active_state` = `active` or `inactive`, `recovered: true`); published right before the unit `incident.resolved` |
| `signal.error` | http | medium | `POST /signal` with kind `error` (nerve-client) |
| `<any>` | http | from body | `POST /event` (gaia-live body) |
| `inventory.changed` | inventory | low | running services / listening ports changed |
| `escalation.batch` | escalation | batch max | a coalesced batch, delegated to the cortex (`data.events` = the events) |
| `action.result` | actions | info..high | every `POST /actions` request and its result |
| `nerve.started` / `nerve.heartbeat` | nerve | info | start; liveness every `NERVE_EVT_HEARTBEAT` (900 s) |
| `nerve.source_down` | nerve | high | journalctl died (restarted with backoff, resumes after the last cursor) |
| `nerve.watch_overflow` | inotify | medium | inotify queue overflow |
| `stream.gap` / `stream.keepalive` | nerve | info | stream only, never stored |

## Endpoints

| route | what |
|---|---|
| `GET /events/stream?since=SEQ&min=SEV` | NDJSON, long-lived. `since` replays the ring (2048 events) first; a `stream.gap` line if events were already dropped. Keepalive line every 5 min when idle. Max 4 streams. |
| `GET /evt?since=SEQ&n=N&min=SEV` | JSON array (catch-up without a stream; `since=0` = last N) |
| `GET /inventory` | grouped inventory (503 `warming_up` before the first discovery) |
| `GET /stats` | wakes/escalations per hour (sliding hour and average), by source, engine cycles, dedupe, LLM calls/USD, config |
| `GET /escalations` | last 100 batches with outcome (`delegated`, `logged`, `called`, `refused`, `failed`) |
| `POST /actions` | action request (below) |
| `GET /actions` | last 100 action results |
| `GET /events`, `POST /event` | unchanged; medium+ evt events are also appended to the JSONL log (`source":"nerve-evt"`) |
| `GET /health` | unchanged, 200 |

## Action requests (`POST /actions`)

```json
{"action":"restart","unit":"hermes-rs.service","reason":"crash loop","request_id":"cx-17","source":"cortex","dry_run":true}
```

- `action`: `restart` or `note`. Nothing else is accepted (400).
- `restart` only for units on the reflex allowlist (`reflex::ALLOW`) and never
  for the never-touch list (`galion-nerve`, `ssh`, `cron`, `dbus`, firewall,
  `tuner-quick`, ...): 403.
- Executed only if **all three** hold: `NERVE_EVT_ACTIONS=live`, the nerve's
  reflex mode is live (not `NERVE_DRY_RUN=1`, not the `mode` file), and the
  request has `"dry_run": false`. Otherwise 200 with
  `{"result":"dry-run","would_run":"systemctl restart --no-block <unit>","why":[...]}`.
- Limits when live: 2 restarts per unit per hour, 6 per hour total (429).
- Every request becomes an `action.result` event.

## Debounce, dedupe, escalation

1. Hub dedupe: same `key` within `NERVE_EVT_DEDUPE` (600 s) -> folded (`count`+1), not re-published.
2. Coalescer: only severity >= `NERVE_EVT_ESCALATE_MIN` (high; lower values are refused at start).
   A batch closes `NERVE_EVT_DEBOUNCE` (30 s) after the last event, or at most
   `NERVE_EVT_MAX_WAIT` (120 s) after the first. Burst -> one batch.
3. Warm-up: incidents found by the first engine cycle after start do not escalate.
4. Escalation (`NERVE_EVT_ESCALATE`): `delegate` (default; publish `escalation.batch`),
   `log` (NoOp, journal line), `off`, `openai` (below). Never on a timer.

### `openai` slot (disabled by default, not wired to any provider)

`NERVE_EVT_LLM_ENDPOINT` (OpenAI-compatible chat completions URL),
`NERVE_EVT_LLM_MODEL`, `NERVE_EVT_LLM_KEY_ENV` (NAME of the env var that
holds the key; names only, values never logged or exposed),
`NERVE_EVT_LLM_PRICE_IN_PER_MTOK`, `NERVE_EVT_LLM_PRICE_OUT_PER_MTOK`,
`NERVE_EVT_LLM_BUDGET_USD` (hard total cap, default 0 = nothing can be spent;
persisted in `$NERVE_DATA_DIR/evt-llm-spend.json`; the worst case is reserved
before each call; a failed call is charged the worst case),
`NERVE_EVT_LLM_CALLS_PER_HOUR` (default 4, max 60), `NERVE_EVT_LLM_MAX_TOKENS` (400).
Unless every slot is set the escalator refuses without any network call.

**OpenRouter guard** (user rule: nerve and cortex never call OpenRouter,
directly or via Hermes): any endpoint containing `openrouter` (case-insensitive,
also percent-encoded) and any Hermes endpoint (host contains `hermes`, port
8642) is refused at start (exit 2) and again right before every call; curl runs
with `--max-redirs 0`; plain `http://` only to loopback. A build-time endpoint
(`NERVE_EVT_LLM_ENDPOINT_BUILTIN`) containing `openrouter` fails to compile.

## Sources and their cost

| source | mechanism | wakes |
|---|---|---|
| journal | `journalctl -f -o json` child, server-side matches (failure IDs on all units; lifecycle IDs on tracked units; kernel prio <= 3) | one per matching entry (drained together) |
| inotify | `inotify_init1` + blocking `read` (`evt::sys`) | one per kernel batch |
| pidfd | `pidfd_open` + blocking `poll`, one 64 KiB thread per process (max 48) | one per exit |
| http | `POST /event`, `/signal` (errors), `/actions` | one per request |
| timers | heartbeat (900 s), debounce flush, deferred engine cycle | only while work is pending + 4/h liveness |

Not used: netlink proc connector (needs CAP_NET_ADMIN in the init namespace;
the VPS is an LXC), `/dev/kmsg` (absent in the LXC; OOM comes from systemd's
`fe6faa94...` message instead), D-Bus `PropertiesChanged` (journal carries the
same transitions without a D-Bus dependency; `busctl monitor` is the drop-in
alternative if needed).

## Env reference

`NERVE_MODE=event`, `NERVE_EVT_HEARTBEAT` (900, 60..3600), `NERVE_EVT_DEBOUNCE` (30),
`NERVE_EVT_MAX_WAIT` (120), `NERVE_EVT_DEDUPE` (600), `NERVE_EVT_ESCALATE_MIN` (high),
`NERVE_EVT_CYCLE_DEBOUNCE` (5), `NERVE_EVT_CYCLE_MIN_GAP` (60), `NERVE_EVT_JOURNAL` (1),
`NERVE_EVT_JOURNALCTL` (journalctl; `replay:PATH` reads a file/FIFO for tests;
`native` / `native:DIR` selects the in-process `sdjournal` follower when built with
`--features journal-native` — see `docs/third-party/nerve-journal-reader-rs.md`),
`NERVE_EVT_PIDFD` (1), `NERVE_EVT_PIDS`, `NERVE_EVT_WATCH`, `NERVE_EVT_UNITS`,
`NERVE_EVT_DISK` (/), `NERVE_EVT_DISK_PCT` (85,90,95), `NERVE_EVT_CERTS`,
`NERVE_EVT_CERT_DAYS` (30,14,3,0), `NERVE_EVT_ACTIONS` (dry-run; `live`),
`NERVE_EVT_PORT_GROUPS` (`port=group,...`), `NERVE_EVT_ESCALATE` and `NERVE_EVT_LLM_*` above.
Do not watch the nerve's own data dir (it writes there every cycle).

## Known limits (v1)

- A stream whose peer disconnected while idle holds its slot until the next
  event or keepalive (max 5 min); 4 slots.
- `seq` restarts at 1 when the nerve restarts; resume with `since=0` after a
  `nerve.started` line.
- `journalctl -f` RSS was ~38 MB on the VPS (mapped journal files, mostly
  shared; re-measured 2026-10-27 CET: ~49 MB under live nerve bb287cc). Optional
  in-process replacement: feature `journal-native` (prototype, not live).
  page cache); the nerve itself ~2.3 MB.
- Processes run by a non-root nerve: `ss -p` cannot name other users'
  sockets; ports then fall back to the well-known port table.
