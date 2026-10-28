<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-test evidence: backup-drop-legacy-root (wave-4 slot 427)

Branch `swarm/backup-drop-legacy-root` off `legacy-vectors-archive-plan`
7aba14ec. D-406-3 **staged only**: galion-backup code + tests + runbook
step 6 (docs/ops/legacy-vectors-archive.md). No VPS write, no unit/timer
change, no deploy. Box + VPS read-only, 2026-10-28 21:42-21:53 CET (+01:00).
Hermes research not used. No new dependencies.

## Change

`crates/galion-backup/src/backup.rs`: `LEGACY_ROOT_DB` stays in `DBS`
(same MAP order) with two new rules, `legacy_archive_marker()` (read-only
scan of `/var/backups/memory-legacy-root-*/SHA256SUMS` for a
`<64 hex>  vectors.db` line, newest valid wins) and the F-406-2 guard;
`src/sqlite.rs`: `has_content()` (read-only; ignores `sqlite_*`,
`schema_migrations`, `embedding_meta`, FTS5 virtual + shadow tables).
Other DBs (instance memory, hermes-rs, hermes legacy) are untouched.

## VPS read-only probes (VPS clock UTC, converted)

- 21:43:14 CET: root `vectors.db` still present, 126,976 B, sha
  `cb0a3a5a...` (= slot 406), `-shm` mtime 19:19:54 CET (last backup);
  **no** `/var/backups/memory-legacy-root-*` (D-406-1 not run yet).
  galion-backup.service ExecStart `galion-backup-98466ab run`; timer last
  19:19:53 CET, next 2026-10-29 01:17:59 CET; journal 13:22 and 19:19 runs
  `ok`. Newest archive `galion-20261028-191953+0100` has
  `db/memory-root-vectors.db` + 3 others. 12 archives.
- `grep -c '^OPENMIND_MEMORY_INSTANCE_ID=galion$' /etc/openmind-memory.env` = 1
  (count only).
- 21:43 `scp -p` copy of the root `vectors.db` only (126 KB) to box
  `/workspace/swarm/scratch-427` (0700), hash `cb0a3a5a...` matched; used
  only for counts; **deleted 21:52**.
- 21:52:19 end state: root sha unchanged, openmind-memory MainPID 291
  NRestarts 0 active, `/health` 200; nerve `/state` 200 (galion-backup.service
  listed as protected unit, alert-only); galion-backup ExecStart unchanged.

## Box tests

`GALION_REAL=1 cargo test --release -p galion-backup --test
real_legacy_root_drop -- --nocapture --test-threads 1` (real release
`openmind-memory` on 127.0.0.1:22970/22971): **2/2 pass, 4 runs**
(21:45:08, 21:45:20 with `GALION_LEGACY_ROOT_COPY`, 21:51:4x x2).
Run with the live root DB copy (instance DB written by the real server):

| Scenario | exit | log lines (after `svc=galion-backup`) | MAP |
|---|---|---|---|
| S1 root present (live copy, 4 l0 rows) | 0 | `ok` only | 4, root line 2nd (unchanged) |
| S2 runbook steps 1-4 run literally (tar, sha256sum -c `vectors.db: OK`, mv) | 0 | `level=info msg="legacy db archived, skipped /var/lib/memory-tencentdb/vectors.db" marker=/var/backups/memory-legacy-root-20261029-091500+0100`, `ok`; **no warn** | 3 |
| S2b real server (instance galion) + capture after archive | 0 | same 2 lines; root trio still absent | 3 |
| S3 marker dir removed | 0 | `level=warn msg="missing db ..."` (old behaviour), `ok` | 3 |
| S4 F-406-2: real server WITHOUT instance id recreates root DB (4096 B, 0 l0 rows, `has_content=false`) | 0 | `level=warn msg="empty legacy db not backed up ..." hint="check OPENMIND_MEMORY_INSTANCE_ID (F-406-2)"`, `ok`; source sha unchanged | 3 |
| S5 rollback (rm empty, mv originals back, `vectors.db: OK`) | 0 | `ok` only | 4, sha `cb0a3a5a...`, 4 rows |

Every run: `sha256sum -c SHA256SUMS` ok on the extracted archive; instance
snapshot l0 rows == live instance store rows (4 -> 6 after the capture);
backup never created the root DB (trio absent after S2/S2b/S3).

`cargo test -p galion-backup --test fake_tree`: **8/8** (4 new):
`legacy_marker_detection` (no dir / no SHA256SUMS / bad hash / `-wal`
name / bare prefix rejected; newest valid wins),
`legacy_root_archived_with_marker_skipped_and_never_created`,
`legacy_root_empty_db_never_backed_up` (memory schema + FTS5 + meta rows,
0-byte file, then 1 content row -> backed up, MAP order kept),
`has_content_ignores_only_meta_and_shadow_tables` (quoted table names;
missing file is not created).

`GALION_REAL=1 ... --test real_parity` (bash oracle e43d7ab vs Rust, unshare
chroot): **26 passed, 0 failed**, with the dev release binary and with the
static x86_64-musl binary (`GALION_BACKUP_BIN`).

## Gates (box, 21:46-21:53 CET)

`cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets`
(excl openmind-sandbox, cgroup-detect) `-D warnings` ok; `cargo test
--workspace` (excl openmind-sandbox, cgroup-detect, hermes-tools) **382
suites, 2025 passed / 0 failed / 18 ignored** (base 381 / 2019: +1 suite,
+6 tests); `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 756 scanned,
0 missing; repo-check 0 errors / 83 warnings (= base). galion-backup
release musl: x86_64 1,432,064 B, aarch64 1,441,952 B (rust-lld);
`cargo check` armv7-musleabihf ok; aarch64-linux-android fails in the
`libsqlite3-sys` build script (no Android C toolchain on the box,
pre-existing, same as slot 381). Ports 22970-22979 free afterwards;
target dir removed.
