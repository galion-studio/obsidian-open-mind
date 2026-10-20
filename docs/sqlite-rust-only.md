<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Replacing bundled C SQLite (rusqlite `bundled`) with Rust only

Branch `swarm/sqlite-pure-spike`, based on main 13a296c. Spikes are in
`spikes/` and are standalone packages, not workspace members. Written on
2026-10-20 at 22:05 +02:00 (Europe/Zagreb). Verbatim runs are in
`docs/real-tests/sqlite-pure-spike.md` and licences in
`docs/third-party/sqlite-pure-spike.md`.

## Recommendation

1. **Do not use Turso (`turso_core`) on the live databases.** On a copy of
   an openmind-memory DB containing FTS5:
   - it cannot see FTS5 tables, so every search fails;
   - it refused the l0 upsert;
   - it **corrupted an index** that real SQLite then reported
     (`wrong # of entries in index idx_l1_sessionkey_updated`).

   Its own FTS makes the file unreadable for SQLite, and it needs tantivy's
   zstd-sys, which is C.
2. **openmind-memory: move to redb (rows) + tantivy (BM25)** with
   `default-features = false, features = ["lz4-compression", "mmap"]`.
   - That pair has **no C**, needs no `-ldl` and no linker workaround.
   - It builds for x86_64/aarch64/armv7 musl, `cargo check` passes on
     aarch64-android, and the spike passes 4/4 under qemu aarch64 and armv7.
   - Binary: 2.9 MB aarch64 (Turso spike: 6.9 MB).
   - It needs a one-off data migration and changes search ranking (see
     decisions).
3. **hermes-rs, desk-hermes and one-defence only read SQLite they do not
   own**: hermes reads the legacy Python `state.db`; desk-hermes and
   one-defence open DBs `OpenFlags` read-only.
   - hermes-rs already has a pure-Rust JSONL session store
     (`--no-default-features`); make it the default.
   - Read the legacy files with the existing pure-Rust read-only reader
     `crates/galion-one/src/sqlite_ro.rs` (rowid tables + WAL frames).
     Move it into a shared crate. It has no SQL, so the few queries become
     Rust filters.
4. **(c) cross-compiling the C** (`zig cc`, already used by
   `crates/openmind-memory/cross.sh`) works today but breaks "Rust only".
   Keep it only as the interim path until 2 and 3 land.

## What the code uses (main 13a296c plus swarm/memory-schema 7952626)

**openmind-memory** (`store.rs`, `tools.rs`, 471 lines):
- Full-text search:
  - FTS5 virtual tables `l1_fts` and `l0_fts` with `UNINDEXED` columns;
  - `MATCH` with `bm25()` ordering;
  - a `LIKE ? ESCAPE '\'` fallback.
- Writes:
  - `INSERT ... ON CONFLICT(record_id) DO UPDATE SET ... excluded.*`;
  - `INSERT OR REPLACE`;
  - transactions.
- Schema and pragmas:
  - `PRAGMA journal_mode=WAL`, `cache_size=-2048`, `busy_timeout`;
  - `ALTER TABLE ADD COLUMN`, `pragma_table_info()`, `sqlite_master`;
  - the backup API (`rusqlite::backup`) for snapshots.
- 7952626 adds:
  - `PRAGMA user_version` read and write;
  - `DROP TABLE` of the FTS tables and an FTS rebuild (DELETE + reinsert);
  - `schema_migrations` and `idx_l0_session_ts`.
- No triggers and no JSON SQL functions.

**hermes-rs** (`session_sqlite.rs`, 350 lines, feature `sqlite`, on by default):
- its own DB: `journal_mode=delete`, `AUTOINCREMENT`, `FOREIGN KEY`, the `ON CONFLICT(id) DO UPDATE` upsert, `UPDATE` with a `COUNT` subquery, `pragma_table_info('messages')`, `ORDER BY ... LIMIT`;
- a read-only open of the Python `state.db`, which has FTS5 and triggers.

**Others with bundled SQLite**: desk-hermes and one-defence (read-only).

## Evidence

### (a) Turso `turso_core` 0.8.3

Facts:
- MIT. Latest version is 0.9.0-pre.1 (2026-10-15), so it is pre-1.0.
- COMPAT.md says the file format is supported, with these gaps:
  - FTS3/4/5 are not supported. The replacement is tantivy-based
    `CREATE INDEX ... USING fts`, marked experimental.
  - WAL only: "rollback journal not needed".
  - No backup API.
  - No mixed SQLite + Turso multi-process access.
  - Invalid UTF-8 is replaced with U+FFFD.

The spike `spikes/sqlite-pure`:
- Fixtures were made by real SQLite 3.46 (`fixtures`, rusqlite, host-only oracle).
- Turso then ran the real statements, and SQLite verified the files afterwards.
- Results are identical on x86_64, and under qemu on aarch64 and armv7 (51 PASS / 10 FAIL).

