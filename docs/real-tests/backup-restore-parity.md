<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# backup-restore-parity (wave-4 slot 279): Rust galion-backup archives restore like bash ones

All times Europe/Zagreb (CET, +01:00). Branch `swarm/backup-restore-parity`, base
main-staging `98466ab`. Box-only round trip plus read-only VPS checks. **No live
timer, unit, permission or file change on the VPS.** No ports used (CLI only;
the assigned range 21490-21499 stayed free).

## Why

At 18:20 the t273u deploy switched the live `galion-backup.service` to
`/opt/galion/rust/bin/galion-backup-98466ab run` (drop-in `90-deploy-t273u.conf`).
`tests/real_parity.rs` (26/26) had already shown the Rust archive matches the
bash one byte for byte (except the gzip stream). What was missing: proof that
an archive **restores** to the same machine state. This slot adds that proof.

## Binary under test = the deployed ELF

| ELF | sha256 | where |
|---|---|---|
| `galion-backup-98466ab` (static x86_64 musl, 1 959 376 B) | `fb26d673cfe6c615bf7272c81b0b076d92f4c05249bc5b8d40440b18fc1a2163` | box `target/backup-deploy-t273u/...` **and** VPS `/opt/galion/rust/bin/galion-backup-98466ab` (same sha256, read 18:24) |
| cargo release build of this branch (glibc, 1 302 912 B) | `6098f713...661d` | box only |
| oracle `galion-backup.sh` | `0897efc0...80a4` | git `e43d7ab` = VPS copy (same sha256) |

## The test: `crates/galion-backup/tests/real_restore_parity.rs`

```
GALION_REAL=1 [GALION_RESTORE_TREE=vps] [GALION_BACKUP_BIN=/path/galion-backup-98466ab] \
  cargo test --release -p galion-backup --test real_restore_parity -- --nocapture
```

Same unprivileged `unshare -rm` + chroot jail as real_parity.rs; bash and Rust
each back up the same tree at the same absolute paths, then:

| Step | What is proven |
|---|---|
| R1 | `gzip -t`, GNU tar extract, `sha256sum -c --strict SHA256SUMS` = 6/6 OK (files.tar, MAP, 4 DBs) |
| R2 | restore into an empty root (DBs per `db/MAP`, then `tar -xpf files.tar`) == source tree: bytes, modes, uid/gid, mtimes, symlink targets, hard-link groups, fifo; excluded `*.db`/`-wal`/`-shm`/`.backup`/`dir.db/` not restored |
| R3 | every restored DB: `galion-backup verify` integrity ok + logical dump (every row of every table) == the live source, incl. a committed row that only lives in the **uncheckpointed WAL of a connection held open** during the backup |
| R4 | restore(bash) == restore(rust), whole tree incl. DB bytes |
| R5 | GNU-free restore path: miniz_oxide inflate + `galion-tar` reader read **both** archives (cross-tool) and every member matches the source |
| R6 | disaster round trip: wipe the source (operands + DBs), restore in place from the Rust archive, back up again with Rust: restored tree == original; second `files.tar` **byte-identical** to the first; DB rows == original; `db/MAP` identical |
| R7 | negative controls: 1 flipped bit in files.tar -> `sha256sum -c` FAILED; truncated archive -> `gzip -t`, `tar -x` and the Rust reader all refuse |

## Results (box, 2026-10-27)

| Run (18:30-18:31) | Tree | Checks |
|---|---|---|
| deployed ELF fb26d673 | fake edge-case tree (34 entries: long names, hard link, symlink, dangling symlink, fifo, excludes, dns glob) | **43/43** |
| deployed ELF fb26d673 | VPS-sized tree (1845 files.tar members, 4 DBs ~46 MB, 34 407 rows, held-open WAL) | **43/43** (24.5 s) |
| cargo build 6098f713 | fake tree | **43/43** |
| cargo build 6098f713 | VPS-sized tree | **43/43** (23.3 s) |

VPS-sized run with the deployed ELF (excerpt):

```
bash: exit 0 ... archive=galion-20261027-183038+0100.tar.gz size_kb=17524 kept=1   (17 940 638 B)
rust: exit 0 ... archive=galion-20261027-183042+0100.tar.gz size_kb=17540 kept=1   (17 958 196 B)
PASS R1 rs: sha256sum -c SHA256SUMS all OK (files.tar, MAP, 4 DBs)          6/6 OK
PASS R2 rs: restored file tree == source (1845 entries ...)
PASS R3 rs: .../instances/galion/vectors.db integrity ok + 3960 rows == live source
PASS R3 rs: /var/lib/memory-tencentdb/vectors.db integrity ok + 54 rows == live source
PASS R3 rs: /var/lib/hermes-rs/rs-state.db integrity ok + 659 rows == live source
PASS R3 rs: /opt/galion/hermes/state.db integrity ok + 29734 rows == live source
PASS R3 rs: WAL-only row restored
PASS R4 restore(bash) == restore(rust): 1849 entries incl. 4 DBs   (DB files byte-identical: true)
R5 rs: outer 10 members, files.tar 1845 members (1793 regular files byte-checked)
PASS R6 backup->restore->backup fixed point: files.tar byte-identical (7792640 B)
RESULT: 43 passed, 0 failed
```

