<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind-memory: real-life tests (swarm slot 09, `swarm/memory-real`)

Written 2026-10-20T19:24:00+02:00 (Europe/Zagreb, CEST +02:00). Base: GitHub
main 13a296c. Box ports: 19090-19099 only. Live VPS: only GET requests to
127.0.0.1:8420 over ssh, plus `ss -ltn` and `systemctl is-active`. No POST was
sent to 8420. No stored contents were read or recorded: only status codes,
latency and key/type shapes.

## How to run

```
scripts/real/memory-real.sh                     # box only
GALION_REAL_VPS=1 scripts/real/memory-real.sh   # + read-only GET probe of live 8420
```

The script builds the release binary, runs `crates/openmind-memory/tests/real_memory.rs`
with `GALION_REAL=1 --test-threads=1 --nocapture`, and then kills anything of
ours still on 19090-19099. Without `GALION_REAL=1`, each test prints `skip` and
passes, so `cargo test --workspace` stays hermetic.

| test | port | what it does |
|---|---|---|
| `real_every_route_on_fixture_db` | 19090 | Fresh fixture DB (21 L1 rows seeded through the library). Covers `/health` shape (= live), OPTIONS 204, 404, `/seed` 501, bad JSON 400, 413 (cap 4096), v2 needs Bearer + service id (401), `/capture` save + cursor dedupe + 400, `/search/conversations` (+ session filter), `/search/memories` (key order, type/scene filters, empty result), `/recall` keys + 400, `/session/end` 200/400. Timestamps: stored `timestamp` ms are exact, `recorded_at`/`recordedAt` use the gateway's JS `toISOString()` format, JSONL shard lines, l0 rows = l0_fts rows |
| `real_auth_get_contract_matches_live` | 19091 | Starts with a fixture Bearer value and expects the same GET status and body keys as live 8420 for 10 paths |
| `real_migrations_empty_idempotent_and_pre_v3` | 19092 | (a) Up from an empty dir: v3 tables and indexes, WAL, no vec0, no `schema_migrations`. (b) 3 more restarts: identical `sqlite_master` and row counts. (c) Pre-v3 DB with no `l0_conversations.timestamp`: column added, old row kept, writes and search work. (d) Legacy FTS without `*_original` columns: opens, `fts:false`, writes still land |
| `real_read_only_mode` | 19095 | `READ_ONLY=1`: a missing DB gives `degraded` and no file is created. On an existing DB, search works, capture writes nothing, the DB stays byte-identical and no JSONL dir appears |
| `real_crash_recovery_kill9_mid_write` | 19093 | 3 rounds: 4 writer threads POST `/capture`, SIGKILL after 700 ms, then `PRAGMA integrity_check`, FTS5 `integrity-check`, `count(l0_conversations) == count(l0_fts)`, rows ≥ acknowledged, restart, search, write |
| `real_perf_baseline_10k` | 19094 | 10,000 L1 rows (library seed) + 10,000 saves over HTTP, then 1,000 queries per search kind |
| `real_live_8420_get_contract_readonly` | (VPS) | ssh GETs with `curl -o /dev/null -w code,time`. The health body is parsed in memory and only key/type pairs are kept |

## Final run, 2026-10-20T19:21:22+02:00 → 19:22:25+02:00

Binary: `target/memory-real/release/openmind-memory`, 1,689,352 bytes, `openmind-memory 0.1.0`. Box load average 3.4 on 8 CPUs (about 21 swarm agents).
`test result: ok. 7 passed; 0 failed` in 46.68 s. Listeners left on 19090-19099: 0.

### Live 8420 contract (read-only GET)

| GET path | live 8420 | box binary (with a Bearer configured) |
|---|---|---|
| /health | 200 | 200 |
| / , /recall , /search/memories , /search/conversations , /session/end , /capture , /no-such-route , /health/ | 401 `{error}` | 401 `{error}` |
| /v2/offload/query-mmd | 401 `{code,message,request_id}` | same |

`/health` shape (keys and types only), live and box are identical, 12 keys:
`status:string, version:string, uptime:number, stores.vectorStore:boolean,
stores.embeddingService:boolean, services.timerScanner:null,
services.pipelineWorker:null, services.stateBackend:string,
implementation:string, implementation_version:string, read_only:boolean, fts:boolean`.
Live latency, curl on the VPS loopback: single GETs 0.28-0.78 ms; `/health` 20× p50 0.400 ms, p95 1.096 ms.
Earlier probe at 19:05:32+02:00: same codes, `/health` 10× 0.21-0.42 ms; `ss -ltn` showed one :8420 listener and `openmind-memory` was `active`.

### Crash recovery (kill -9 mid-write)

```
crash round 0: acked 1400 (total 1400), rows after kill -9 1400, integrity ok
crash round 1: acked 820 (total 2221), rows after kill -9 2221, integrity ok
crash round 2: acked 702 (total 2924), rows after kill -9 2925, integrity ok
```
No acknowledged write was lost. Rows sometimes exceed acks by 1: the write
committed, but the process died before it sent the 200. No torn FTS/row pairs.

### Performance baseline (10k fixture records)

