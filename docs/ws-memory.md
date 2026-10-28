# ws/memory: openmind-memory (Rust replacement for memory-gateway)

Tickets B58 (study), B59 (Rust v0), B60 (shadow run + switch, **staged only**). Branch `ws/memory`.
All times Europe/Zagreb (+02:00). Study done 2026-10-18 20:58–21:10 +02:00, read-only.

## B58: what runs today

| Item | Value |
|---|---|
| Unit | `/etc/systemd/system/memory-gateway.service`, `User=root`, `Restart=always`, `EnvironmentFile=/etc/memory-gateway.env` |
| Start | `WorkingDirectory=/opt/galion/memory-tencentdb/node_modules/@tencentdb-agent-memory/memory-tencentdb`, `ExecStart=/opt/galion/tooling/node-memory/bin/node --import tsx/esm src/gateway/server.ts` |
| Package | `@tencentdb-agent-memory/memory-tencentdb` **1.0.3** (gateway `VERSION` 0.1.0), 55,757 TS lines in `src/`, with local Galion patches (L0 fallback in `/recall`, session-scoped persona filter, pyramid markdown export, offload canvas) |
| Listen | `127.0.0.1:8420` |
| RSS | node 614,664 kB + esbuild service child ~10.9 MB |
| Env names (values never read) | `TDAI_LLM_API_KEY`, `TDAI_LLM_BASE_URL`, `TDAI_LLM_MODEL` (OpenRouter, for the LLM extraction pipeline), `TDAI_GATEWAY_HOST`, `TDAI_GATEWAY_PORT`, `TDAI_GATEWAY_API_KEY` (**set**: v1 and v2 routes need `Authorization: Bearer`), `TDAI_DATA_DIR=/var/lib/memory-tencentdb`, `TDAI_INSTANCE_ID=galion`, `SCANNER_INSTANCES=galion`, `MEMORY_TENCENTDB_GATEWAY_HOST/PORT`, `MEMORY_TENCENTDB_LLM_*` |
| Embeddings | **none**: `/health` → `stores.embeddingService:false`. No vec0 tables (dims 0). Recall = FTS5 BM25 + L0 text fallback. So the Rust port needs **no embedding provider and no outbound calls**. |
| DB | `instances/galion/vectors.db` (3.7 MB + WAL). `vectors.db` at the root is the stale `default` instance (untouched since 2026-10-13; all 4 rows also in the live instance; archive runbook docs/ops/legacy-vectors-archive.md). No sqlite3 CLI on the VPS; schema taken from `src/core/store/sqlite.ts`. |
| Files | `persona.md` (L3), `.metadata/{scene_index,checkpoint,manifest}.json`, `scene_blocks/*.md` (L2), `conversations/YYYY-MM-DD.jsonl` (L0), `records/*.jsonl` (L1), `offload/<session>/{refs/*.md,pending.jsonl,canvas.mmd,state.json,mmds/}`, `pyramid/l0..l3` |
| Hermes | `/root/.hermes/config.yaml`: `memory.provider: memory_tencentdb`. Plugin `/opt/galion/tooling/hermes-agent/plugins/memory/memory_tencentdb` is a symlink into the npm package (`hermes-plugin/memory/memory_tencentdb/{__init__,client}.py`, Galion-patched with offload calls). Client sends `Authorization: Bearer <MEMORY_TENCENTDB_GATEWAY_API_KEY or TDAI_GATEWAY_API_KEY>` and, on `/v2/*`, `x-tdai-service-id: galion`. |

Journal, last 2 h (paths only): `GET /health` 321,314 (watchdog), `POST /recall` 68, `POST /v2/offload/query-mmd` 68,
`POST /capture` 54, `POST /session/end` 40, `POST /v2/offload/ingest` 22.

### Endpoint inventory: 27 total, Hermes uses 9, all 9 implemented

