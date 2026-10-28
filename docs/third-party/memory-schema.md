<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/memory-schema

Written 2026-10-20T20:55:00+02:00 (Europe/Zagreb).

**No new dependencies.** `Cargo.toml` and `Cargo.lock` are unchanged from
swarm/memory-real (723b46c), so the transitive licence set is the same as
main 13a296c.

- Schema versioning uses SQLite's built-in `PRAGMA user_version` through the
  existing `rusqlite` 0.32 (`bundled`, MIT, https://github.com/rusqlite/rusqlite).
- The import-mode record id uses the crate's own portable SHA-256
  (`src/hash.rs`, our MIT code).
- Tests use `rusqlite`, `serde_json` (MIT OR Apache-2.0,
  https://github.com/serde-rs/json) and `std` only (hand-written HTTP client,
  LCG instead of `rand`). `scripts/real/memory-schema.sh` uses bash, git,
  tar, ss, stat, sha256sum and timeout.

## Gateway SQL used as a reference

`@tencentdb-agent-memory/memory-tencentdb` 1.0.3, `src/core/store/sqlite.ts`,
licence **MIT** (package.json `"license": "MIT"`), npm package
https://www.npmjs.com/package/@tencentdb-agent-memory/memory-tencentdb .

- `Store::rebuild_l1_fts` / `rebuild_l0_fts` and the v1-FTS drop in
  `Store::migrate` re-implement the behaviour of its `rebuildFtsIndex` and
  `migrateFtsTablesIfNeeded` in Rust. The column lists in the SQL strings are
  the schema itself (the same as the existing `SCHEMA_SQL`), not copied
  program logic.
- The unit test `gateway_init_sequence_tolerates_v4_database` contains a few
  of the gateway's SQL statements (its L0 upsert, delete, select and FTS
  statements) so that the test runs exactly what the gateway prepares. The
  source is noted in the test's doc comment; MIT allows this, and the
  statements are short schema-level SQL.
