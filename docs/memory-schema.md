<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind-memory: schema versioning and import mode (`swarm/memory-schema`)

Written 2026-10-20T20:55:00+02:00 (Europe/Zagreb, CEST +02:00). Branch
`swarm/memory-schema`, built on `swarm/memory-real` (723b46c), which is based
on main 13a296c.

## 1. Schema version: `PRAGMA user_version`

| `user_version` | meaning |
|---|---|
| 0 | never stamped. Every DB the Node gateway created, and every DB written by openmind-memory ≤ 723b46c (the live VPS DB too, according to `docs/FOUNDATION-REVIEW.md` on `ws/foundation`) |
| 3 | gateway baseline: `l0_conversations.timestamp` + v2 FTS (`*_original` columns). Not written on its own; the first migration goes straight to 4 |
| 4 | baseline + `idx_l0_session_ts ON l0_conversations(session_key, timestamp)` (lookup for the import-mode dedupe) |

`Store::migrate` (crates/openmind-memory/src/store.rs):

- runs in **one `BEGIN IMMEDIATE` transaction**. SQLite DDL and `PRAGMA user_version`
  are both transactional, so a kill -9 at any point leaves the old schema
  **and** the old version (proved in a loop below);
- runs the idempotent **baseline on every writable open**, even when
  `user_version` already says 4. The gateway shares the file, knows nothing
  about the version and may change the schema after us, so the version is a
  record, not the only gate;