| # | Endpoint | Hermes caller | openmind-memory |
|---|---|---|---|
| 1 | `GET /health` (no auth) | watchdog / is_available | ✅ same keys; `services.*` are `null`/`"none"` (no scanner/worker); extra keys `implementation`, `read_only`, `fts` |
| 2 | `POST /recall` | `prefetch()` every turn | ✅ full port (below) |
| 3 | `POST /capture` | `sync_turn()` | ✅ L0 JSONL + `l0_conversations` + `l0_fts`; `scheduler_notified:false` (no L1 pipeline) |
| 4 | `POST /search/memories` | tool `memory_tencentdb_memory_search` | ✅ FTS path, same text format |
| 5 | `POST /search/conversations` | tool `memory_tencentdb_conversation_search` | ✅ FTS path, same text format |
| 6 | `POST /session/end` | shutdown / on_session_end | ✅ `{"flushed":true}` (nothing buffered to flush) |
| 7 | `POST /v2/offload/ingest` | `note_tool_pairs()` | ✅ refs, pending.jsonl, canvas.mmd; L1/L1.5 LLM tasks not run |
| 8 | `POST /v2/offload/query-mmd` | `prefetch()` | ✅ incl. canvas fallback, md5[..12] `version` |
| 9 | `POST /v2/offload/read-ref` | tool `memory_tencentdb_read_node` (`full:true`) | ✅ owned-ref path check, full/line/query windows |
| 10 | `POST /seed` | client method, not called by the provider | 501 `{"error":…}` |
| 11 | `POST /v2/offload/compact` | not called | 501 v2 envelope |
| 12 | `POST /v2/instance/destroy` | not called | 501 (after auth) |
| 13–27 | 15 v2 routes: `conversation/{add,query,search,delete}`, `atomic/{update,query,search,delete}`, `scenario/{ls,read,write,rm}`, `core/{read,write}`, `pipeline/status` | not called | 501 v2 envelope (after the same auth checks) |
| – | `OPTIONS *` | – | 204 |
| – | anything else | – | 404 `{"error":"Not found: <METHOD> <path>"}` (after auth, as Node) |

### Contract details (what the port reproduces)

- **Auth.** If a key is configured: v1 → 401 `{"error":"Unauthorized: missing Bearer token"|"Unauthorized: invalid token"}` (constant-time compare);
  `/v2/*` → 401 `{"code":401,"message":…,"request_id":"req-<16 hex>"}`. `/v2/offload/*` and the 15 v2 routes additionally need a non-empty Bearer and
  `x-tdai-service-id` even when no key is configured. `/health` and `OPTIONS` are open. (Recorded live: tests/fixtures/recorded.json.)
- **`/recall`** `{query, session_key, user_id?}` → `{context, strategy?, memory_count, code:0, message:"ok", retryable:false}`; 400 `{"error":"Missing required fields: query, session_key"}`.
  1. L1 keyword search on `sanitizeText(query)` (skipped under 2 UTF-16 units): `l1_fts MATCH <OR of quoted tokens>`, `bm25` → score `r/(1+r)`, top `maxResults*2`=10, keep `score ≥ 0.3` up to 5, or all hits when ≤5 (small-corpus rule). `memory_count` = number of lines.
  2. `persona.md` minus scene nav, filtered to `- id:` blocks whose `session:` equals `session_key`.
  3. Scene navigation for `scene-<sanitized session>.md` from `.metadata/scene_index.json`.
  4. `context` = persona + nav only (**the Node gateway computes the L1 lines but drops them**; kept for parity; `OPENMIND_MEMORY_RECALL_INCLUDE_L1=1` opts in). `strategy` = configured `"hybrid"` when there is anything, absent otherwise.
  5. Because embeddings are off, always the L0 fallback: `l0_fts` BM25 (LIKE scan when `l0_fts` is empty), limit 8, deduped, **only texts containing the raw query verbatim**; prepended, then cut to 2000 UTF-16 units; then the session filter.
- **`/capture`** `{user_content, assistant_content, session_key, session_id?, user_id?, messages?}` → `{l0_recorded, scheduler_notified}`. Per message: text parts, `data:image` → `[image]`, `sanitizeText`, assistant fenced code removed, framework noise and `/commands` dropped; per-session timestamp cursor. JSONL line keys `sessionKey, sessionId, recordedAt, id, role, content, timestamp`; L0 ids `l0_<session>_<ms>_<i>_<6 hex>`.
- **Search** responses: `{results:<text>, total, strategy}` / `{results:<text>, total}` with the exact Node text layout (`Found N matching memories:` …, `Found N matching message(s):` …, `No matching … found.`), score `toFixed(3)`.
- **Offload** success envelope `{code:0,message:"ok",request_id,data}`; session ids `^[A-Za-z0-9_.:-]{1,500}$`, stored under `offload/<id with non [A-Za-z0-9._-] → _>/`. Node ids `n<alnum tool_call_id>`[..24] or `n<sha256(canonical)[..12]>`; ref markdown and canvas byte-identical to the TS renderers (golden-tested).

