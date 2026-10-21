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
does the whole script in-process:

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

## Deploy (NOT done; owner decision)

Nothing was installed, restarted or written on the VPS. Proposed cutover:

1. Copy the static `x86_64-unknown-linux-musl` release binary over
   `/opt/galion/rust/bin/galion-backup` (it is a superset of the deployed
   helper: same four subcommands + `run`).
2. Replace the unit with `crates/galion-backup/deploy/galion-backup.service`
   (only `ExecStart` changes to `galion-backup run`); keep the offsite
   drop-in and the timer; `systemctl daemon-reload`.
3. `systemctl start galion-backup.service` once, compare the archive listing
   and `SHA256SUMS` with the previous bash archive, check the offsite hook
   still logs `level=info msg=ok`.
4. Keep `galion-backup.sh` on the box for one retention window (3 days) as
   rollback (revert ExecStart).

### Owner decisions

* D1 Deploy now / after cutover-prep lands / never (keep bash).
* D2 One backup implementation: galion-deploy pre-cutover calls
  `galion-backup run` (recommended) vs. two separate implementations.
* D3 Cross builds need a C compiler for SQLite (rusqlite `bundled`); the box
  has none for aarch64/armv7/android, so the zig-cc stopgap (pending owner
  exception, see common-migrate-2) was used for verification only.
