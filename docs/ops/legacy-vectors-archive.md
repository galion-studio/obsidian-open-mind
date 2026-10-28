<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Legacy root `vectors.db`: read-only check + archive runbook (wave-4 slot 406)

Branch `swarm/legacy-vectors-archive-plan` off `memory-db-stale-diag`
1d7164b4. Owner decisions D-388-2 (archive the legacy root DB) and D-388-4
(correct the slot 367 "last write Oct 13" note). **Plan only: nothing on the
VPS was written, moved, restarted or reconfigured.** Probes 2026-10-28
17:19-17:29 CET (+01:00), VPS read-only (`ls`/`stat`, `/proc/*/fd` scan,
`grep` of unit/config files, `systemctl show/cat`, `journalctl`, `tar -t`
and `tar -xO | sha256sum` of existing backup archives, loopback GET only,
`scp` *copy* of the DB files to the box). Env files printed as names only;
no secret value read. `/opt/galion/openmind` not touched. The VPS clock is
UTC; every time below is converted to Europe/Zagreb.

## Short answer

The legacy root DB holds **nothing the live store lacks**: 4 l0 rows, 0 l1,
0 embedding rows, and all 4 rows (same `record_id`, same `session_key` +
`recorded_at`) are also in the live instance DB. **No process holds it open**,
the live server never opens it, and it is in **every one of the 12 rolling
backups**. Archiving it is safe and reversible; the only side effects are a
`missing db` warn line in each later galion-backup run (exit stays 0) and one
store fewer in galion-one's memory panel. Recommendation: archive + move it
(D-406-1), keep the archive 30 days (D-406-2), then drop it from the backup
list (D-406-3).

## 1. The file (2026-10-28 17:19 CET)

| Path (under `/var/lib/memory-tencentdb/`) | Size | mtime (Zagreb) | Note |
|---|---|---|---|
| `vectors.db` | 126,976 B | 2026-10-13 11:20:17 +02:00 | root:root 0644, sha256 `cb0a3a5a0eba869a5aa2159ceda6f1355199c31c45209d42087e263f2e6e748d` |
| `vectors.db-wal` | 0 B | 2026-10-13 11:26:21 +02:00 | ctime 2026-10-28 13:22:03 +01:00 (backup reader) |
| `vectors.db-shm` | 32,768 B | 2026-10-28 13:22:03 +01:00 | touched by galion-backup each run |
| `instances/galion/vectors.db` (+WAL 4,173,592 B) | 4,009,984 B | 2026-10-18 23:42:30 +02:00 | the LIVE store (not touched by this plan) |

Whole data dir 7.9 MB; pool 71 GB free. Contents of the root DB (box copy,
counts only): `l0_conversations=4` (last row 2026-10-13 11:19:53 +02:00),
`l1_records=0`, `embedding_meta=0`. Overlap with the live instance DB: by
`record_id` 4/4, by `session_key`+`recorded_at` 4/4 (message text not
compared, never printed).

`.metadata/manifest.json` (2026-10-13 10:24:52 +02:00) still names
`vectors.db` as the store: it is the Node gateway's default-instance
manifest; no Rust code reads `manifest.json` (grep of `crates/`). Leave it
(D-406-4).

## 2. Readers

