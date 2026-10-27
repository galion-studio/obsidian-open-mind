<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-backup: Rust port of galion-backup.sh (swarm/rust-ports, slot 47)

All times Europe/Zagreb (CEST, +02:00).

## Source of truth

* `deploy/foundation/galion-backup.sh` at ws/foundation `e43d7ab`, sha256
  `0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4`.
* VPS copy `/opt/galion/rust/bin/galion-backup.sh` (read-only `cat` over ssh,
  2026-10-20 22:57 +02:00): **byte-identical** (same sha256).
* VPS units (`systemctl cat`, read-only): `galion-backup.service`
  (oneshot, `ExecStart=/opt/galion/rust/bin/galion-backup.sh`, Nice=19,
  IOSchedulingClass=idle, UMask=0077) + drop-in `offsite.conf`
  (`ExecStartPost=/opt/galion/offsite/galion-offsite-encrypt.sh`,
  `ExecStopPost=... --perms-only`) + `galion-backup.timer`
  (`*-*-* 00/6:17:00`, RandomizedDelaySec=5min, Persistent=true).
  Service locale `LANG=C` (systemd environment), GNU tar 1.35, gzip 1.12.
* Live archive layout (newest, 2026-10-20 20:22:05 +02:00, names/modes only):
  `galion-STAMP/{SHA256SUMS,files/files.tar,db/{MAP,*.db}}`, all 0600/0700
  root:root; files.tar 1845 members; SHA256SUMS in C-locale order.

## What the binary is

`crates/galion-backup` (bin `galion-backup`). It **keeps** the four helper
subcommands the script called (`snapshot`, `verify`, `schema`,
`migrate-baseline`; same output, same exit codes; moved from ws/foundation
`crates/galion-backup/src/main.rs`) and **adds** `galion-backup run`, which
does the whole script in-process. Since slot 326 it also has
`galion-backup restore-test [ARCHIVE]`, the Rust port of the VPS
`galion-restore-test.sh` (mapping table in `src/restore.rs`, evidence in
`docs/real-tests/restore-test-rust.md`, staged unit + timer in
`deploy/staged/galion-restore-test.*`).

`run` step by step:

| Script step | Rust (`src/backup.rs`) |
|---|---|
| `umask 077` | `libc::umask(0o077)` |
| `DEST/KEEP/MINFREE_KB=${VAR:-default}` (`/var/backups/galion`, 12, 2097152) | same env vars, empty = default |
| `STAMP=$(TZ=Europe/Zagreb date +%Y%m%d-%H%M%S%z)` | zagreb-time (tz db compiled in), e.g. `20261020-202205+0200` |
| `log`: `ts=<date -Iseconds> svc=galion-backup ...` on stdout | identical format |
| `mkdir -p DEST; chmod 0700 DEST` | `create_dir_all` + `chmod 0700` (every run, like bash; the offsite ExecStopPost then sets 0710 root:galion-offsite as today) |
| `df -Pk` free check, `level=error msg="low disk NKB"`, exit 1 | `statvfs` f_bavail*f_frsize, KiB rounded up; same line, exit 1; non-integer MINFREE also takes this branch (as `[ -ge ]` did) |
| `exec 9>DEST/.lock; flock -n 9` else `level=warn msg="already running"`, exit 0 | `File::try_lock` (flock) on the same file (0600) |
| `mktemp -d DEST/.work-XXXXXX` + `trap 'rm -rf' EXIT` | 0700 dir, removed on every return path and on SIGTERM/SIGINT/SIGHUP (exit 128+sig, like bash) |
| `snap` x4: missing -> `level=warn msg="missing db SRC"`; snapshot + verify + `MAP` line; failure -> `level=error msg="snapshot failed SRC"`, abort exit 1 | same order, same messages (stderr of the helper identical too), online backup API in-process |
| `FILES` list + `dns-backup-*.json` glob, keep `[ -e ]` | same list/order; glob sorted bytewise (LANG=C) |
| `tar -C / --exclude='*.db' '*.db-wal' '*.db-shm' '.backup' -cf files.tar ...` (exit 1 tolerated) | own GNU-format tar writer: same members in the same (readdir) order, same headers; **byte-identical files.tar** in the parity test |
| `find . -type f ! -name SHA256SUMS \| sort -z \| xargs -0 sha256sum > SHA256SUMS` | own SHA-256, same lines, C-locale order; byte-identical |
| `tar -czf DEST/.galion-STAMP.tar.gz.part`; `chmod 0600`; `mv` | own tar + gzip (miniz_oxide deflate level 6); 0600; atomic rename |
| (none) | optional `GALION_BACKUP_GZIP_LEVEL` 0-9, default 6; invalid -> exit 1 before anything is written (swarm/backup-perf) |
| `ls -1t galion-*.tar.gz \| tail -n +KEEP+1 \| xargs rm -f` | mtime (ns) newest first, ties by name, same glob; same survivors |
| `du -k`, `kept=$(ls ... \| wc -l)`, `level=info msg="ok" archive=... size_kb=N kept=K` | `st_blocks` KiB rounded up; same line |