- baseline = the gateway's `initSchema` without vec0: v1 FTS (no `*_original`
  columns) → drop both FTS tables, `CREATE … IF NOT EXISTS`, add the pre-v3
  `timestamp` column after a column check (no ALTER-and-ignore), then
  **repopulate** an FTS table that was missing or dropped while its data table
  has rows (port of the gateway's `rebuildFtsIndex`). Before this branch a
  pre-v3 DB opened with an empty `l0_fts` (old rows not searchable) and a v1
  FTS DB opened with `fts:false`;
- numbered steps above the baseline are **additive only** (new indexes), so
  the gateway and older binaries keep working on a migrated DB;
- `user_version` newer than the binary → **no DDL at all**, a warning in the log,
  normal service on the columns it knows;
- no `schema_migrations` table is created (the GH design keeps none; the
  memory-real test asserts it). One that already exists (the VPS DB has one
  from `galion-backup migrate-baseline`, row v3) is left alone and only reported.

`openmind-memory schema <db>` prints a read-only report (user_version, binary
version, state, timestamp column, FTS v2, row counts, `schema_migrations` rows,
tables). It never creates or migrates the file.

`/health` is unchanged on purpose: its key set is compared with live 8420.

### Why the Node gateway tolerates it (verified from its SQL)

Source read: `@tencentdb-agent-memory/memory-tencentdb` **1.0.3** (MIT),
`src/core/store/sqlite.ts` (2,564 lines; the copy on the box at
`/workspace/oom-study/memory`, same package version as `docs/ws-memory.md`
records for the VPS install). It is not in the GitHub repo or the ws/vps
branches: `git grep` over every `origin/*` ref finds `l0_conversations` only
in docs/fixtures, and `user_version` only in `ws/foundation`'s
`galion-backup` (read-only `schema` report) and `docs/FOUNDATION-REVIEW.md`.

| gateway code (sqlite.ts) | effect on a DB at user_version 4 |
|---|---|
| no `user_version` / `application_id` / `schema_version` anywhere in the package copy (`grep -rn` over src/, index.ts, hermes-plugin/) | the header field is invisible to it; it never resets it |
| PRAGMAs set: `busy_timeout`, `journal_mode=WAL`, `cache_size`, `mmap_size`, `wal_autocheckpoint` (l. 436-448) | none touches user_version |
| `CREATE TABLE/INDEX/VIRTUAL TABLE IF NOT EXISTS` for embedding_meta, l1_records, l0_conversations, l1_fts, l0_fts and 12 indexes (l. 552-895) | all already exist with the same definitions → no-ops |
| `try { ALTER TABLE l0_conversations ADD COLUMN timestamp … } catch {}` (l. 721-727) | throws "duplicate column", caught, expected |
| `migrateFtsTablesIfNeeded` checks `sqlite_master` for `l1_fts` and `pragma_table_info('l1_fts')` for `content_original` (l. 2383-2418) | v2 present → no drop |
| `dropVectorTables` drops only `l1_vec`/`l0_vec` (l. 1047) | we never create them |
| `sqlite_master` lookups only by name (`l1_fts`, vec table DDL) | an extra index (`idx_l0_session_ts`) is ignored |
| `INSERT … ON CONFLICT(record_id) DO UPDATE` into l0_conversations, `DELETE … WHERE record_id`, FTS inserts/searches | work unchanged with the extra index |

This is replayed as a unit test, `gateway_init_sequence_tolerates_v4_database`:
the gateway's CREATE set, its blind ALTER (must fail with "duplicate column"),
the FTS v2 check, and 8 of its prepared statements run against a v4 DB; then
`user_version` is still 4 and this binary reopens it as current. The real
Node gateway could not be run on the box (Node 20 here; the gateway needs
`node:sqlite` from Node ≥ 22.5), so this is SQL-level evidence, not a run of
the gateway itself.

Older openmind-memory binaries: `real_rollback_to_previous_binary` starts the
main 13a296c binary (the design live on 8420 as 0.1.0) on a v4 DB: health ok,
`fts:true`, it reads the v4 rows, writes a new one and leaves `user_version`
at 4; this binary then reads that row.

## 2. Import mode (old timestamps on replay)

Default, unchanged: `/capture` keeps only messages newer than the session
cursor, which starts at the request time (gateway parity). A replay of an old
session is therefore dropped (`l0_recorded: 0`).

Opt-in: the server needs `OPENMIND_MEMORY_ALLOW_IMPORT=1` **and** the request
body `"import": true`. Without the env var the flag is ignored like any
unknown field. In import mode:

- no cursor filter, and the session cursor is neither read nor advanced, so
  live capture is unaffected;
- every message needs a numeric `timestamp` (otherwise it is skipped and
  counted: no stable key, no idempotence);
- dedupe key = (session_key, timestamp, sanitized message_text). The record id
  is `l0_imp_` + the first 32 hex chars of SHA-256 over the NUL-joined key, and
  a row counts as a duplicate when that id OR any row with the same
  (session_key, timestamp, message_text) exists, so messages that live capture
  already stored with a random id are not added again;
- the batch goes in ONE `BEGIN IMMEDIATE` transaction (rows + FTS); only rows
  actually inserted are appended to the JSONL shard;
- response: `{"l0_recorded": n, "scheduler_notified": false, "import":
  {"inserted": n, "duplicates": d, "skipped_no_timestamp": s}}`; 503 without a
  database, 500 (nothing stored, safe to retry) on a SQLite error;
- read-only mode stores nothing, as before.

Example (box only, fixture data):

```
OPENMIND_MEMORY_ALLOW_IMPORT=1 OPENMIND_MEMORY_PORT=19368 openmind-memory &
curl -s 127.0.0.1:19368/capture -d '{"session_key":"s","user_content":"x","assistant_content":"y","import":true,
  "messages":[{"role":"user","content":"stara poruka","timestamp":1759312800000}]}'
```

## 3. Known limits

- Emoji-only queries have no word characters, so `build_fts_query` returns
  nothing (search answers 200 with 0 results; the gateway's fallback tokenizer
  does the same). Emoji inside stored text round-trips byte for byte.
- The FTS rebuild of a large legacy DB runs inside the migration transaction:
  60k l0 + 20k l1 rows took about 1.2-1.5 s on the box under swarm load, with a
  39 MB WAL until the next checkpoint.
- Import dedupe is exact: the same message with a different timestamp, or
  different whitespace after sanitizing, is a different message.
