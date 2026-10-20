<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind-memory: real-life tests for `swarm/memory-schema` (slot 36)

Written 2026-10-20T20:58:00+02:00 (Europe/Zagreb, CEST +02:00). Box ports:
19360-19369 only. Fixture databases only (synthetic text). Nothing was
deployed to, built on or written to the VPS.

## How to run

```
scripts/real/memory-schema.sh                     # box only
GALION_REAL_VPS=1 scripts/real/memory-schema.sh   # + ONE read-only GET /health of live 8420
```

| test | port | what it does |
|---|---|---|
| `real_kill9_during_migration_loop` | 19366 | Builds a pre-v3 **gateway-style** fixture (user_version 0, no `l0_conversations.timestamp`, no FTS tables; 60,000 l0 + 20,000 l1 rows with CJK/emoji/hangul/astral text, 14 MB). Measures a full migration, then **phase A**: 16 rounds, a fresh copy each time, SIGKILL at a spread of delays across the migration window; **phase B**: the SAME DB killed again and again, each time later, until a start completes. After every kill: `integrity_check` ok, row counts unchanged, and the DB is either fully old (user_version 0, no column, no FTS) or fully new (user_version 4, FTS rows = data rows); never in between. Then CJK FTS search over the old rows, byte check of a legacy row, `openmind-memory schema` report |
| `real_cjk_emoji_roundtrip_and_fts` | 19367 | 8 messages (Chinese, unspaced Chinese, katakana, hangul, ZWJ family, skin tone, flag, astral 𠮷, combining accent, Hebrew, keycap) via `/capture`; exact bytes in `l0_conversations`, `l0_fts.message_text_original` and the JSONL shard; 9 FTS queries (CJK 2-gram, kana, hangul, astral, Latin with diacritics) return the exact original; emoji-only query is a clean 200/0; still identical after a restart |
| `real_import_replay_idempotent` | 19369, 19368 | Default server: 40 old-timestamp messages → `l0_recorded 0` (unchanged). `ALLOW_IMPORT=1` server on a pre-v3 fixture: 40 inserted, then 3 replays → 40 duplicates each, then 8 **concurrent** replays of an overlapping 60-message batch → exactly +20; rows = FTS rows = JSONL lines = 60; original 2025 timestamps kept; live capture on the same server still uses the cursor |
| `real_rollback_to_previous_binary` | 19369 | New binary migrates a pre-v3 fixture to v4 and writes; the **previous binary (main 13a296c)** then serves it (health ok, fts true, reads the v4 row, writes); `user_version` stays 4; the new binary reads the old binary's row |
| `real_live_8420_health_get_only` | (VPS) | exactly one `curl -s http://127.0.0.1:8420/health` over ssh; only status, timing and keys are kept |
| memory-real suite (`tests/real_memory.rs`) | 19360-19365 | re-run unchanged except: ports via `OPENMIND_REAL_PORT_BASE=19360`, and two expectations updated: a pre-v3 DB's legacy rows are now found by FTS (total 2, was 1), and a v1-FTS DB now opens with `fts:true` (was false) |

## Final box run, 2026-10-20T20:47:42+02:00 → 20:49:00+02:00

Release binary 1,717,576 bytes (sha256 d32a5b01de83d385…). Box under swarm load.