Backups stay **0600** (archives, lock, files inside) and **0700**
(DEST at backup time, work dir, dirs inside the archive), exactly as before.

## Behaviour diff vs bash

Verified identical (tests/real_parity.rs, 26/26): exit codes, stdout log
lines, stderr on snapshot failure, DEST/lock/archive modes, DEST contents,
outer archive member order/modes/owners/sizes, `files.tar` bytes,
`db/MAP` bytes, `SHA256SUMS` bytes, every extracted file (bytes + modes),
rotation survivors (incl. mtime ties), lock-held, low-disk, non-integer
MINFREE, missing DB, corrupt DB.

Differences (none in what is backed up, where, or with which permissions):

1. **gzip stream bytes**: deflate is miniz_oxide level 6, not GNU gzip; the
   decompressed tar is identical, the .tar.gz is a few bytes different in
   size (6904 B bash vs 6960 B Rust on the fake tree), so `size_kb` can
   differ by a block. The gzip header has no name and MTIME 0.
2. **Diagnostics only**: where bash died with a coreutils message or silently
   (tar stderr went to /dev/null), Rust prints one `galion-backup: <step>:
   <error>` line to stderr; exit codes are the same. Empty operand list
   (nothing at all to back up): both exit 1, Rust says why.
3. **Invalid `GALION_BACKUP_KEEP`** (not a non-negative integer) is refused
   up front (exit 1, nothing written). Bash would evaluate `$((KEEP+1))`
   after building the archive (an unknown name counted as 0 -> delete all
   archives, or an arithmetic error). Valid values behave identically.
4. **Usage text** of the helper now lists `run` first.
5. SIGTERM: bash defers its EXIT trap until the running child exits; Rust
   stops at the next 64 KiB block. Both remove the work dir and exit 143.

Not exercised on the box: hard links / fifos / sockets / >8 GiB files /
names >100 bytes are implemented like GNU tar (link entries, fifo entries,
sockets skipped, base-256 sizes, `././@LongLink`) and the first three plus
long names are covered by the fake tree; >8 GiB only by a unit test.

## Overlap with cutover-prep (galion-deploy pre-cutover backup)

Another swarm agent (cutover-prep) is adding a **pre-cutover backup
subcommand to galion-deploy**. That is a one-shot snapshot taken right
before a cutover. This crate is the **periodic** replacement for the 6-hour
timer job. They should not diverge: the cleanest end state is for
galion-deploy's pre-cutover step to call `galion-backup run` (optionally
with `GALION_BACKUP_DEST` pointed elsewhere and `GALION_BACKUP_KEEP` high)
or link `galion_backup::backup::run` as a library, so there is one
implementation of "what a Galion backup contains". Owner decision below.

