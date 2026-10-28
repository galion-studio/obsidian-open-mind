<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Staged swap: galion-restore-test.sh -> Rust `restore-test` (wave-4 slot 361)

**STAGED, NOT INSTALLED.** Nothing here has been run on the VPS. Times are
Europe/Zagreb CET (+01:00). Branch `swarm/restore-checksum-fail`, code commit
`dd89bea4` (off `swarm/restore-test-rust` bc741f2a).

## Why
F-326-1: the live `/opt/galion/rust/bin/galion-restore-test.sh` (sha256
`264397750a31083298e8e926934d02ce84418c2b3501a12bb0af74ada8170f14`) ends with
`restore test ok` and **exit 0** after a checksum failure, because
`sha256sum --quiet -c SHA256SUMS && echo "checksums ok"` is an `&&` list that
`set -e` ignores. Slot 361 measured it on real archives: of 14 tamper kinds the
.sh lets **10 through** with exit 0 (box), and on tampered copies of the
newest REAL VPS archive it let 3 of 5 through (dropped SHA256SUMS line,
unlisted file, malformed line). The Rust `galion-backup restore-test` at
dd89bea4 fails hard on all 14 (box) and 5/5 (VPS), and passes all 12 real VPS
archives with stdout byte-identical to the .sh (no false positives).

Also fixed on the way (F-361-1): the slot-326 Rust port stopped reading the
gzip stream at the tar end-of-archive blocks, so a flipped bit in the gzip
CRC32 / ISIZE trailer passed. Now the stream is always drained and the
trailer verified (`gzip check FAILED: ... crc error`).

## Binary
| | |
|---|---|
| build | `cargo build --release --target x86_64-unknown-linux-musl -p galion-backup` at dd89bea4 |
| size | 1513984 B, static musl |
| sha256 | `30f58ba4a8e3d8b7b230fdbb6144858193164284d8c79ff3d35f5f178dbc51c6` |
| install name | `/opt/galion/rust/bin/galion-backup-dd89bea4` (0755 root:root) |

The ELF is multi-call: invoked as `galion-restore-test` or
`galion-restore-test.sh` (via a symlink) it behaves exactly like
`galion-backup restore-test [ARCHIVE]`, so callers of the .sh path keep
working with the same argument and the same stdout.

## D-361-1 (owner go needed; replaces the restore part of D-326-1)
Gate: owner says go. Nothing here restarts or touches a running service; the
.sh has no unit, timer or cron reference (only run by hand), so the swap has
no live consumer to break.

1. Copy the binary and check it:
   ```
   install -m 0755 -o root -g root galion-backup /opt/galion/rust/bin/galion-backup-dd89bea4
   sha256sum /opt/galion/rust/bin/galion-backup-dd89bea4   # must be 30f58ba4...51c6
   ```
2. Dry run by hand on the newest archive; expect `checksums ok`, 4x
   `integrity=ok`, `files restored: N`, 4x `present:`, `restore test ok: ...`, exit 0:
   ```
   /opt/galion/rust/bin/galion-backup-dd89bea4 restore-test; echo rc=$?
   ```
3. Swap (keep the old script next to it):
   ```
   cd /opt/galion/rust/bin
   mv galion-restore-test.sh galion-restore-test.sh.pre361
   ln -s galion-backup-dd89bea4 /opt/galion/rust/bin/galion-restore-test.sh
   /opt/galion/rust/bin/galion-restore-test.sh; echo rc=$?        # same output as step 2
   ```
4. Optional, separate go: install the staged
   `crates/galion-backup/deploy/staged/galion-restore-test.{service,timer}`
   (now `ExecStart=/opt/galion/rust/bin/galion-backup-dd89bea4 restore-test`,
   daily 03:47 + up to 10 min, sandboxed) and
   `systemctl enable --now galion-restore-test.timer`. A failed run then
   shows up as a failed unit (exit 1) that the nerve already sees as
   `unit:` incidents.

### Rollback
```
cd /opt/galion/rust/bin
rm galion-restore-test.sh && mv galion-restore-test.sh.pre361 galion-restore-test.sh
sha256sum galion-restore-test.sh   # 26439775...0f14 again
# if step 4 was done:
systemctl disable --now galion-restore-test.timer
rm /etc/systemd/system/galion-restore-test.{service,timer} && systemctl daemon-reload
```
`galion-backup-dd89bea4` can stay (nothing else uses it) or be removed.

## What "fail hard" means now (exit 1, one stderr line naming the archive)
* any listed file whose digest differs -> `NAME: FAILED` on stdout (coreutils text)
* any listed file missing/unreadable -> `NAME: FAILED open or read`
* any malformed SHA256SUMS line (like `sha256sum --strict`)
* any listed name that is absolute, has `..`, or is / goes through a symlink
* any regular file in the archive that SHA256SUMS does not list, and any
  entry that is not a regular file or directory
* gzip CRC32 / ISIZE trailer mismatch; tar header checksum mismatch
Last stderr line, e.g.:
`galion-backup: restore-test: /var/backups/galion/galion-X.tar.gz: checksum check FAILED: 1 mismatched [./db/MAP], 0 unreadable, 0 malformed line(s), 0 unsafe name(s), 0 unlisted (5 of 6 listed files ok); this backup is NOT verified, do not restore from it`
Nothing after the failure runs (no `checksums ok`, no DB verify, no
`restore test ok`); the temp dir is removed.
