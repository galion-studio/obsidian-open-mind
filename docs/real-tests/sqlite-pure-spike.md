<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Spike runs: sqlite-pure-spike (slot 28)

All times are Europe/Zagreb (CEST, +02:00). No ports were used and no
processes were left running. All files were scratch copies under `/tmp`.
No live DB was touched.

How to reproduce (each package has its own target directory):

```
cd spikes/sqlite-pure/fixtures && cargo build --release      # oracle (host only)
cd spikes/sqlite-pure && cargo build --release [--features fts]
fixtures make DIR && sqlite-pure-spike DIR && fixtures verify DIR
cd spikes/store-b && cargo build --release && store-b-spike DIR
# aarch64/armv7: CARGO_TARGET_<T>_LINKER=rust-lld, run with qemu-aarch64 / qemu-arm.
# The Turso spike also needs CARGO_TARGET_<T>_RUSTFLAGS="-L native=<dir with an empty libdl.a>".
```

Cross builds (2026-10-20 21:40 to 22:01 +02:00):

| target | turso default | turso `fts` | redb+tantivy |
|---|---|---|---|
| x86_64-unknown-linux-musl build | ok | ok | ok (3.8 MB) |
| aarch64-unknown-linux-musl build | `-ldl` link error; ok with an empty libdl.a stub (6.9 MB) | zstd-sys needs `aarch64-linux-musl-gcc` (C) | ok (2.9 MB) |
| armv7-unknown-linux-musleabihf build | same as aarch64 (7.2 MB) | zstd-sys C | ok (2.8 MB) |
| aarch64-linux-android check | ok | zstd-sys C | ok |

qemu runs (21:49:56 +02:00): the Turso spike gave the same 10 FAILs on aarch64
and armv7 as on x86_64, and the SQLite verify gave the same result.
store-b (22:01:40 +02:00): 4/4 PASS on aarch64 and armv7.

## Turso spike, x86_64, `--features fts` (verbatim)