```
memory-schema: start 2026-10-20T20:47:42+02:00
memory-schema: binary /workspace/swarm/target/memory-schema/release/openmind-memory (1717576 bytes, sha256 d32a5b01de83d385)
memory-schema: previous binary (main 13a296c) /workspace/swarm/target/memory-schema/prev-target/release/openmind-memory
running 5 tests
test real_cjk_emoji_roundtrip_and_fts ... FTS     萨格勒布 -> total 1 incl. "我们明天去萨格勒布吗？🚆"
FTS       格勒 -> total 1 incl. "我们明天去萨格勒布吗？🚆"
FTS      タワー -> total 1 incl. "東京タワーで会いましょう 🗼"
FTS       서울 -> total 1 incl. "서울특별시 날씨 좋네요 ☀\u{fe0f}"
FTS       날씨 -> total 1 incl. "서울특별시 날씨 좋네요 ☀\u{fe0f}"
FTS      𠮷野家 -> total 1 incl. "𠮷野家の牛丼 (astral U+20BB7)"
FTS   Ćevapi -> total 1 incl. "Ćevapi u Sarajevu, e\u{301} combining, שלום"
FTS       分词 -> total 1 incl. "纯中文没有空格的一句话用于测试分词效果"
FTS     rust -> total 1 incl. "keycap 1\u{fe0f}\u{20e3} crab 🦀 rust"
recall emoji-only -> context String("")
real_cjk_emoji_roundtrip_and_fts: ok (port 19367)
ok
test real_import_replay_idempotent ... default server (port 19369): 40 old-timestamp messages -> l0_recorded 0 (unchanged)
import (port 19368): 40 inserted, 3x40 duplicates, 8 concurrent x60 -> +20, rows 60/60 FTS, JSONL 60
real_import_replay_idempotent: ok
ok
test real_kill9_during_migration_loop ... fixture: pre-v3 gateway DB, 60000 l0 + 20000 l1 rows, 14020608 bytes, built in 278 ms
full migration (spawn -> healthy): 1171 ms
A round 00: kill -9 after  501 ms, wal  20760712 bytes -> old schema intact (user_version 0, rolled back)
A round 01: kill -9 after  622 ms, wal  25923072 bytes -> old schema intact (user_version 0, rolled back)
A round 02: kill -9 after   21 ms, wal         0 bytes -> old schema intact (user_version 0, rolled back)
A round 03: kill -9 after  416 ms, wal  17279312 bytes -> old schema intact (user_version 0, rolled back)
A round 04: kill -9 after  646 ms, wal  27084912 bytes -> old schema intact (user_version 0, rolled back)
A round 05: kill -9 after  489 ms, wal  19104472 bytes -> old schema intact (user_version 0, rolled back)
A round 06: kill -9 after 1291 ms, wal  39378992 bytes -> fully migrated (user_version 4)
A round 07: kill -9 after  986 ms, wal  39378992 bytes -> fully migrated (user_version 4)
A round 08: kill -9 after  391 ms, wal  16006232 bytes -> old schema intact (user_version 0, rolled back)
A round 09: kill -9 after 1357 ms, wal  39378992 bytes -> fully migrated (user_version 4)
A round 10: kill -9 after  735 ms, wal  30051312 bytes -> old schema intact (user_version 0, rolled back)
A round 11: kill -9 after  208 ms, wal   8120552 bytes -> old schema intact (user_version 0, rolled back)
A round 12: kill -9 after  245 ms, wal  10011632 bytes -> old schema intact (user_version 0, rolled back)
A round 13: kill -9 after  393 ms, wal  15141032 bytes -> old schema intact (user_version 0, rolled back)
A round 14: kill -9 after  735 ms, wal  31073072 bytes -> old schema intact (user_version 0, rolled back)
A round 15: kill -9 after 1086 ms, wal  39378992 bytes -> fully migrated (user_version 4)
phase A: 12 kills mid-migration (rolled back), 4 after commit
B attempt 0: kill -9 after 20 ms -> rolled_back=true
B attempt 1: kill -9 after 166 ms -> rolled_back=true
B attempt 2: kill -9 after 312 ms -> rolled_back=true
B attempt 3: kill -9 after 458 ms -> rolled_back=true
B attempt 4: kill -9 after 604 ms -> rolled_back=true
B attempt 5: kill -9 after 750 ms -> rolled_back=true
B attempt 6: kill -9 after 896 ms -> rolled_back=true
B attempt 7: kill -9 after 1042 ms -> rolled_back=false
schema report after phase B:
user_version=4
binary_schema_version=4
state=current
l0_conversations.timestamp=true
fts_v2=true
rows l0=60000 l0_fts=60000 l1=20000 l1_fts=20000
schema_migrations=absent
tables=embedding_meta,l0_conversations,l0_fts,l0_fts_config,l0_fts_content,l0_fts_data,l0_fts_docsize,l0_fts_idx,l1_fts,l1_fts_config,l1_fts_content,l1_fts_data,l1_fts_docsize,l1_fts_idx,l1_records
real_kill9_during_migration_loop: ok (port 19366)
ok
test real_live_8420_health_get_only ... skip: live probe needs GALION_REAL=1 GALION_REAL_VPS=1
ok
test real_rollback_to_previous_binary ... previous binary on v4 DB: health "ok" fts true, read + write ok
real_rollback_to_previous_binary: ok (port 19369, user_version stays 4)
ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 28.72s
running 7 tests
test real_auth_get_contract_matches_live ... real_auth_get_contract_matches_live: ok (port 19361)
ok
test real_crash_recovery_kill9_mid_write ... crash round 0: acked 1584 (total 1584), rows after kill -9 1585, integrity ok
crash round 1: acked 862 (total 2447), rows after kill -9 2450, integrity ok
crash round 2: acked 684 (total 3132), rows after kill -9 3135, integrity ok
real_crash_recovery_kill9_mid_write: ok (port 19363)
ok
test real_every_route_on_fixture_db ... 2026-10-20T20:48:21+02:00 [openmind-memory] schema migrated: MigrateReport { from: 0, to: 4, newer: false, fts_v1_dropped: false, l1_fts_rebuilt: None, l0_fts_rebuilt: None }
real_every_route_on_fixture_db: ok (port 19360)
ok
test real_live_8420_get_contract_readonly ... skip: live probe needs GALION_REAL=1 GALION_REAL_VPS=1
ok
test real_migrations_empty_idempotent_and_pre_v3 ... real_migrations_empty_idempotent_and_pre_v3: ok (port 19362)
ok
test real_perf_baseline_10k ... 2026-10-20T20:48:21+02:00 [openmind-memory] schema migrated: MigrateReport { from: 0, to: 4, newer: false, fts_v1_dropped: false, l1_fts_rebuilt: None, l0_fts_rebuilt: None }
PERF fixture: 10000 L1 rows seeded in 2.10s via library, 10000 L0 rows saved over HTTP; vectors.db 13172736 bytes (+WAL)
| op (ms, fresh TCP connection per request) | n | p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| save POST /capture (1 L0 row) | 10000 | 1.970 | 3.847 | 4.685 | 324.913 |
| search POST /search/memories (L1, 10k) | 1000 | 3.696 | 5.037 | 5.622 | 6.834 |
| search POST /search/conversations (L0, 10k) | 1000 | 3.617 | 4.797 | 5.547 | 14.587 |
| search /search/conversations + session_key | 1000 | 4.455 | 6.159 | 7.862 | 14.422 |
ok
test real_read_only_mode ... 2026-10-20T20:48:58+02:00 [openmind-memory] schema migrated: MigrateReport { from: 0, to: 4, newer: false, fts_v1_dropped: false, l1_fts_rebuilt: None, l0_fts_rebuilt: None }
real_read_only_mode: ok (port 19365)
ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 39.43s
memory-schema: schema report of a fresh DB:
user_version=4
binary_schema_version=4
state=current
l0_conversations.timestamp=true
fts_v2=true
rows l0=0 l0_fts=0 l1=0 l1_fts=0
schema_migrations=absent
tables=embedding_meta,l0_conversations,l0_fts,l0_fts_config,l0_fts_content,l0_fts_data,l0_fts_docsize,l0_fts_idx,l1_fts,l1_fts_config,l1_fts_content,l1_fts_data,l1_fts_docsize,l1_fts_idx,l1_records
memory-schema: listeners left on 19360-19369: 0
memory-schema: done 2026-10-20T20:49:00+02:00
```

