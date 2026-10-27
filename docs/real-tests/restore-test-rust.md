<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: restore-test-rust (wave-4 slot 326)

All times Europe/Zagreb CET (+01:00). Branch `swarm/restore-test-rust` off
`swarm/backup-dropin-fold` 5d72cc98. Code commit 2595481. **No live unit or
service change on the VPS**; no ports used (nothing serves HTTP; assigned
range 21960-21969 unused).

## What was ported
`/opt/galion/rust/bin/galion-restore-test.sh` (sha256
`26439775…0f14`, read-only `cat` 2026-10-28 00:16 CET). It has **no unit
and no timer** on the VPS (`systemctl list-unit-files '*restore*'` empty,
no cron reference): it only ran by hand. Its line 11 calls the pre-t273u
helper ELF `/opt/galion/rust/bin/galion-backup verify` (the slot 311
blocker). New: `galion-backup restore-test [ARCHIVE]` (same env
`GALION_BACKUP_DEST`, same stdout line for line), plus staged (NOT
installed) `deploy/staged/galion-restore-test.{service,timer}`.

## Finding F-326-1 (live .sh bug)
`sha256sum --quiet -c SHA256SUMS && echo "checksums ok"` is an `&&` list,
so `set -e` does not fire when a checksum fails. The live .sh prints
`./db/MAP: FAILED`, carries on, and ends with `restore test ok: …` and
**exit 0** for a tampered or incomplete archive (reproduced: S4, S5 below).
The Rust port stops at the first failed checksum with exit 1.

## Box: `real_restore_test` (2026-10-28 00:20 CET)
`GALION_REAL=1 cargo test --release -p galion-backup --test real_restore_test -- --nocapture --test-threads=1`

Real archives come from the real `galion-backup run` over a fake VPS tree
(placeholder secrets only, 8 MiB extra blob). The oracle is the live .sh,
verbatim and sha256-checked, with its one helper path pointed at this
build's `galion-backup verify`.

```
[2026-10-28T00:20:27+01:00] archives: galion-20261028-002024+0100.tar.gz (8398494 B), galion-20261028-002026+0100.tar.gz (8398538 B)
[2026-10-28T00:20:27+01:00] PASS S1 newest (no arg): stdout identical (11 lines), sh exit Some(0) 181 ms, rs exit Some(0) 119 ms
[2026-10-28T00:20:27+01:00] PASS S1b newest is by mtime: stdout identical (11 lines), sh exit Some(0) 168 ms, rs exit Some(0) 71 ms
[2026-10-28T00:20:27+01:00] PASS S2 explicit arg: stdout identical (11 lines), sh exit Some(0) 116 ms, rs exit Some(0) 63 ms
[2026-10-28T00:20:28+01:00] PASS S3 GNU-tar repacked archive: stdout identical (11 lines), sh exit Some(0) 88 ms, rs exit Some(0) 100 ms
[2026-10-28T00:20:28+01:00] PASS S4 checksum mismatch: F-326-1 reproduced: .sh exit 0 + `restore test ok: galion-badsum.tar.gz` despite `./db/MAP: FAILED`; rust exit Some(1) (`galion-backup: restore-test: checksum check failed (1 mismatched, 0 unreadable of 6)`)
[2026-10-28T00:20:29+01:00] PASS S5 listed file missing: F-326-1 reproduced: .sh exit 0 + `restore test ok: galion-missing.tar.gz` despite `./db/MAP: FAILED open or read`; rust exit Some(1) (`galion-backup: restore-test: checksum check failed (0 mismatched, 1 unreadable of 6)`)
[2026-10-28T00:20:29+01:00] PASS S6 corrupt DB, sums consistent: stdout identical (103 lines), sh exit Some(1) 170 ms, rs exit Some(1) 62 ms
[2026-10-28T00:20:29+01:00] PASS S7 truncated gzip: stdout identical (0 lines), sh exit Some(2) 71 ms, rs exit Some(1) 20 ms
[2026-10-28T00:20:29+01:00] PASS S8 no archives: stdout identical (0 lines), sh exit Some(2) 3 ms, rs exit Some(1) 2 ms
[2026-10-28T00:20:29+01:00] PASS S9 missing archive arg: stdout identical (0 lines), sh exit Some(2) 4 ms, rs exit Some(1) 1 ms
[2026-10-28T00:20:31+01:00] PASS S10 persona missing: stdout identical (11 lines), sh exit Some(0) 128 ms, rs exit Some(0) 97 ms
[2026-10-28T00:20:31+01:00] PASS S11 two galion-* dirs: stdout identical (0 lines), sh exit Some(1) 77 ms, rs exit Some(1) 19 ms
[2026-10-28T00:19:38+01:00] PASS hostile dotdot: refused (galion-backup: restore-test: <tmp>/dest/galion-dotdot.tar.gz: refusing member with '..': galion-x/../../../outside/pwn)
[2026-10-28T00:19:38+01:00] PASS hostile symlink-escape: refused (galion-backup: restore-test: <tmp>/dest/galion-symlink-escape.tar.gz: refusing to extract through symlink galion-x/esc/pwn)
[2026-10-28T00:20:36+01:00] SIGTERM at 30 ms -> exit Some(143), temp dir removed
[2026-10-28T00:20:36+01:00] SIGTERM at 80 ms -> exit Some(143), temp dir removed
[2026-10-28T00:20:37+01:00] SIGTERM at 150 ms -> exit Some(143), temp dir removed
test result: ok. 4 passed; 0 failed (VPS half skipped without GALION_VPS_SSH)
```
S1-S3, S10: stdout byte-identical, both exit 0 (incl. a GNU `tar -czf`
repack, so GNU-format members and @LongLink names are read too). S6-S9, S11:
both fail with identical stdout (S6 prints the multi-line SQLite integrity_check message, 103 lines, identically). S4/S5: F-326-1. Hostile archives (Rust
only): `..` members and writes through a symlink are refused, nothing lands
outside. SIGTERM at 30/80/150 ms: exit 143 and the temp dir is gone each time.