| case | result |
|---|---|
| memory.db, SQLite-made, **with FTS5** | 10 FAIL: `no such table: l1_fts/l0_fts` (search, insert, delete, rebuild); the `IF NOT EXISTS` schema re-run fails (`index idx_l1_type already exists`); the l0 upsert is refused (`ON CONFLICT clause does not match`); `VACUUM INTO` fails (FTS5 shadow tables are WITHOUT ROWID). `INSERT OR REPLACE` "passes" but **SQLite then reports `integrity_check = wrong # of entries in index idx_l1_sessionkey_updated` and sees 0 of the rows written**. |
| memory schema without FTS5 (made by SQLite, or made by Turso) | all PASS, SQLite `integrity_check=ok` |
| hermes.db (own DB) | all PASS, SQLite ok. `PRAGMA journal_mode=delete` silently answers `wal`. |
| legacy state.db (FTS5 + triggers), read-only | reads PASS |
| Turso FTS (`--features fts`) | `fts_match` finds rows but `fts_score` is always 0.0. Afterwards SQLite cannot open the file at all (`malformed database schema (__turso_internal_fts_dir_...) near "USING"`). |

Portability:
- Default features: x86_64-musl builds; android `check` passes.
- aarch64 and armv7 musl fail to link with `unable to find library -ldl`
  (`libloading`, which Turso uses for extensions). They link with an empty
  `libdl.a` stub (`-L native=`), which is what musl itself ships.
- `--features fts` pulls tantivy with default features, which includes
  **zstd-sys (C)**. It fails on aarch64, armv7 and android without a C
  cross compiler, and a downstream crate cannot turn that feature off.
- SIMD code in deps (all runtime-detected, none needs target-cpu flags):
  - simdutf8;
  - crc32c (SSE4.2/ARM CRC);
  - aes via cpufeatures;
  - aegis is forced to `pure-rust` (feature `pure-rust-crypto`); `cc`
    remains only as an unused build-dependency.

### (b) redb 2.6.4 + tantivy 0.26.2 (`spikes/store-b`)

- Licences: redb is MIT OR Apache-2.0; tantivy is MIT.
- With `default-features = false` (lz4 via lz4_flex) the tree has no `-sys`
  C crate (only linux-raw-sys, which is Rust).
- What the spike checks:
  - an upsert by key;
  - reopen;
  - a tantivy index with delete-by-term and re-add (the FTS rebuild);
  - a BM25 search after reopen.

  Result: 4/4 PASS on x86_64, and on aarch64 and armv7 under qemu
  (`bm25=1.869`).
- SIMD (no `asm!`, nothing needs target-cpu flags):
  - bitpacking uses SSE3 on x86 with runtime detection and scalar code
    elsewhere;
  - tantivy-bitpacker uses AVX2 with runtime detection;
  - redb's xxh3 uses AVX2 with runtime detection on x86 and NEON (baseline)
    on aarch64.
- Other stores: fjall (MIT/Apache, LSM, 2.x) is fine but has more moving
  parts than needed. sled is still 0.34 beta from 2021, so it is not
  recommended. native_db is a typed layer over redb and is not needed.

## Effort

| work | estimate |
|---|---|
| openmind-memory store on redb+tantivy, behind the same `Store` API, with golden recall tests | 2 to 3 days |
| one-off migrator SQLite → redb/tantivy, reading with `sqlite_ro`, dry-run + count/hash compare | 1 day |
| hermes-rs: JSONL default plus a legacy `state.db` reader on `sqlite_ro` | 1 day |
| desk-hermes and one-defence read-only on `sqlite_ro` | 0.5 to 1 day each |
| shared `sqlite_ro` crate out of galion-one, plus index-free query helpers | 0.5 day |

The total is about 6 to 7 agent-days. Each step can ship on its own.

## Risks

- **Search ranking changes.** Tantivy BM25 and its tokenizer are not
  FTS5's, so recall order and hits will differ. Golden comparisons on a
  copy of the live DB are needed before switching.
- **Migration** of the live memory DB means downtime and a backup. The old
  file stays as the rollback, and a reverse export must be planned if a
  way back to SQLite is wanted.
- `sqlite_ro` has no index support, which means a full scan. That is fine
  for the current sizes; measure against the live DB copy.
- The upstream crates are 0.x/2.x; pin exact versions.

## Decisions for Slavik

1. Accept a search-ranking change in openmind-memory (FTS5 → tantivy BM25): yes/no.
2. Store choice: redb + tantivy (recommended), or wait for Turso to support FTS5: yes/no.
3. hermes-rs: JSONL as the default store, with legacy `state.db` read-only via `sqlite_ro`: yes/no.
4. Keep `zig cc` cross builds of the C SQLite as a temporary exception until the migration lands: yes/no.
5. Allow deps with runtime-detected SIMD intrinsics (tantivy, redb): yes/no. Our rule bans our own SIMD and target-cpu flags; these deps have scalar fallbacks.
