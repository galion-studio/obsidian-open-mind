<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Cortex S2: event feed (T2.10) and hades/sandbox sensor (T2.13)

- Written: 2026-10-20 23:25 CEST (+02:00), branch `swarm/cortex-s2-d`, base `swarm/cortex-sensor` e536446.
- Code: `crates/openmind-nerve/src/feed.rs`, `src/sensors/hades.rs`; shared edits: 2 lines in `sensors/mod.rs`,
  1 module line + 1 doc line in `lib.rs`, 1 route (4 lines) + 1 doc line in `server.rs`. No edit to engine.rs,
  cortex.rs, config.rs. No new dependency, no new listener.

## T2.10 `GET /api/v1/cortex/events` (also `/cortex/events`)

`Cortex::nerve_events` of the last cycle (B41 rule: one event per `(source, kind)`, worst severity wins, info-only
pairs skipped) served on the existing loopback API. The accept loop already drops every non-loopback peer
(`server::serve`) and the nerve binds 127.0.0.1 by default.

| query | answer |
|---|---|
| (none) | `application/x-ndjson`: one B33 `NerveEvent` per line (deserialises into `openmind_schema::NerveEvent`) |
| `since=C` | empty unless the nerve finished a cycle after `C` (every event carries `data.cycle`: the poller's cursor) |
| `format=json` | `{"cycle", "ts", "events": [..]}`, same events |

`data` = `{cycle, findings, ids (max 32), ids_more, top: {id, component, cause, evidence_n}}`. Titles, fixes and
evidence lines are dropped and every string is cut to 96 printable chars: titles can hold addresses
(`port 5173 listens on 0.0.0.0`), so the feed is counts and ids only.

Example (real run, 2026-10-20 23:19:23 +02:00):

```
{"data":{"cycle":1,"findings":4,"ids":["hades-token-abuse","hades-safety-refusals","hades-unapprovable-actions","sandbox-escape-attempts"],"ids_more":0,"top":{"cause":"hades","component":"hades-gate","evidence_n":3,"id":"hades-token-abuse"}},"kind":"vulnerability","severity":"high","source":"hades","ts":"2026-10-20T23:19:23+02:00"}
```

## T2.13 sensor `hades`

Off unless configured (`NERVE_HADES_LEDGER`, `NERVE_HADES_AUDIT` comma list, `NERVE_HADES_DISABLE_FILE`, or
`NERVE_HADES=1` for journal only). `NERVE_HADES_WINDOW_S` (default 900), `NERVE_HADES_BURST` (default 20).
Files are tailed only when `Input.live` (max 256 KiB per file per cycle; first read = the last 256 KiB;
truncation/rotation restarts at 0; half-written lines wait). Journal lines of units `hades*` are read in replay too.
Counting is by each line's own Zagreb `ts` in a sliding window.

| finding id | kind | sev | from |
|---|---|---|---|
| `hades-safety-refusals` | vulnerability | warn, error at >= 5 (+signal) | ledger `refused`, reason `admin-safety: ...` (sub-class: destructive, secret-exposure, admin-access, protected-target, opaque-plan) |
| `hades-token-abuse` | vulnerability | error, critical at >= 3 (+signal) | forged, replayed, wrong-scope, malformed tokens |
| `hades-unapprovable-actions` | vulnerability | warn | `kill` / `rotate` / non-allow-listed actions |
| `hades-ledger-failing` | bleeding | error (+signal) | `ledger corrupt/unavailable` refusals, unparsable ledger lines, hades-gate journal |
| `hades-refusal-burst` | bleeding | warn | >= burst refusals of any class (agent loop) |
| `hades-off` | bleeding | info | disable file present, or `stopped by admin request` in the journal |
| `sandbox-escape-attempts` | vulnerability | warn, error at >= 5 (+signal) | hermes-tools audit `refused`: path traversal, outside worktree, symlink, denied path |
| `sandbox-not-enforced` | vulnerability | warn | hermes-tools `ran` with a layer in `sandbox.not_enforced` whose reason does not say "policy", or "sandbox refused to start" |
| `agent-refusal-burst` | bleeding | warn | >= burst refused coding-tool calls |

Signals use cause `hades` (component `hades-gate`) or `sandbox` (component `hermes-rs`). Evidence is class counts
plus the window (`destructive: 1`, `window: 900 s`); plans, targets, paths, sessions, token hashes and reason
details never leave the sensor (unit-tested against the fixtures and in the real run).

HADES compatibility: reasons are matched against hades-gate `Refusal::reason` as of ccaab42; swarm/hades-redteam
e6ceca7 (S3.1 allow-list) changes no reason string (`plan cannot be reviewed` covers its allow-list refusals), so
no dependency on that branch. A changed reason lands in `OtherRefusal` (counted only in the burst), not lost.
