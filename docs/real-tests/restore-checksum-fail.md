<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: restore-checksum-fail (wave-4 slot 361)

All times Europe/Zagreb CET (+01:00). Branch `swarm/restore-checksum-fail`
off `swarm/restore-test-rust` bc741f2a; code commit dd89bea4. **No live
change on the VPS** (throwaway /var/tmp folder only, removed). No ports used
(nothing serves HTTP; range 22310-22319 unused).

## Change
* `restore::check_sums` is strict and returns a `SumReport`. ANY of these is
  exit 1 with one clear stderr line naming the archive and ending
  `this backup is NOT verified, do not restore from it`: digest mismatch,
  missing/unreadable listed file, malformed SHA256SUMS line, listed name that
  is absolute / has `..` / is or goes through a symlink, archive file not
  listed (or not a regular file). Stdout for mismatched/unreadable files is
  still coreutils' `NAME: FAILED` / `NAME: FAILED open or read`.
* **F-361-1 (bug in slot-326 port, found here):** a flipped bit in the gzip
  CRC32 trailer PASSED (tar reader stops at end-of-archive, the gzip trailer
  was never read). Now the gzip stream is drained after extraction so CRC32
  and ISIZE are always verified. First real run caught it:
  `G1 gzip CRC32 trailer bit flipped alias=false: exit left: Some(0) right: Some(1)` (05:14:39).
* Multi-call: argv[0] `galion-restore-test` / `galion-restore-test.sh` = `restore-test`
  (the staged swap is a symlink; see docs/ops/restore-checksum-swap.md, D-361-1).
* Staged unit `galion-restore-test.service` now runs `galion-backup-dd89bea4`.

## Box: `real_checksum_fail` (2026-10-28 05:15 CET)
`GALION_REAL=1 cargo test --release -p galion-backup --test real_checksum_fail -- --nocapture --test-threads=1`
Real archive from the real `galion-backup run` over the fake VPS tree
(placeholder secrets). Each tampered copy is run by the Rust tool directly
AND via a `galion-restore-test.sh` symlink (both must exit 1, no `checksums ok`
/ `restore test ok`, temp dir gone), and by the live .sh (verbatim, sha
checked) for comparison.
```
PASS T0 clean archive: rust ok direct + via galion-restore-test.sh symlink, stdout identical to .sh (11 lines)
PASS T1 one bit flipped in db/memory-vectors.db: rust exit 1 (direct + alias); sh exit 0 + `restore test ok` (LET THROUGH, F-326-1)
PASS T2 one bit flipped in files/files.tar: rust exit 1 (direct + alias); sh exit Some(2) (caught)
PASS T3 db/MAP truncated to 0 bytes: ... LET THROUGH
PASS T4 listed file deleted (db/MAP): ... LET THROUGH
PASS T5 SHA256SUMS line for db/MAP removed: ... LET THROUGH
PASS T6 unlisted file injected (db/notes.txt): ... LET THROUGH
PASS T7 malformed line appended to SHA256SUMS: ... LET THROUGH
PASS T8 one digest digit changed in SHA256SUMS: ... LET THROUGH
PASS T9 absolute name with valid digest appended: ... LET THROUGH
PASS T10 db/MAP swapped for symlink to outside copy (same digest): ... LET THROUGH
PASS T11 three tampers at once: ... LET THROUGH
PASS G1 gzip CRC32 trailer bit flipped: rust exit 1 (direct + alias); sh exit Some(2) (caught)
PASS G2 gzip ISIZE trailer bit flipped: rust exit 1; sh exit Some(2) (caught)
PASS G3 tar header checksum broken: rust exit 1; sh exit Some(2) (caught)
SUMMARY: 14 tamper cases; rust failed hard on 14/14; live .sh let 10/14 through with exit 0 + `restore test ok`
```
Example messages:
```
galion-backup: restore-test: <tmp>/tampered/galion-t1.tar.gz: checksum check FAILED: 1 mismatched [./db/memory-vectors.db], 0 unreadable, 0 malformed line(s), 0 unsafe name(s), 0 unlisted (5 of 6 listed files ok); this backup is NOT verified, do not restore from it
galion-backup: restore-test: <tmp>/tampered/galion-t11.tar.gz: checksum check FAILED: 1 mismatched [./db/memory-vectors.db], 1 unreadable [./db/MAP], 0 malformed line(s), 0 unsafe name(s), 1 unlisted [./extra.bin] (4 of 6 listed files ok); this backup is NOT verified, do not restore from it
galion-backup: restore-test: <tmp>/tampered/galion-g1.tar.gz: gzip check FAILED: gzip: invalid compressed data--crc error (stored e6c0e4db, computed e6c1e4db); this backup is NOT verified, do not restore from it
galion-backup: restore-test: <tmp>/tampered/galion-g3.tar.gz: tar: tar header checksum mismatch
```
Regression: `real_restore_test` (slot 326) re-run 05:15:33-05:15:44: S1-S11,
hostile dotdot/symlink-escape, SIGTERM 30/80/150 ms all PASS (4/4); S4/S5
messages now the strict form.