| Reader | Opens root `vectors.db`? | Evidence |
|---|---|---|
| any running process | **no** | `/proc/*/fd` scan of all pids 17:21 CET: 0 hits for the root trio |
| `openmind-memory.service` (pid 291, `openmind-memory-aa77864`) | no | env has `OPENMIND_MEMORY_INSTANCE_ID=galion` -> `Config::db_path` = `instances/galion/vectors.db`; box test: server with the instance id never creates/opens the root file |
| `galion-backup.timer` (~6 h; last 13:22:03 CET, next 19:20:04 CET) | **yes, online snapshot** | `DBS` in crates/galion-backup/src/backup.rs (+ the old .sh line 28); this is what touches `-shm` |
| `galion-one` `GET /api/v1/analytics` (pairing-cookie gated) | yes, read-only, on demand | `memory_db_paths` walks `/var/lib/memory-tencentdb` and counts rows of every `vectors.db` (sqlite_ro); picks the biggest as primary, lists all in `stores`. Live GET without cookie -> 401 (cookie not used) |
| console (Rust, :8080 / :18080) | no | live argv has no `--memory-root`; `port4::memory_snap` reads pyramid/scene/persona only |
| `memory-gateway.service` (Node) | no | masked; its env also has `TDAI_INSTANCE_ID=galion` |
| `/opt/galion/console-dist/server/_ssr/*.mjs` (Node SSR) | no | Node not running (`pgrep node` empty) |
| ad-hoc probes | sometimes | `-wal` atime 2026-10-28 06:26:35 +01:00 = slot 367 grep; DB atime 2026-10-27 18:19:42 +01:00 (relatime, reader unknown, 24 s before the 18:20:06 backup) |

No cron entry, no other unit references the path (`grep -r` of /etc,
/lib/systemd, /usr/lib/systemd, /etc/cron*, /var/spool/cron).

## 3. Backup coverage

- Local `/var/backups/galion`: 12/12 archives (2026-10-26 01:18 CET ->
  2026-10-28 13:22 CET, KEEP=12 = 3 days) contain `db/memory-root-vectors.db`
  and `db/MAP` maps it to `/var/lib/memory-tencentdb/vectors.db`. All 12
  `SHA256SUMS` rows = `61389643f29a9f0a...`; re-hashing the snapshot from the
  newest archive gives the same value. (It differs from the raw file hash
  `cb0a3a5a...` by design: the SQLite online backup rewrites the header and
  sets `journal_mode=delete`.)
- `files/files.tar` excludes `*.db`/`-wal`/`-shm`, so the raw file is only in
  `db/` (1,821 `memory-tencentdb` entries, no `vectors.db`).
- Offsite: `/var/backups/galion/offsite` has 12 `.age` copies (+`latest`);
  encrypted, not opened (no key read); same source archives.
- **After removal** the rolling copies age out in 3 days (12 x 6 h). The
  archive dir from step 3 below is then the only copy -> D-406-2.

## 4. Runbook (OWNER runs it; root on the VPS; ~5 min)

Do not run within 10 minutes of a galion-backup run
(`systemctl list-timers galion-backup.timer`). No service restart is needed:
nothing holds the file open.

```sh
# 0. preconditions (prints counts only, never values)
D=/var/lib/memory-tencentdb
grep -c '^OPENMIND_MEMORY_INSTANCE_ID=galion$' /etc/openmind-memory.env   # must be 1
systemctl list-timers galion-backup.timer --no-pager                       # not within 10 min
for p in /proc/[0-9]*; do ls -l $p/fd 2>/dev/null | grep -q "$D/vectors.db" && echo "OPEN by ${p#/proc/}"; done  # must print nothing

# 1. pre-check
ls -l --time-style=full-iso $D/vectors.db*
sha256sum $D/vectors.db        # expect cb0a3a5a0eba869a5aa2159ceda6f1355199c31c45209d42087e263f2e6e748d

# 2. archive (same filesystem, so mv is an atomic rename)
TS=$(TZ=Europe/Zagreb date +%Y%m%d-%H%M%S%z)
A=/var/backups/memory-legacy-root-$TS
install -d -m 0700 $A $A/moved
tar -C $D -cpf $A/legacy-root-vectors.tar vectors.db vectors.db-wal vectors.db-shm
( cd $D && sha256sum vectors.db ) > $A/SHA256SUMS
( cd $A && sha256sum legacy-root-vectors.tar ) >> $A/SHA256SUMS
chmod 0600 $A/legacy-root-vectors.tar $A/SHA256SUMS

# 3. verify the archive before touching the original
install -d -m 0700 $A/verify && tar -C $A/verify -xpf $A/legacy-root-vectors.tar
( cd $A/verify && sha256sum -c <(head -1 $A/SHA256SUMS) )   # vectors.db: OK
rm -r $A/verify

# 4. move the originals out of the data dir (second copy, keeps mtimes)
mv $D/vectors.db $D/vectors.db-wal $D/vectors.db-shm $A/moved/

# 5. post-check
ls $D/vectors.db* 2>&1                      # No such file
systemctl show -p ActiveState -p NRestarts -p MainPID openmind-memory.service   # active, 0, same pid
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8420/health          # 200
curl -s http://127.0.0.1:8670/state | grep -o "\"name\":\"openmind-memory\",\"note\".\{0,700\}" | grep -o "\"status\":\"[a-z]*\"" | head -1   # "status":"healthy"
# after the next galion-backup run:
journalctl -u galion-backup.service -n 5 --no-pager   # warn: missing db /var/lib/memory-tencentdb/vectors.db ; then ok ; exit 0
```

