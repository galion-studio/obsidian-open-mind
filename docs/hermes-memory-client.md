<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# hermes-rs memory client (openmind-memory recall + capture)

Written 2026-10-28 17:40 (+01:00) Europe/Zagreb, wave-4 slot 403 `hermes-rs-memory-client`,
branch `swarm/hermes-rs-memory-client` off `memory-db-stale-diag` 1d7164b4. This is the
**D-388-1 prep** (recommended option): the Rust gateway gets the recall + capture client that the
Python Hermes `memory_tencentdb` plugin had, so the long-term memory store stops being frozen
(slot 388 found the last write 2026-10-18 21:42 +02:00, when Python Hermes retired).
**Staged only. Off by default. Nothing deployed. The owner still decides D-403-1.**

## What it does

| Python plugin (`memory_tencentdb`) | hermes-rs (`crates/hermes-rs/src/memory.rs`) |
|---|---|
| `prefetch` -> `POST /recall {query, session_key, user_id}` | `MemoryClient::recall` before each turn, timeout 3 s (Python: 10 s) |
| text `## memory-tencentdb Memory\n{context}` inside upstream `<memory-context>` fence + system note, appended to the API copy of the current user message | same text, same fence and note (`context_block`), appended to the last user message of the turn only (`inject`); capped at 8000 chars |
| `sanitize_context` strips fences | `sanitize_context` (case-insensitive; fence, system note, stray close tags) on recall and on both capture sides |
| `sync_turn` -> `POST /capture` on a background thread, max 4 in flight | `capture_bg`: thread per capture, `MAX_INFLIGHT` 4, extra captures dropped and counted; timeout 10 s |
| Bearer key from `TDAI_GATEWAY_API_KEY` | key read once from `MEMORY_TENCENTDB_GATEWAY_API_KEY`, else `TDAI_GATEWAY_API_KEY` (name overridable by `HERMES_RS_MEMORY_KEY_ENV`); never logged, `Debug` prints `key_set` only |

Session key for both calls: the run's `session_key` when non-empty, else the run id
(`api::memory_key`). Capture runs only for completed runs with non-empty final text.
The stored transcript keeps the plain user text (no fence), so the block is not replayed into
later turns (upstream also only adds it to the API copy).

Failure policy: memory never blocks a chat. Recall error/timeout -> the turn runs without
memory, `recall_err` +1 and a nerve `cap_hit("hermes-rs","memory_error")`. Capture error ->
`capture_err` +1. Real test: memory server down, chat still 200 in 4-6 ms.

Safety: URL must be `http://` loopback (127.0.0.1, ::1, localhost) with no path or userinfo;
anything else is a config error and the gateway exits 2 at start (`hermes-gateway config error:
memory url must be loopback: ...`). Replies capped at 256 KiB. No new dependencies (plain
`std::net` HTTP/1.0).

Visibility: `/health/detailed` gets a `memory` object (enabled, url, key_set, recall_ok,
recall_hit, recall_err, capture_ok, capture_err, capture_dropped, capture_inflight) only when on;
`--check-config` prints `memory=<url|off>`; start log `memory on: <url> key_set=<bool> health ...`.

## Switches

| Where | Name | Default |
|---|---|---|
| env | `HERMES_RS_MEMORY` (`1`/`0`, anything else is a config error) | off |
| env | `HERMES_RS_MEMORY_URL`, else `http://$MEMORY_TENCENTDB_GATEWAY_HOST:$MEMORY_TENCENTDB_GATEWAY_PORT` | `http://127.0.0.1:8420` |
| env | `HERMES_RS_MEMORY_USER` | `default` |
| env | `HERMES_RS_MEMORY_RECALL_MS` (100-30000), `HERMES_RS_MEMORY_CAPTURE_MS` (100-60000) | 3000 / 10000 |
| YAML | `hermes_rs.memory.{enabled,url,user_id,recall_timeout_ms,capture_timeout_ms}` | off |

The Python-era `memory.provider: memory_tencentdb` / `memory_enabled: true` keys that are still in
the live `/opt/galion/hermes/config.yaml` are **ignored on purpose**, so deploying this binary
alone changes nothing on live (checked read-only 2026-10-28 17:29 +01:00: no `hermes_rs:` block).

## How to enable on live (only after the owner says yes to D-403-1)

1. Deploy a hermes-gateway built from this branch the usual staged way (:18xxx rehearsal, then
   swap), nothing else changes while the switch is off.
2. Add a systemd drop-in for `hermes-rs.service` with `Environment=HERMES_RS_MEMORY=1`. The URL
   comes from the existing `MEMORY_TENCENTDB_GATEWAY_HOST/PORT` and the key from the existing
   `TDAI_GATEWAY_API_KEY` in `/opt/galion/hermes/.env` (names seen, values not read).
3. Before that, the owner (or an agent with the owner's yes) must confirm the key in
   `/opt/galion/hermes/.env` equals the one in `/etc/openmind-memory.env`; a mismatch shows up as
   `recall_err` growth and nerve `memory_error` hits, never as a chat failure.
4. Check `/health/detailed` memory counters and the vectors.db mtime moving after a chat.
   Roll back = remove the drop-in and restart.

## Not ported (D-403-3)

`query-mmd` second recall, offload/ingest of whole sessions, session-end flush and the
`memory_tencentdb` tool calls. The two calls above are what kept the store fresh before
2026-10-18; the rest can follow if wanted.