Regression gates on the branch (18:33-18:34): `real_parity` 26/26, `real_perf`
(GALION_PERF_RUNS=1) 0 failed (files.tar + SHA256SUMS byte-identical at VPS
scale, 1845 members, RSS < 64 MiB), `cargo test --release -p galion-backup
-p galion-tar -p galion-deploy` all ok, `cargo fmt --all --check` ok, clippy
`-D warnings` (galion-backup + galion-tar, all targets) ok, `scripts/no-python.sh` ok.

## Live VPS evidence (read-only, 18:31-18:33)

Archives read by streaming only (`tar -O`, `--to-command`; nothing extracted
to disk, no content printed). Compared the **first live Rust archive**
`galion-20261027-182006+0100.tar.gz` (18:20, galion-backup-98466ab) with the
**last live bash archive** `galion-20261027-131713+0100.tar.gz` (13:17):

| Check | bash 13:17 | Rust 18:20 |
|---|---|---|
| `gzip -t` | OK | OK |
| outer members / modes | 10; 7x `-rw-------` + 3x `drwx------` root/root | same |
| SHA256SUMS lines / re-hashed in stream, diff lines | 6 / 0 | 6 / 0 |
| files.tar members (types) | 1845 (1794 files, 51 dirs) | 1845 (1794, 51) |
| files.tar names only in one side | 0 | 0 |
| `db/MAP` diff | - | 0 lines |
| DB headers `SQLite format 3` | 4/4 | 4/4 |
| DB sizes B (memory, root-vectors, hermes-rs, legacy) | 4075520, 126976, 1314816, 40779776 | 4075520, 126976, 1339392 (live growth), 40779776 |

Unit state: `Result=success ExecMainStatus=0`; journal 18:20:09 `msg="ok"
archive=galion-20261027-182006+0100.tar.gz size_kb=2629 kept=12` (size_kb is
ZFS-compressed allocation, as with bash), offsite `.age` 18:20:10 `msg=ok`.
Next timer run 19:17. Nerve `/state` status healthy, score 78; the cortex
logged `R1-protected unit:galion-backup.service -> none (no action)` at
18:20:11 (the manual oneshot; alert only, correct).

## Findings

1. **Outer member order differs on the VPS, but bash differs from itself too.**
   /var/backups is ZFS (hash-ordered readdir); the five newest live archives
   (4 bash, 1 Rust) have five different outer orders. Restore is order-
   independent; content parity holds (table above). Not a regression.
2. **Restore order matters for directory mtimes** (found by this test at VPS
   scale): copying DBs *after* `tar -xpf` bumps the mtime of
   `/var/lib/hermes-rs`, `/var/lib/memory-tencentdb`, `.../instances/galion`.
   Correct order: DBs first, then `tar -xpf files.tar` (tar sets dir mtimes
   last). Cosmetic, same for both implementations.
3. **DB file owner/mode are not in the archive** (both implementations: DBs
   are snapshot files 0600 root). Live today: both memory DBs `root:root 644`,
   hermes-rs and legacy `root:root 600`. A restore must `chmod 644` the two
   memory DBs (they come back 0600).
4. **Stale `-wal`/`-shm` must be removed** before an in-place DB restore, or
   SQLite would replay an old WAL onto the restored file.
5. In the jail the bash script uses the binary under test as its `snapshot`
   helper (as on the VPS before 18:20 it used the older helper ELF `3df3f88`),
   so DB bytes are identical across implementations here; on the VPS the
   comparison above is by header/size/row-agnostic means only (no DB
   extracted on the production host).

## Restore runbook (what R2/R6 execute)

See docs/galion-backup.md "Restore". Summary, as root on the target host,
services that own the paths stopped:

```
umask 077; W=$(mktemp -d /var/tmp/galion-restore-XXXXXX)
tar -xpzf /var/backups/galion/galion-STAMP.tar.gz -C "$W"
cd "$W"/galion-STAMP && sha256sum -c --strict SHA256SUMS      # must be 6x OK
while read -r src name; do rm -f "$src-wal" "$src-shm"; install -m 0600 "db/$name" "$src"; done < db/MAP
chmod 0644 /var/lib/memory-tencentdb/instances/galion/vectors.db /var/lib/memory-tencentdb/vectors.db
tar -xpf files/files.tar -C /
/opt/galion/rust/bin/galion-backup-98466ab verify <each restored db>  # integrity=ok
rm -rf "$W"
```

## Owner decisions

* **D-279-1**: adopt the restore runbook above in docs/galion-backup.md (done
  on this branch) and do one supervised restore drill on a non-production
  host (not the VPS) from a real offsite `.age` copy.
* **D-279-2**: store DB owner/mode in `db/MAP` (a third column) so restores are
  exact without a manual `chmod`? It changes the archive format vs bash.
* **D-279-3**: make `galion-backup` write the outer members in a fixed order
  (sorted) for reproducible archives? Purely cosmetic; bash never had it.
