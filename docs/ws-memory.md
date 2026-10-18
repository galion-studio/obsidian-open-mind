# ws/memory: openmind-memory (Rust replacement for memory-gateway)

Status: B58 study done, B59 v0 in progress (2026-10-18, Europe/Zagreb +02:00). Full contract below; shadow plan and switch steps follow in later commits.

## B58 findings (read-only study of the VPS)

- Unit: `memory-gateway.service` (`/etc/systemd/system/`), `User=root`, `EnvironmentFile=/etc/memory-gateway.env`,
  `WorkingDirectory=/opt/galion/memory-tencentdb/node_modules/@tencentdb-agent-memory/memory-tencentdb`,
  `ExecStart=/opt/galion/tooling/node-memory/bin/node --import tsx/esm src/gateway/server.ts`, `Restart=always`.
- Package: `@tencentdb-agent-memory/memory-tencentdb` 1.0.3 (55,757 TS lines in `src/`), with local Galion patches
  (L0 fallback in `/recall`, session-scoped persona filter, pyramid markdown export).
- Listens on 127.0.0.1:8420. RSS 614,664 kB (node) + ~11 MB esbuild service child.
- Env names (values not read): `TDAI_LLM_API_KEY`, `TDAI_LLM_BASE_URL`, `TDAI_LLM_MODEL`, `TDAI_GATEWAY_HOST`, `TDAI_GATEWAY_PORT`,
  `TDAI_GATEWAY_API_KEY` (set, so v1 routes need Bearer), `TDAI_DATA_DIR` (=/var/lib/memory-tencentdb), `TDAI_INSTANCE_ID` (=galion),
  `SCANNER_INSTANCES`, `MEMORY_TENCENTDB_GATEWAY_HOST/PORT`, `MEMORY_TENCENTDB_LLM_*`.
- `/health`: `stores.embeddingService:false`, so **no embeddings / vector recall are in use**; recall is FTS5 BM25 only.
- Storage: SQLite `instances/galion/vectors.db` (TDAI_INSTANCE_ID != default) with tables `l1_records`, `l0_conversations`,
  `l1_fts`, `l0_fts` (FTS5, v2 schema: segmented text + `*_original` raw column), `embedding_meta`; no vec0 tables (dims 0).
  Plus files: `persona.md`, `.metadata/scene_index.json`, `scene_blocks/`, `conversations/YYYY-MM-DD.jsonl`, `records/`, `offload/<session>/`.
- Hermes: `memory.provider: memory_tencentdb` in `/root/.hermes/config.yaml`; plugin is a symlink into the npm package
  (`hermes-plugin/memory/memory_tencentdb`, `client.py`).

## API contract (endpoints Hermes calls)

Traffic seen in the last 2 h of the journal: `GET /health` (most), `POST /recall` 68, `POST /v2/offload/query-mmd` 68,
`POST /capture` 54, `POST /session/end` 40, `POST /v2/offload/ingest` 22. The plugin also calls `/search/memories`,
`/search/conversations` (LLM tools) and `/v2/offload/read-ref` (read_node tool).

See the crate docs and tests for exact request/response shapes; a full table is added in the next commit.
