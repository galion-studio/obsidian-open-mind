<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Memory gateway routes Hermes uses

- DESK-ID: openmind.B58
- Written: 2026-10-19T17:13:31+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: read-only. Which memory-gateway HTTP routes the Hermes memory provider calls.
- Not opened: `/opt/galion/hermes`, `state.db`, `.env`, vault, secrets. No request body was sent to the live gateway.

## Selected provider

The redacted snapshot `stack/hermes/config.yaml.redacted` (copy of `/opt/galion/hermes/config.yaml`, recorded 2026-10-18 19:42:38 CEST) sets:

```yaml
memory:
  provider: memory_tencentdb
```

That name is the v1 plugin `memory_tencentdb` (aliases `tdai`, `memory-tencentdb`), version 1.0.0 in its `plugin.yaml`. It is the plugin installed for the agent tree:

`/opt/galion/tooling/hermes-agent/plugins/memory/memory_tencentdb`

is a symlink to

`/opt/galion/memory-tencentdb/node_modules/@tencentdb-agent-memory/memory-tencentdb/hermes-plugin/memory/memory_tencentdb`

Package on disk: `@tencentdb-agent-memory/memory-tencentdb` 1.0.3. The sibling v2 provider `memory_tencentdb_v2` is shipped inside that package and is not linked under `plugins/memory/`. The redacted config does not select it. The live Hermes home was not opened, so a later edit of the live `config.yaml` would not be visible here.

The client default base URL is `http://127.0.0.1:8420` (`client.py`). v1 calls are JSON POST, or GET for health. `Authorization: Bearer` is added only when the client was given an API key. Values were not read.

The process listening on `127.0.0.1:8642` is `/opt/galion/rust/bin/hermes-gateway`. Its source is not in this repo (`desk-hermes` does not call the memory gateway). This study is the Python provider the config names, not a disassembly of that binary.

## Routes the selected provider calls

Caller: `hermes-plugin/memory/memory_tencentdb/__init__.py` through `client.py`.

| When | Method and path | Body fields the client sends |
| --- | --- | --- |
| startup, `is_available`, supervisor wait, watchdog | `GET /health` | none |
| `prefetch` (before the prompt) | `POST /recall` | `query`, `session_key`, optional `user_id` |
| same `prefetch`, after recall | `POST /v2/offload/query-mmd` | `session_id` |
| `sync_turn` (background thread, max 4 in flight) | `POST /capture` | `user_content`, `assistant_content`, `session_key`, optional `session_id`, `user_id` |
| `note_tool_pairs` | `POST /v2/offload/ingest` | `session_id`, `tool_pairs`, optional `prompt` |
| tool `memory_tencentdb_memory_search` | `POST /search/memories` | `query`, `limit`, optional `type`, `scene` |
| tool `memory_tencentdb_conversation_search` | `POST /search/conversations` | `query`, `limit`, optional `session_key` |
| tool `memory_tencentdb_read_node` | `POST /v2/offload/read-ref` | `session_id`, `result_ref`, `full` |
| `on_session_end` | `POST /session/end` | `session_key`, optional `user_id` |

Offload calls go through `_post_v2`. They send the same bearer header as v1, plus `x-tdai-service-id` set to a fixed local instance label in `_v2_headers`. The label is not repeated here.

`on_memory_write` is a no-op (comment says mirroring is still TODO). It does not call the gateway. `queue_prefetch` is a no-op; recall is synchronous inside `prefetch`.

`GET /health` is also what the supervisor polls (up to the plugin README wait, about 30 s) before it treats the gateway as up.

## Client method that Hermes does not call

`MemoryTencentdbSdkClient.seed` would `POST /seed` (long timeout, body `data` plus optional `session_key`, `strict_round_role`, `auto_fill_timestamps`, `config_override`). Nothing in the Hermes plugin calls `.seed(`. It is not a route Hermes uses.

## v2 provider (not selected, not installed)

`memory_tencentdb_v2` is in the same npm package and talks through the `tencentdb_agent_memory` Python SDK, not `client.py`. That SDK wheel was not found on this host. The TypeScript SDK next to it (`@tencentdb-agent-memory/memory-sdk-ts` 1.0.0) maps the same method names 1:1 onto the gateway `routeTable` in `src/gateway/v2-router.ts`. If this provider were selected, the plugin methods would call:

| Plugin method | HTTP |
| --- | --- |
| `prefetch` `search_atomic` | `POST /v2/atomic/search` |
| `prefetch` `read_core` | `POST /v2/core/read` |
| `prefetch` `list_scenarios` | `POST /v2/scenario/ls` |
| `sync_turn` `add_conversation` | `POST /v2/conversation/add` |
| tool `tdai_memory_search` | `POST /v2/atomic/search` |
| tool `tdai_conversation_search` | `POST /v2/conversation/search` |
| tool `tdai_read_scene` | `POST /v2/scenario/read` |