Shared tar (swarm/shared-tar-crate, 2026-10-21 +02:00): both tools now use the
one std-only `crates/galion-tar` crate. galion-backup keeps only its tar
*policy* in `src/tarw.rs` (exclude set + libc `getpwuid_r`/`getgrgid_r`
names); the GNU writer itself moved there unchanged, and galion-deploy's ustar
writer/reader is re-exported from it. See docs/galion-tar.md.

## Deploy (DONE 2026-10-27 18:20 CET +01:00 by the t273u deploy; documented by slot 269)

The live route differs from the original proposal: instead of overwriting
`/opt/galion/rust/bin/galion-backup` and the base unit, t273u installed a
versioned binary and a drop-in, so rollback is one `rm` + `daemon-reload`.

* Binary `/opt/galion/rust/bin/galion-backup-98466ab` (main-staging 98466ab,
  static-pie x86_64 musl, sha256 `fb26d673…2163`). The old helper ELF
  `galion-backup` (= `galion-backup-pre-t273u`, sha256 `3df3f88d…1d09`) and
  `galion-backup.sh` (sha256 `0897efc0…80a4` = the parity oracle) stay in place.
* Base unit unchanged (still `ExecStart=…/galion-backup.sh`), mirrored in
  `crates/galion-backup/deploy/galion-backup.service`.
* Drop-in `galion-backup.service.d/90-deploy-t273u.conf` resets ExecStart to
  `galion-backup-98466ab run`, mirrored in
  `crates/galion-backup/deploy/galion-backup.service.d/90-deploy-t273u.conf`.
* ws/offsite drop-in `offsite.conf` and `galion-backup.timer` (mirrored)
  unchanged.
* Rollback: `rm /etc/systemd/system/galion-backup.service.d/90-deploy-t273u.conf; systemctl daemon-reload`.

Checks: `tests/deploy_units.rs` (always on; systemd ExecStart reset
semantics, rollback = script) and `tests/real_unit_live.rs` (GALION_REAL=1,
read-only `systemctl cat/show` over ssh: live effective ExecStart and
drop-in set = repo, last run success, live binary sha = recorded). Evidence:
docs/real-tests/backup-rust-unit.md.

### Owner decisions

* D1 ~~Deploy now / later / never~~ decided: deployed (t273u).
* D2 One backup implementation: galion-deploy pre-cutover calls
  `galion-backup run` (recommended) vs. two separate implementations.
* D3 Cross builds need a C compiler for SQLite (rusqlite `bundled`); the box
  has none for aarch64/armv7/android, so the zig-cc stopgap (pending owner
  exception, see common-migrate-2) was used for verification only.
* D-269-1 When to fold the drop-in into the base unit (and retire the .sh and
  the pre-t273u ELF): proposal after 12 good timer runs (= the 3-day
  retention window). Prepared by slot 311: staged folded unit, gate test and
  retirement checklist in docs/galion-backup-fold.md (gate CLOSED 1/12 at
  2026-10-27 21:56 CET; earliest open 2026-10-30 ~13:17 CET).
* D-269-2 B2 `size_kb` (du on ZFS before block allocation): still faithful to
  bash; live first Rust run logged `size_kb=2629` for an 18,729,100 B file.

## Performance (swarm/backup-perf, 2026-10-21 03:47 +02:00)

On a VPS-sized fake tree (46 MB SQLite, 1845 files.tar members, bash
3.46 s / 2.55 s CPU vs ~3 s / 2.5 s on the VPS), level 6: Rust gnu +15 %
wall / +18 % CPU, Rust musl +25 % / +31 %. Memory: one 5.8-7.4 MiB
process vs 14-19 bash processes peaking at 10.2 MiB together. The CPU gap is
miniz_oxide vs GNU gzip plus pure-Rust SHA-256 vs SHA-NI. The CRC-32 is now
slicing-by-8 (4.5x faster). `GALION_BACKUP_GZIP_LEVEL=5` gives -36 % CPU vs
bash for +6 % archive size. Details, raw numbers and owner decisions B1/B2:
`docs/real-tests/backup-perf.md`.
