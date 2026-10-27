<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Slot 304 backup-size-kb-fix — real tests (2026-10-27, Europe/Zagreb CET +01:00)

Branch `swarm/backup-size-kb-fix`, base `swarm/backup-rust-unit` eada2741.
Fixes D-269-2 / t315u: the `size_kb=` field of the `msg="ok"` log line.

## Cause

`du_kb()` = `st_blocks * 512 / 1024` (like `du -k`), read right after the
rename. The VPS backup dataset is ZFS (`rpool/subvol-1022217-disk-0`), which
allocates blocks at the next txg sync, so the Rust (and earlier bash) runs
logged `size_kb=1`, `2629`, `3589`, … for ~18.7 MB archives.

## Fix

`backup::size_kb()` = `len.div_ceil(1024)` (= `du -k --apparent-size`), used
for the log field only. `du_kb()` is kept (public, unchanged). Log line text
and field order unchanged. No dependency added.

## Evidence

| Check | Result |
|---|---|
| unit tests galion-backup (lib) | 11/0 (3 new: rounding 0/1/1023/1024/1025 B + 18,729,100 B -> 18291; sparse 64 MiB -> 65536 while du_kb < 65536; missing file errors) |
| all galion-backup tests (debug) | lib 11, deploy_units 6, fake_tree 3, real_* stubs pass |
| `real_size_kb` fake tree (21:05:36) | len 6944 B, size_kb 7, du alloc 8, du --apparent-size 7 |
| `real_size_kb` VPS read-only (21:05:38) | newest `galion-20261027-191858+0100.tar.gz` on zfs: len 18,733,188 B, du -k 18318, du --apparent-size 18295, live logged `size_kb=1`, fixed formula 18295 = apparent |
| `real_parity` bash vs Rust, release gnu (21:04:28) | 26 passed / 0 failed (bash size_kb=8 alloc, rust 7 apparent; size blanked in compare) |
| `real_parity`, static x86_64 musl (21:07:38) | 26 passed / 0 failed |
| `real_perf` VPS-sized tree (21:03) | 0 failed; rust size_kb 17538 for 17,958,053 B (= ceil); files.tar + SHA256SUMS byte-identical |
| `real_unit_live` VPS read-only (21:04) | 0 failed: live unit set = repo mirror, ELF sha = t273u artifact, last Rust run ok (size_kb=1), 0 Rust error lines |
| galion-tar tests | 9/0 |
| fmt / clippy -D warnings (galion-backup, all targets) | ok |
| x86_64 + aarch64 musl release build | ok (aarch64 via zig-cc stopgap for SQLite C, D3) |
| armv7-musleabihf + aarch64-android cargo check | ok (zig-cc stopgap) |
| scripts/no-python.sh | ok |

VPS 21:07 read-only: nerve `:8670/state` 200 (recent warn: galion-desk 1
restart in the last hour, after the desk-hermes cutover); galion-backup.timer
active, next run 2026-10-28 01:20 CET (will still log the old value: live ELF
is 98466ab, unchanged).

## Not done / owner

* NO live unit or binary change (row says so). The fix lands when the next
  galion-backup ELF is deployed (new drop-in pointing at a new sha).
* Nothing reads `size_kb` (offsite hook reads the file itself), so the
  change is log-only.