```
== 2026-10-20T22:02:17+02:00 turso_core 0.8.3 spike, x86_64
turso_core 0.8.3, features: fts=true
== memory.db (openmind-memory store.rs + memory-schema 7952626)
PASS memory open read-write: opened
PASS PRAGMA cache_size = -2048: ()
PASS PRAGMA journal_mode = WAL: [["wal"]]
FAIL schema SQL (CREATE ... IF NOT EXISTS, incl. fts5): Parse error: index idx_l1_type already exists
PASS ALTER TABLE ADD COLUMN (duplicate, error expected and ignored by the code): "Parse error: duplicate column name: timestamp"
PASS ALTER TABLE ADD COLUMN (new column): ()
PASS pragma_table_info('l1_fts') (has_v2_fts): []
PASS SELECT COUNT(*) FROM l1_records: [["3"]]
PASS SELECT COUNT(*) FROM l0_conversations: [["3"]]
FAIL SELECT COUNT(*) FROM l1_fts: Parse error: no such table: l1_fts
FAIL SELECT COUNT(*) FROM l0_fts: Parse error: no such table: l0_fts
FAIL search_l1_fts: bm25(l1_fts) ... MATCH: Parse error: no such table: l1_fts
FAIL search_l0_fts: bm25(l0_fts) ... MATCH: Parse error: no such table: l0_fts
PASS l0_text_fallback: LIKE ? ESCAPE '\': [["sailing lesson moved to sunday morning"], ["zagreb harbour sailing plan for saturday"]]
PASS upsert_l0 BEGIN: ()
FAIL upsert_l0 INSERT ... ON CONFLICT(record_id) DO UPDATE: Parse error: ON CONFLICT clause does not match any PRIMARY KEY or UNIQUE constraint
FAIL upsert_l0 DELETE FROM l0_fts (fts5): Parse error: no such table: l0_fts
FAIL upsert_l0 INSERT INTO l0_fts (fts5): Parse error: no such table: l0_fts
PASS upsert_l0 COMMIT: ()
PASS insert_l1 INSERT OR REPLACE INTO l1_records: []
PASS PRAGMA user_version (read): [["0"]]
PASS PRAGMA user_version = 3 (write): ()
PASS sqlite_master lookup: [["1"]]
FAIL memory-schema FTS rebuild: DELETE FROM l1_fts (fts5): Parse error: no such table: l1_fts
FAIL tools snapshot alternative: VACUUM INTO: Parse error: WITHOUT ROWID tables are an experimental feature. Enable with --experimental-without-rowid flag
== Turso native FTS (feature fts, tantivy) as an FTS5 replacement
PASS CREATE INDEX ... USING fts (content): ()
PASS fts_match / fts_score: [["l1-0", "0.0"], ["l1-2", "0.0"]]
PASS memory close: ()
== memory-fresh.db (created by Turso: the memory schema without the two fts5 tables)
PASS fresh open: opened
PASS fresh schema SQL: ()
PASS fresh schema SQL again (IF NOT EXISTS): ()
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh INSERT OR REPLACE l1_records turso-b: []
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh CREATE INDEX ... USING fts (content): ()
PASS fresh insert after fts index: []
PASS fresh fts_match / fts_score: [["turso-c", "0.0"]]
PASS fresh counts: [["3", "1"]]
PASS fresh close: ()
== memory-nofts.db (created by SQLite: the memory schema without the two fts5 tables)
PASS fresh open: opened
PASS fresh schema SQL: ()
PASS fresh schema SQL again (IF NOT EXISTS): ()
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh INSERT OR REPLACE l1_records turso-b: []
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh counts: [["3", "2"]]
PASS fresh close: ()
== hermes.db (hermes-rs session_sqlite.rs, own DB)
PASS hermes open read-write: opened
PASS PRAGMA journal_mode = delete (hermes default): [["wal"]]
PASS schema SQL (AUTOINCREMENT, FOREIGN KEY): ()
PASS upsert sessions ON CONFLICT(id) DO UPDATE ... excluded: []
PASS append BEGIN: ()
PASS append INSERT INTO messages (AUTOINCREMENT id): []
PASS append UPDATE ... (SELECT COUNT(*) ...): []
PASS append COMMIT: ()
PASS list ORDER BY started_at DESC LIMIT: [["s-1", "api_server", "galion-one", "m2", "openrouter", "title", "", "1760800000.5", "", "1", "1", "2", "0.01"]]
PASS hermes close: ()
== legacy-state.db (Python Hermes state.db: FTS5 + triggers), read-only
PASS legacy open read-only: opened
PASS has_active: pragma_table_info('messages'): [["1"]]
PASS get session: [["s-py", "api_server", "galion-one", "anthropic/claude-haiku-5.5", "openrouter", "", "", "1760800000.5", "", "0", "0", "0", ""]]
PASS messages ... AND active = 1 ORDER BY id: [["user", "hi from python", "", "", ""], ["assistant", "", "[{\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"clock\",\"arguments\":\"{}\"}}]", "",
PASS read-only open refuses writes: "attempt to write a readonly database"
== 10 check(s) failed
exit (failures) 10
== SQLite 3.46 verify
error: malformed database schema (__turso_internal_fts_dir_l1_records_fts_key) - near "USING": syntax error
```

## store-b spike, qemu-aarch64 (verbatim)

```
== 2026-10-20T22:03:43+02:00
PASS redb upsert + read back (3 rows, r1 updated)
PASS redb reopen after close
PASS tantivy index + delete/re-add + commit
  hit r1 bm25=1.869
PASS tantivy BM25 search after reopen (1 hit, r1, score > 0, 3 docs)
failures: 0
```

## Turso spike, default features, qemu-aarch64 (run 21:49:56 +02:00), then the SQLite 3.46 verify (verbatim)