## VPS: `real_vps_checksum_fail` (2026-10-28 05:16:26-05:17:54 CET)
`GALION_REAL=1 GALION_VPS_SSH=slavik-vps GALION_RESTORE_VPS_BIN=<x86_64 musl, sha256 30f58ba4…51c6>`
Throwaway 0700 `/var/tmp/restore-test-361-20261028-051626+0100`; live .sh
sha re-checked (`26439775…`) before use. Names/exit codes only printed.

**Part 1, all 12 REAL archives** (galion-20261025-131808 … galion-20261028-012029):
**12/12 sh_rc=0, 12/12 rs_rc=0, 12/12 al_rc=0 (symlink alias), 12/12
STDOUT_IDENTICAL, t_left=0** -> the strict checks give no false positive on
real bash-made or Rust-made backups.

**Part 2, tampered COPIES of the newest real archive (galion-20261028-012029+0100)**:
```
tamper=files_tar_byte    sh_rc=2 sh_says_ok=0 rs_rc=1 t_left=0
tamper=sums_line_dropped sh_rc=0 sh_says_ok=1 rs_rc=1 t_left=0   (.sh LET THROUGH)
tamper=unlisted_file     sh_rc=0 sh_says_ok=1 rs_rc=1 t_left=0   (.sh LET THROUGH)
tamper=malformed_line    sh_rc=0 sh_says_ok=1 rs_rc=1 t_left=0   (.sh LET THROUGH)
tamper=gzip_crc          sh_rc=2             rs_rc=1 t_left=0
  rs_msg=... galion-tamper-sums_line_dropped.tar.gz: checksum check FAILED: 0 mismatched, 0 unreadable, 0 malformed line(s), 0 unsafe name(s), 1 unlisted [./db/MAP] (5 of 5 listed files ok); this backup is NOT verified, do not restore from it
  rs_msg=... g.tar.gz: gzip check FAILED: gzip: invalid compressed data--crc error (stored bbfffe8e, computed bb0efe8e); ...
cleaned=/var/tmp/restore-test-361-20261028-051626+0100
tmp_left=0
```
Re-checked 05:18:01: no `/var/tmp/restore-test-361-*` left, live .sh sha
unchanged `264397750a310832…`, `systemctl list-unit-files '*restore*'` 0
(nothing installed).

Nerve (GET 127.0.0.1:8670/state, 05:18:01 CET): HTTP 200, 17 `healthy`
entries; the only backup mention is an incident `unit:galion-backup.service`
"protected unit" (warn, 01:20:36, the scheduled backup run). Still **no
restore-test component** (slot 311/326 observability gap); installing the
staged timer (D-361-1 step 4) would surface failures as failed-unit
incidents. Cortex: not probed (not live as a separate health source); nerve
used as the real-time view.

## Gates (2026-10-28 ~05:20 CET)
see CALL-RECORD DONE line for final numbers.
