<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-test evidence: legacy-vectors-archive-plan (wave-4 slot 406)

Branch `swarm/legacy-vectors-archive-plan` off `memory-db-stale-diag`
1d7164b4. Box + VPS read-only, 2026-10-28 17:19-17:38 CET (+01:00). Runbook:
docs/ops/legacy-vectors-archive.md. No VPS write, no deploy, no service
touched; `/opt/galion/openmind` not touched; no secret read (env files by
name only). Hermes not used. No new dependencies (docs/third-party: none).

## VPS read-only probes (times converted from the VPS UTC clock)

- 17:19:31 `ls/stat` of `/var/lib/memory-tencentdb`: root `vectors.db`
  126,976 B mtime 2026-10-13 11:20:17 +02:00; `-wal` 0 B; `-shm` mtime
  2026-10-28 13:22:03 +01:00; instance DB 4,009,984 B + WAL 4,173,592 B;
  dir 7.9 MB; pool 71 GB free.
- 17:21 `/proc/*/fd` scan, all pids: 0 holders of the root trio.
  Units referencing the dir: only `openmind-memory.service`
  (`ReadWritePaths`); no cron. `/etc/openmind-memory.env` names:
  HOST, PORT, DATA_DIR, INSTANCE_ID, TDAI_GATEWAY_API_KEY (values not read
  except `DATA_DIR=/var/lib/memory-tencentdb`, `INSTANCE_ID=galion`).
  `memory-gateway.service` masked (`TDAI_INSTANCE_ID=galion`).
- galion-backup: unit runs `galion-backup-98466ab run`; journal 13:22:03-13:22:08
  ok (+ offsite encrypt ok, kept=12). `tar -t` of all 12 archives
  (2026-10-26 01:18 -> 2026-10-28 13:22 CET): every one has
  `db/memory-root-vectors.db`, SHA256SUMS row `61389643f29a9f0a...` in all
  12; `tar -xO | sha256sum` of the newest = same; `db/MAP` maps it to the
  root path; `files.tar` has 1,821 memory entries and no `vectors.db`.
  Offsite dir: 14 entries incl. 12 `.age` + `latest`; encrypted, not opened.
- `size_kb` in the journal since 2026-10-26: `1` x8, 2629/3589/5894/8456 x1
  each, for 18.6 MB archives (F-406-1).
- galion-one `GET /api/v1/analytics` -> 401 without pairing cookie (cookie not
  used). Console argv (both :8080 and :18080): no `--memory-root`.
- `pgrep node` / `pgrep python` empty.
- 17:23 `scp -p` copy of the root DB and the instance DB + WAL to the box
  (`/workspace/swarm/scratch-406`, 0700; deleted at the end). Raw root hash
  `cb0a3a5a0eba869a5aa2159ceda6f1355199c31c45209d42087e263f2e6e748d` =
  the VPS `sha256sum`.
- 17:38:00 end state: root trio unchanged (same sizes/mtimes, atime still
  2026-10-27 18:19:42 +01:00), openmind-memory MainPID 291 NRestarts 0
  active, `/health` 200; nerve `/state` 200 (score 64 D, openmind-memory
  `healthy`); cortex `/health` 200. Temp files `/tmp/.x406`, `/tmp/.n406`
  removed.

## Box real tests

`GALION_REAL=1 GALION_MEMORY_SNAPSHOT_DIR=<box copy> cargo test --release -p
openmind-memory --test real_legacy_root_archive -- --test-threads 1`
(ports 22760-22762): **2/2 pass, 4 runs** (17:26:16, 17:27:12 x2, 17:37:32;
first run 17:25 failed on my own wrong expectation `l0_rows=5`: one
`/capture` of 2 messages added 1 l0 row; assert changed to "grew").

- `real_legacy_root_archive_rehearsal` (synthetic tree): before
  `stray_root_db=yes`, instance `l0_rows=3`; archive tar of 3 files, extract
  + re-hash identical, read-only open 4 rows; after removal no stray line,
  same instance verdict; real server (instance galion) `/health` 200 in
  ~51 ms, real `/capture` 200, instance l0 grew, root file NOT recreated, no
  content/session key in output; hazard: server without
  `OPENMIND_MEMORY_INSTANCE_ID` created a new empty root `vectors.db`
  (F-406-2); rollback restores sha `83e29a2a...` and the stray line.
- `real_legacy_root_archive_live_snapshot_copy` (throwaway copy of the live
  files): root tables `embedding_meta=0 l0_conversations=4 l1_records=0`;
  **root l0 rows also in the live instance: by record_id 4/4, by
  session_key+recorded_at 4/4**; freshness before exit 3, instance
  `l0_rows=2610 l1_rows=216`; archive sha `cb0a3a5a...` (asserted); after
  removal still exit 3, same counts, no stray line; real server
  `OPENMIND_MEMORY_READ_ONLY=1` on :22762 `/health` 200
  (`"read_only":true,"fts":true`), root not recreated; rollback sha
  `cb0a3a5a...`, root `l0_rows=4 l0_last=2026-10-13T11:19:53+02:00` again.
- Negative control 17:37: same test with the post-archive server started
  WITHOUT the instance id -> fails `server recreated vectors.db` (test
  restored afterwards).
- `cargo test --release -p galion-backup --test fake_tree` 4/4, 3 runs:
  new `legacy_root_memory_db_archived_then_restored` = backup with the root
  DB moved away exits 0, MAP 3 lines, `sha256sum -c` ok, instance snapshot
  300 rows, no root DB in files.tar; after putting it back the next run has
  `memory-root-vectors.db` again.
- Runbook text executed literally on a box copy (17:31, `D`/`A` pointed at
  the scratch tree; systemctl/curl/journalctl lines skipped): pre-check
  hash `cb0a3a5a...`, `vectors.db: OK`, trio moved, `ls` -> No such file;
  rollback block -> `vectors.db: OK`, trio back. Tar sha `f2602059...`
  (box run; the VPS run will differ by mtimes/owner, the DB hash will not).

## Gates (box, 17:31-17:38 +01:00)

`cargo fmt --check` ok; `cargo clippy --workspace --all-targets` (excl
openmind-sandbox, cgroup-detect) `-D warnings` ok; `cargo test --workspace`
(excl openmind-sandbox, cgroup-detect, hermes-tools) 381 suites 2019 passed /
0 failed / 18 ignored (base 380 / 2016: +1 suite, +3 tests);
`scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 755 scanned, 0 missing;
repo-check 0 errors / 83 warnings (= base). Only test + docs files changed,
no binary changes, so no musl/android rebuild needed.