`on_session_end` in v2 is a no-op. The plugin comment says v2 has no session-end route and flush is timer-based. v2 routes require `Authorization: Bearer` and `x-tdai-service-id` (`parseV2Auth` in `v2-router.ts`). This provider is not the one the redacted config selects, and it is not linked into `plugins/memory/`.

## Gateway routes the selected Hermes provider does not use

From `src/gateway/server.ts`, `v2-router.ts`, and `src/offload_server/router.ts` (package 1.0.3):

v1, present but unused by the provider class: `POST /seed`.

v2 data plane, unused by the selected provider: `POST /v2/conversation/add`, `POST /v2/conversation/query`, `POST /v2/conversation/search`, `POST /v2/conversation/delete`, `POST /v2/atomic/update`, `POST /v2/atomic/query`, `POST /v2/atomic/search`, `POST /v2/atomic/delete`, `POST /v2/scenario/ls`, `POST /v2/scenario/read`, `POST /v2/scenario/write`, `POST /v2/scenario/rm`, `POST /v2/core/read`, `POST /v2/core/write`, `POST /v2/pipeline/status`, `POST /v2/instance/destroy`.

Offload, unused by the selected provider: `POST /v2/offload/compact`.

The TypeScript SDK also documents `POST /v2/cos/secret` for file reads. The Hermes v1 plugin does not call it. The local `server.ts` switch does not register that path.

## Gap versus `crates/openmind-memory`

The in-repo spare server (not edited by this ticket) implements only:

- `GET /health`
- `POST /capture`
- `POST /search/memories`
- `POST /search/conversations`
- `POST /v2/conversation/add`
- `POST /v2/atomic/search`
- `POST /v2/conversation/search`

The crate comment lists that set as "the memory routes Hermes actually calls". That matches the v2 plugin search/store subset plus v1 capture and the two v1 search tools. It does not match the provider that is actually installed and selected.

Routes the selected provider calls that this crate returns 404 for:

- `POST /recall` (every prefetch)
- `POST /session/end` (every session end)
- `POST /v2/offload/query-mmd` (every prefetch)
- `POST /v2/offload/ingest` (tool-pair notes)
- `POST /v2/offload/read-ref` (the read-node tool)

`POST /seed` is also unimplemented, and Hermes does not call it.

The three v2 routes the crate does implement are the v2 provider, which this host has not installed and the redacted config does not select. A parity switch of the live provider onto this crate would miss prefetch, session flush, and offload.

The crate refuses to bind port 8420. That guard is about the spare process, not about which routes Hermes calls.

## Listeners observed

`ss -lntp` on this host, loopback only, this hour:

| Address | Process |
| --- | --- |
| `127.0.0.1:8420` | `/opt/galion/rust/bin/openmind-memory` (cwd `/`) |
| `127.0.0.1:8431` | same binary (cwd `/root/growth-20261018`) |
| `127.0.0.1:8432` | `openmind-memory.new` (deleted inode, cwd `/root/growth-20261018`) |
| `127.0.0.1:8433` | same deleted binary |
| `127.0.0.1:8642` | `/opt/galion/rust/bin/hermes-gateway` |
| `127.0.0.1:8652` | `hermes-gateway --mock` |
| `127.0.0.1:8653` | `hermes-gateway.new --mock` |

One `GET http://127.0.0.1:8420/health` returned 200, `Content-Type: application/json`, 277 bytes. Keys: `status` `ok`, `version` `0.1.0`, `uptime` (seconds), `stores.vectorStore` true, `stores.embeddingService` false, `services.timerScanner` null, `services.pipelineWorker` null, `services.stateBackend` `none`, `implementation` `openmind-memory`, `implementation_version` `0.1.0`, `read_only` false, `fts` true. No token field was present. The other memory ports were not requested. No body was posted.

`galion-one` also GETs `/health` on 8420. That is not a Hermes route.

## What this study did not do

- Did not open the Hermes home, `state.db`, env files, vault, or secrets.
- Did not change `crates/openmind-memory`.
- Did not claim the running Rust `hermes-gateway` binary issues these calls. The calls are in the Python provider the redacted config selects.
- Did not curl any route except that single health GET.

## `/recall` `scope` (slot 426, 2026-10-28 21:58 +01:00 Europe/Zagreb)

openmind-memory (Rust) accepts an optional `scope` on `POST /recall`:

| `scope` | L1 keyword lines + L0 text fallback | reply |
|---|---|---|
| absent / `null` | every session of the store (legacy gateway parity, unchanged) | no `scope` key (byte-identical to before) |
| `"all"` | every session | `"scope": "all"` |
| `"session"` | only rows whose `session_key` equals the request's (SQL filter before the limit) | `"scope": "session"` |
| anything else | - | 400 `{"error": "scope must be \"session\" or \"all\""}` |

Persona and scene blocks were already per session and are unchanged. The echo lets a client
tell this server from an older one, which ignores the field and answers from every session.
hermes-rs sends `scope: "session"` by default and drops any reply without the echo
(see docs/hermes-memory-client.md).