## B59: crate `crates/openmind-memory`

std-only HTTP/1.1 (thread per connection, `Connection: close`, 1 MiB body cap = `MEMORY_MAX_BODY_BYTES`), rusqlite `bundled` (FTS5 compiled in), serde_json, zagreb-time for log stamps. Hand-written MD5/SHA-256 and text matchers (no regex, no crypto crates). No tokio, no TLS, no outbound network.

Config (env only, loopback defaults; `OPENMIND_MEMORY_*` first, then the gateway's `TDAI_*`, so `/etc/memory-gateway.env` names work):
`OPENMIND_MEMORY_HOST|TDAI_GATEWAY_HOST` (127.0.0.1), `…_PORT` (8420), `OPENMIND_MEMORY_DATA_DIR|TDAI_DATA_DIR` (`~/.openmind/memory`), `…_INSTANCE_ID|TDAI_INSTANCE_ID` (default → `vectors.db`, else `instances/<id>/vectors.db`), `OPENMIND_MEMORY_API_KEY|TDAI_GATEWAY_API_KEY`, `OPENMIND_MEMORY_READ_ONLY=1` (DB opened `SQLITE_OPEN_READ_ONLY`, no file writes; responses keep their shape), `OPENMIND_MEMORY_RECALL_{MAX_RESULTS,THRESHOLD,STRATEGY,INCLUDE_L1}`, `OPENMIND_MEMORY_MAX_CONNS` (32).

Subcommands: `snapshot <src.db> <dst.db>` (SQLite online backup, source read-only, refuses to overwrite) and `parity <http://a> <http://b> <queries.txt> [session_key]` (read-only POSTs to both, prints match/diff with lengths + sha256 prefixes only, never content; exit 2 on any diff).

### Migration / compatibility note

- Same files, same schema: opens the existing `vectors.db` as is (`CREATE … IF NOT EXISTS` only when writable; adds the `l0_conversations.timestamp` column like the gateway, all in one transaction via `Store::migrate`; before swarm/memory-real a pre-v3 DB failed to open, see docs/real-tests/memory-real.md). Since swarm/memory-schema the version is recorded in `PRAGMA user_version` (4), a missing or v1 FTS index is rebuilt during the migration, and `/capture` has an opt-in import mode; see docs/memory-schema.md. Never creates or reads vec0 tables, so a DB with sqlite-vec tables still opens.
- FTS tokenisation: Node uses jieba (`@node-rs/jieba`, native, ~5 MB dict). The port is dictionary-free: non-CJK words split exactly as the gateway's regex fallback and as jieba does for Latin text; CJK runs become the run + 2-grams + 3-grams on both query and write side. Rows written by Node (jieba words) are still found by Rust queries (their 2/3-char words are among our n-grams) and rows written by Rust are found by Rust. Expected recall differences: only CJK queries, only in ranking/extra hits. If exact jieba parity is required later, a `jieba` cargo feature can be added (not default: size + phone builds).
- Rollback is trivial: the Rust server writes the same tables/files the Node one reads.

## B60 prep: shadow run (staged proposal, nothing run on the VPS)

1. (approval) Copy the musl binary to `/opt/galion/openmind/dist/openmind-memory` (x86_64 static, 1.9 MB).
2. Consistent copy, live service untouched:
   `openmind-memory snapshot /var/lib/memory-tencentdb/instances/galion/vectors.db /var/lib/openmind-memory-shadow/instances/galion/vectors.db`
   then `cp -a persona.md .metadata scene_blocks offload` into `/var/lib/openmind-memory-shadow/` (data never leaves the VPS).
3. `/etc/openmind-memory.env` per the header of `deploy/staged/openmind-memory.service` (port 8421, `READ_ONLY=1`, shadow dir, the Bearer line copied from `/etc/memory-gateway.env` without printing it; no LLM key).
4. `systemd-run --unit=openmind-memory-shadow -p EnvironmentFile=/etc/openmind-memory.env /opt/galion/openmind/dist/openmind-memory` (or install the staged unit).
5. Compare: `openmind-memory parity http://127.0.0.1:8420 http://127.0.0.1:8421 /root/parity-queries.txt <a real session key>` with Slavik-chosen queries; output has hashes/lengths only. Accept when all `/search/*` match and `/recall` matches for Latin-script queries. Re-snapshot right before comparing (the live DB keeps changing). Also compare `GET /health` keys and RSS (`ps -o rss`).
6. Stop the shadow: `systemctl stop openmind-memory-shadow`; `rm -r /var/lib/openmind-memory-shadow` after the report.

## Switch (for Slavik's approval; not done)

Pre-req: decide the L1/L2/L3 gap below (accept frozen persona/scene/L1 or keep Node's pipeline).
1. `systemctl stop memory-gateway` (Hermes degrades gracefully: prefetch returns "" until health is back).
2. Edit `/etc/openmind-memory.env`: `OPENMIND_MEMORY_PORT=8420`, `OPENMIND_MEMORY_DATA_DIR=/var/lib/memory-tencentdb`, remove `OPENMIND_MEMORY_READ_ONLY`.
3. `install -m 644 deploy/staged/openmind-memory.service /etc/systemd/system/ && systemctl daemon-reload && systemctl disable memory-gateway && systemctl enable --now openmind-memory`.
4. Verify: `curl -s 127.0.0.1:8420/health` shows `"implementation":"openmind-memory"`; Hermes log shows `memory-tencentdb Gateway already running`; one test chat turn → `/capture` 200.
5. Rollback: `systemctl disable --now openmind-memory && systemctl enable --now memory-gateway` (same data dir; Node's FTS rebuild is not needed).

## Gaps (v0)

- **No LLM pipeline**: L1 extraction (`records/`, `l1_records`), L2 scenes, L3 persona, the offload L1/L1.5 summaries (`mmds/`) and the timer scanner/pipeline worker are not ported. After a switch, persona/scenes/L1 stay frozen at the switch point while L0 keeps growing. Options: (a) accept (recall today is dominated by persona + L0 anyway), (b) v1: port the L1 extractor using OpenRouter via a blocking rustls client with the key from env (never hard-coded), (c) run the Node pipeline as a batch job.
- No checkpoint.json cursor persistence (in-memory per-session cursor; Hermes sends no message timestamps, so no effect).
- No pyramid markdown export (`pyramid/`), no seed, no metrics/OTel/ClickHouse.
- `read-ref` non-`full` windows count tokens as ~4 bytes/token (Node: tiktoken o200k). Hermes only uses `full:true`.
- `query-mmd` lists `mmds/*.mmd` sorted by name (Node: directory order).
- Chunked request bodies are rejected (urllib sends Content-Length).

## Universal CPU

Constraint added 2026-10-18 21:02 +02:00 (UNIVERSAL-CPU.md). Effect on the plan:
- Dropped the idea of axum/tokio and of optional jieba; kept std HTTP and dictionary-free CJK n-grams so keyword recall is fully offline and identical on every CPU.
- No embedding/vector code is needed at all (the live gateway has none); if ever added it must be plain Rust loops (no BLAS/SIMD).
- Replaced `md-5`/`sha2`/`regex` candidates with small portable hand-written code; MD5 constants are a literal table (no `sin()` at runtime, which could differ by libm).
- The only C is SQLite (rusqlite `bundled`, FTS5). Cross C compiler: `zig cc` 0.13 (downloaded to `/workspace/oom-tools`, box-only); Rust links with `rust-lld`. Script: `crates/openmind-memory/cross.sh`.

Results (2026-10-18 21:16–21:20 +02:00, release profile opt-level z, lto, strip, panic=abort):

| Target | Tier | Result | Size |
|---|---|---|---|
| x86_64-unknown-linux-gnu | host | build + 23 tests + clippy -D warnings | 1,687,336 B |
| x86_64-unknown-linux-musl | 1 | build, static, runs on the box | 1,980,096 B |
| aarch64-unknown-linux-musl | 1 | build | 1,809,752 B |
| armv7-unknown-linux-musleabihf | 2 | build | 1,769,892 B |
| aarch64-linux-android | 2 | `cargo check` OK for all Rust code; bundled SQLite C needs bionic headers (no NDK on the box), check run with musl headers for that one C file. On Termux: `pkg install rust clang` and build natively. | – |

RSS (x86_64): gnu build 4.4 MB VmHWM after 200 captures + 400 recall/search requests on synthetic data; static musl build 2.5 MB VmHWM after 200 captures + 200 recalls. Node gateway: 614.7 MB (≈ 140–250× less).