Each request opens a fresh TCP connection (`Connection: close`, the server's
model). The queries are 2 words from a 24-word vocabulary, so each one matches
thousands of rows. That is a broad, worst-case-ish BM25 load.

| op (ms) | n | p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| save `POST /capture` (1 L0 row: SQLite tx + FTS + JSONL append) | 10000 | 2.040 | 3.901 | 4.740 | 163.578 |
| search `POST /search/memories` (L1, 10k rows) | 1000 | 3.740 | 5.476 | 6.601 | 9.155 |
| search `POST /search/conversations` (L0, 10k rows) | 1000 | 3.660 | 5.403 | 7.770 | 13.730 |
| search `/search/conversations` + `session_key` | 1000 | 4.607 | 6.758 | 9.978 | 22.370 |

Same harness on the unmodified base binary (13a296c), 19:19 CEST: save p50 1.914 / p95 3.847;
memories 3.936 / 5.611; conversations 3.846 / 5.523; + session_key 3.827 / 5.290.
The save max (160-370 ms) is a WAL checkpoint stall on a loaded box. It shows up on both binaries.

## Findings

1. **BUG (fixed): a pre-v3 `vectors.db` could not be opened.** In `SCHEMA_SQL`,
   `CREATE INDEX idx_l0_timestamp ON l0_conversations(timestamp)` ran *before*
   the `ALTER TABLE … ADD COLUMN timestamp`, so `Store::open` failed with "no
   such column: timestamp". Reproduced on the base binary (19:13 CEST): `/health`
   gave `status:degraded, vectorStore:false, fts:false`, while `/capture` still
   answered `l0_recorded: 2`. Those rows went only to the JSONL shard, never to
   SQLite. Fix: `Store::migrate` runs everything in one transaction (SQLite DDL is
   transactional, so a crash mid-migration rolls back and the next start retries),
   checks the column with `pragma_table_info` instead of ignoring the ALTER error,
   then creates the index (`SCHEMA_V3_SQL`). Live 8420 reports `fts:true`, so its
   DB is already v3 and is not affected today. A restore from an old backup would
   have been.
2. **There is no `schema_migrations` table** in either design (GH `store.rs` or the
   VPS `index.rs`), and neither is there `PRAGMA user_version`. The "v3" schema
   is implicit: `CREATE … IF NOT EXISTS` + the one column check. The test
   asserts that no `schema_migrations` table is created. Adding one would be a
   schema change to a file that the Node gateway also reads. That is an owner
   decision and was not done.
3. **The `/capture` cursor drops "old" client timestamps.** With no cursor for a
   session (fresh process), the cursor is `request_ms - 1`, so messages whose
   `timestamp` is ≤ now are silently skipped (`l0_recorded: 0`). This matches the
   gateway port. Hermes sends no timestamp (the server uses now), so it is fine
   in production. Importers that replay history, though, get nothing. Cursors
   are in memory only: after a restart, a replay of the same messages *with
   future timestamps* would be stored twice. The tests use timestamps one day
   ahead.
4. **Ported from the VPS rewrite (`vps-main-20261020`, `index.rs::search`):
   filters run before the limit.** Before, `/search/memories` took `limit*3`
   BM25 hits and *then* applied `type`/`scene`, and `/search/conversations` took
   `limit*4` and then filtered `session_key`. Both could return 0 or a short page
   while matches existed (a unit test and the real test reproduce it: 20 strong
   episodic hits hide 1 instruction hit). Now the type/scene filter streams up to
   5,000 ranked rows, and `session_key` is a SQL `AND session_key = ?`.
   Unfiltered queries are unchanged. Cost: +0.8 ms p50 for the session-filtered
   search at 10k rows, for correct full pages.
5. **Tried and dropped:** a `record_id` tie-break (`ORDER BY rank, record_id`)
   for stable tie order. It doubled the search p50 (7.0-7.5 ms against 3.7-3.9 ms,
   A/B at 19:16-19:18 CEST), so it was removed.
6. **MEM-SEARCH (c241753) is docs only.** It adds no code. Its contract (the
   `/search/memories` keys `results,total,strategy` in that order, and
   `/search/conversations` `results,total`) already holds in the GH design, and
   the real test now asserts it.
7. Stored timestamps keep the gateway's formats (JS `toISOString()` UTC and
   epoch ms) for file compatibility with the Node gateway. Logs use Zagreb time.
   The VPS rewrite stores Zagreb RFC 3339 instead. That was not ported, because
   it would change the shared DB format.
8. Not ported from the VPS rewrite: its `capture`, which also writes an L1 note.
   That would change `/search/memories` results compared with the gateway. Its 2048-byte
   query cap was not ported either, because it would change the 200 → 400
   contract.

## Process notes

- At 19:05 CEST the first live probe wrote each GET body to `/tmp/.mr09` on the VPS
  and deleted it in the same command. Bodies were not printed. Later probes use
  `-o /dev/null`. Nothing was written under /opt/galion/openmind.
- Cross builds via `crates/openmind-memory/cross.sh` at nice 10 / -j 4 (19:21 CEST):
  x86_64 / aarch64 / armv7 musl build, aarch64-linux-android `cargo check`. All pass.