## Live 8420 (read-only)

2026-10-20T20:44:26+02:00, one `GET /health` over ssh:

```
live GET /health -> 200 0.000254 (code, seconds); keys ["status", "version", "uptime", "stores", "services", "implementation", "implementation_version", "read_only", "fts"]
live status="ok" implementation="openmind-memory" implementation_version="0.1.0" read_only=false fts=true
```

So 8420 is served by the Rust openmind-memory 0.1.0 (writable, FTS on), with
the same 9 top-level keys as this branch's `/health` (unchanged on purpose).
Its DB is therefore at user_version 0 with the v3 schema; the
`live_like_db_with_schema_migrations_baseline` unit test models it (plus the
`schema_migrations` baseline row from galion-backup): the upgrade adds one
index and stamps user_version 4, no FTS rebuild, `schema_migrations` untouched.

Disclosure: before the GET-only scope was applied, one extra read-only probe
ran at about 20:45 +02:00: `systemctl status openmind-memory` (header only,
`-n 0`), `systemctl status memory-tencentdb-gateway` and `ss -ltn`. It showed
`/opt/galion/rust/bin/openmind-memory` active since 2026-10-18 22:14 +02:00,
no `memory-tencentdb-gateway` unit, and 8420/8431/8432/8433 listening on
loopback. A further status probe was blocked by the safety review and not
retried. Nothing was changed.

## Cross targets (cross.sh, zig cc + rust-lld)

| target | result | size |
|---|---|---|
| x86_64-unknown-linux-musl | build ok | 2,010,368 |
| aarch64-unknown-linux-musl | build ok | 1,830,752 |
| armv7-unknown-linux-musleabihf | build ok | 1,791,428 |
| aarch64-linux-android | check ok | - |

## Hermetic checks

`cargo fmt --check` ok; `cargo clippy --workspace --all-targets -- -D warnings`
ok; `cargo test --workspace` 521 passed, 0 failed (real tests skip without
`GALION_REAL=1`); `scripts/no-python.sh` ok. openmind-memory alone: 27 lib
unit tests (8 new, in store.rs), 2 new `import_mode` tests.