Expected after the next backup: archive has `db/memory-vectors.db`,
`hermes-rs-state.db`, `hermes-legacy-state.db`, MAP 3 lines, no
`memory-root-vectors.db`; nerve keeps `galion-backup` ok (it judges the unit
result / exit status, not warn lines). galion-one's analytics `stores` list
shrinks from 2 to 1 (primary unchanged: it already picked the bigger
instance DB).

### Rollback (any time while `$A` exists)

```sh
mv $A/moved/vectors.db $A/moved/vectors.db-wal $A/moved/vectors.db-shm /var/lib/memory-tencentdb/
# or, if moved/ is gone: tar -C /var/lib/memory-tencentdb -xpf $A/legacy-root-vectors.tar
( cd /var/lib/memory-tencentdb && sha256sum -c <(head -1 $A/SHA256SUMS) )
```

The next galion-backup run includes it again (box test below). No service
restart either way.

### Do NOT

- remove `OPENMIND_MEMORY_INSTANCE_ID` from `/etc/openmind-memory.env`: the
  box rehearsal shows the server then opens the default instance and creates
  a **new empty** root `vectors.db` (memory would look empty) - F-406-2;
- touch `instances/galion/` (the live store), `.metadata/`, or the rolling
  backups; delete `$A` before D-406-2 says so.

## 5. Box rehearsal (real binaries, real data copy)

`crates/openmind-memory/tests/real_legacy_root_archive.rs` (GALION_REAL=1,
ports 22760-22762) runs the steps above with the real release
`openmind-memory` and system `tar`/`sha256sum`, on a synthetic tree and on a
throwaway copy of the live data files; `crates/galion-backup/tests/fake_tree.rs`
`legacy_root_memory_db_archived_then_restored` runs the real backup library
before/after. Numbers in docs/real-tests/legacy-vectors-archive-plan.md.

## Owner decisions

- **D-406-1 (= D-388-2) archive now?** Recommended **yes**, with the runbook
  above (owner runs it; this slot changed nothing). Alternative: keep it;
  `freshness` already labels it `stray_root_db` and ignores it.
- **D-406-2 retention of `$A`:** recommended keep 30 days, then delete; the
  rolling backups lose it 3 days after step 4.
- **D-406-3 drop the root entry from galion-backup `DBS`** (Rust + the old
  .sh) after D-406-1, to stop a 6-hourly `missing db` warn line; code change
  + deploy in its own slot. Not needed for correctness (exit 0).
- **D-406-4 `.metadata/manifest.json`:** leave (recommended; unread) or add
  it to the archive.
- **D-388-4 done here:** slot 367 note corrected in
  docs/real-tests/memory-search-local-time.md.

## Findings

- **F-406-1** (cosmetic, parity with the old .sh): galion-backup logs
  `size_kb=1` for 18 MB archives in 8 of 12 runs since 2026-10-26 - `du`
  right after the write on ZFS sees unallocated blocks. Not fixed.
- **F-406-2** (hazard): see "Do NOT". A start-up check that refuses the
  default instance when `instances/` exists would close it; proposal only.