```
turso_core 0.8.3, features: fts=false
== memory.db (openmind-memory store.rs + memory-schema 7952626)
PASS memory open read-write: opened
PASS PRAGMA cache_size = -2048: ()
PASS PRAGMA journal_mode = WAL: [["wal"]]
FAIL schema SQL (CREATE ... IF NOT EXISTS, incl. fts5): Parse error: index idx_l1_type already exists
PASS ALTER TABLE ADD COLUMN (duplicate, error expected and ignored by the code): "Parse error: duplicate column name: timestamp"
PASS ALTER TABLE ADD COLUMN (new column): ()
PASS pragma_table_info('l1_fts') (has_v2_fts): []
PASS SELECT COUNT(*) FROM l1_records: [["3"]]
PASS SELECT COUNT(*) FROM l0_conversations: [["3"]]
FAIL SELECT COUNT(*) FROM l1_fts: Parse error: no such table: l1_fts
FAIL SELECT COUNT(*) FROM l0_fts: Parse error: no such table: l0_fts
FAIL search_l1_fts: bm25(l1_fts) ... MATCH: Parse error: no such table: l1_fts
FAIL search_l0_fts: bm25(l0_fts) ... MATCH: Parse error: no such table: l0_fts
PASS l0_text_fallback: LIKE ? ESCAPE '\': [["sailing lesson moved to sunday morning"], ["zagreb harbour sailing plan for saturday"]]
PASS upsert_l0 BEGIN: ()
FAIL upsert_l0 INSERT ... ON CONFLICT(record_id) DO UPDATE: Parse error: ON CONFLICT clause does not match any PRIMARY KEY or UNIQUE constraint
FAIL upsert_l0 DELETE FROM l0_fts (fts5): Parse error: no such table: l0_fts
FAIL upsert_l0 INSERT INTO l0_fts (fts5): Parse error: no such table: l0_fts
PASS upsert_l0 COMMIT: ()
PASS insert_l1 INSERT OR REPLACE INTO l1_records: []
PASS PRAGMA user_version (read): [["0"]]
PASS PRAGMA user_version = 3 (write): ()
PASS sqlite_master lookup: [["1"]]
FAIL memory-schema FTS rebuild: DELETE FROM l1_fts (fts5): Parse error: no such table: l1_fts
FAIL tools snapshot alternative: VACUUM INTO: Parse error: WITHOUT ROWID tables are an experimental feature. Enable with --experimental-without-rowid flag
PASS memory close: ()
== memory-fresh.db (created by Turso: the memory schema without the two fts5 tables)
PASS fresh open: opened
PASS fresh schema SQL: ()
PASS fresh schema SQL again (IF NOT EXISTS): ()
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh INSERT OR REPLACE l1_records turso-b: []
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh counts: [["2", "1"]]
PASS fresh close: ()
== memory-nofts.db (created by SQLite: the memory schema without the two fts5 tables)
PASS fresh open: opened
PASS fresh schema SQL: ()
PASS fresh schema SQL again (IF NOT EXISTS): ()
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh INSERT OR REPLACE l1_records turso-b: []
PASS fresh INSERT OR REPLACE l1_records turso-a: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh upsert_l0 ON CONFLICT turso-1: []
PASS fresh counts: [["3", "2"]]
PASS fresh close: ()
== hermes.db (hermes-rs session_sqlite.rs, own DB)
PASS hermes open read-write: opened
PASS PRAGMA journal_mode = delete (hermes default): [["wal"]]
PASS schema SQL (AUTOINCREMENT, FOREIGN KEY): ()
PASS upsert sessions ON CONFLICT(id) DO UPDATE ... excluded: []
PASS append BEGIN: ()
PASS append INSERT INTO messages (AUTOINCREMENT id): []
PASS append UPDATE ... (SELECT COUNT(*) ...): []
PASS append COMMIT: ()
PASS list ORDER BY started_at DESC LIMIT: [["s-1", "api_server", "galion-one", "m2", "openrouter", "title", "", "1760800000.5", "", "1", "1", "2", "0.01"]]
PASS hermes close: ()
== legacy-state.db (Python Hermes state.db: FTS5 + triggers), read-only
PASS legacy open read-only: opened
PASS has_active: pragma_table_info('messages'): [["1"]]
PASS get session: [["s-py", "api_server", "galion-one", "anthropic/claude-haiku-5.5", "openrouter", "", "", "1760800000.5", "", "0", "0", "0", ""]]
PASS messages ... AND active = 1 ORDER BY id: [["user", "hi from python", "", "", ""], ["assistant", "", "[{\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"clock\",\"arguments\":\"{}\"}}]", "",
PASS read-only open refuses writes: "attempt to write a readonly database"
== 10 check(s) failed
== 2026-10-20T22:03:51+02:00 SQLite verify of the same files
verify memory.db: integrity_check=wrong # of entries in index idx_l1_sessionkey_updated
verify memory-fresh.db: integrity_check=ok
verify memory-nofts.db: integrity_check=ok
verify hermes.db: integrity_check=ok
verify legacy-state.db: integrity_check=ok
verify memory.db: rows written by turso=0, user_version=3, FTS5 MATCH 'turso' -> Ok(0)
verify hermes.db: s-1 message_count=1, messages=1
```