## VPS: `real_vps_restore_parity` on all 12 REAL archives (2026-10-28 00:21-00:22 CET)
`GALION_REAL=1 GALION_VPS_SSH=slavik-vps GALION_RESTORE_VPS_BIN=<x86_64 musl galion-backup>` (static musl
build sha256 `1643d255…f719`, built from 2595481's tree).
The test copies the binary to a throwaway `/var/tmp/restore-test-326-<stamp>`
(0700), runs the live .sh and `galion-backup restore-test` on **every**
`/var/backups/galion/galion-*.tar.gz` (12: 2026-10-25 07:19 … 2026-10-27
19:18 CET, i.e. bash-made archives and the Rust-made ones after the t273u
cutover), diffs stdout, then removes the folder. Only names, counts and
ok/FAILED are printed; no file content.

Result: **12/12 STDOUT_IDENTICAL, 12/12 sh_rc=0, 12/12 rs_rc=0**, rust temp
dir empty after every run, no `/tmp/galion-restore-*` left, folder removed
(re-checked: 0 left). Every archive: `checksums ok`, 4 DBs integrity ok
(tables 25 / 4 / 15 / 16), `files restored: 1794`, all 4 presence checks
`present`. Time per 18 MB archive on the loaded VPS (loadavg ~14): .sh
1.2-1.3 s, Rust (nice 10) 1.4-1.5 s. Last two runs as printed:
```
archive=galion-20261025-131808+0100.tar.gz size=18642141
sh_rc=0
rs_rc=0
sh_ms=1229 rs_ms=1457
STDOUT_IDENTICAL
t_left=0 tmp_left=0
archive=galion-20261025-071947+0100.tar.gz size=18637373
sh_rc=0
rs_rc=0
sh_ms=1300 rs_ms=1537
STDOUT_IDENTICAL
t_left=0 tmp_left=0
cleaned=/var/tmp/restore-test-326-20261028-002156+0100
```
Nerve (`127.0.0.1:8670/state` GET, ts 2026-10-28T00:15:02+01:00): HTTP 200,
16 components healthy; still **no galion-backup / restore-test component**
(the slot 311 observability gap stands). Cortex: not probed live (its health
GET gave no answer on 8680); nerve used as the real-time view.

## Offline unit checks
* `tests/restore_units.rs`: staged service runs `galion-backup-2595481
  restore-test` (no .sh), Type=oneshot, UMask 0077, PrivateTmp, PrivateNetwork,
  NoNewPrivileges, ProtectSystem=strict, ProtectHome; timer 03:47 +10 min
  never overlaps a 00/6:17 backup window. 2/2 pass.
* `systemd-analyze --user verify` (systemd 257, box; VPS runs 255, all keys
  predate 255) on the staged pair with ExecStart pointed at the box build: rc 0.

## Builds
x86_64 musl static (used on the VPS), aarch64 musl and armv7 musl release
builds, aarch64-android and armv7-android `cargo check`: all Finished (C
SQLite cross-compiled with zig cc 0.15.2 per docs/toolchain-aarch64.md;
without it libsqlite3-sys fails exactly as before this slot). No qemu on the
box, so the aarch64 binary was not executed.

## Gates (2026-10-28 ~00:30 CET)
`cargo fmt --check` clean; `cargo clippy --workspace --all-targets --exclude
openmind-sandbox --exclude cgroup-detect -- -D warnings` rc 0 (only the
vendored candle-core deprecation notes); `cargo test --workspace --exclude
openmind-sandbox --exclude cgroup-detect`: 390 suites, 2057 passed, 0 failed,
18 ignored; galion-backup alone: lib 15 (incl. 5 new gunzip + 2 restore unit
tests), restore_units 2, all others unchanged and passing;
`scripts/no-python.sh` ok. Sandbox crates not run here (sandbox_vps_only;
this slot does not touch them).
