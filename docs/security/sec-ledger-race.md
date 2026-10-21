<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# S3.4 hades-gate ledger: single use under race, fail closed on short writes (slot 219)

Branch `swarm/sec-ledger-race`, base integration-5 `0d68f46`, 2026-10-21 (CEST +02:00).

## What was tested

| Claim | Test | Result |
|---|---|---|
| One token, many processes at the same instant: exactly one approval | `real_ledger_single_use_under_process_race`: 40 rounds x 12 `hades-gate check` processes (release binary), FIFO start barrier; round 0 races a ledger file that does not exist yet | 40/40 rounds exactly 1 approval, 440 `replayed` refusals, 480 ledger lines all well-formed, winners spread over all 12 spawn slots |
| Same across `serve` and `check` sharing one ledger | `real_ledger_single_use_under_mixed_serve_and_cli_race`: 25 rounds x (6 CLI + 3 HTTP to each of 2 `serve` processes) | 25/25 rounds exactly 1 approval; winners CLI, server A and server B all seen; `/health` 200, consumed 25, audit_failures_total 0 on both |
| A write cut short by the kernel never approves; the gate fails closed afterwards | `real_ledger_short_write_fails_closed` (`prlimit --fsize`) | no-room write: process killed by SIGXFSZ, nothing written, token passes later exactly once; torn write (40 bytes landed): no approval, every later check refused `code ledger`, stderr `ledger corrupt at line 4`, `status` exit 3 |

Stability: both race tests passed in 8 release runs (5 in a loop, 3 single runs), i.e. 320 process-race rounds and 200 mixed rounds, 6,240 contended checks, 0 double approvals.

The lock is `flock` on the ledger file (`File::lock`, std), taken before the
replay scan and held until the fsynced append; it is per open file
description, so it holds across processes and across threads.

Slot 125 (`swarm/hades-ledger-fail` 41e8084d + 84fef879, not in integration-5)
is carried as the first commit of this branch, applied unchanged from the
GitHub compare, so this branch builds on its detection (stderr `ledger
unavailable` / `ledger corrupt`, exit 3, `/health` 503) without duplicating
it. `/dev/full` and `chmod 0400` cases stay in slot 125's `real_ledger_fail`.

## Findings

1. **Single use holds under real multi-process races.** No double approval in
   6,240 contended checks.
2. **Torn tail glues the next record (new).** After a short write, refusal
   notes are still appended (best effort). Because the torn fragment has no
   newline, the first later note is glued onto it, so the corrupt line holds
   fragment + note. Fail-closed is not weakened (the merged line never parses,
   so everything is refused), but the repair has to delete a whole line and
   loses that refusal note, and the admin cannot simply cut back to the last
   newline (the test tried that first; it does not repair the ledger).
3. **Torn tail locks the gate until an admin repairs it.** Correct fail-closed
   behaviour, but a full disk for one write turns into a lasting HADES outage.
   Repairing by deleting the corrupt line is safe: a torn consume line was
   never followed by an approval (approval is only returned after the full
   line is written and fsynced). The test shows the torn token passes exactly
   once after repair and is `replayed` afterwards.
4. **File-size limit kills the gate silently.** Under `RLIMIT_FSIZE` (systemd
   `LimitFSIZE=`) the over-limit write raises SIGXFSZ and the process dies with
   no `ledger unavailable` line, so the cortex `hades` sensor would not see it
   (a `serve` unit would just restart). Fails closed (no approval), but not
   loud.

## Owner decisions (Slavik)

- **D-219-1** torn tail: (a) keep as is (manual repair); (b) on the next
  locked append, write a `\n` first when the file does not end in one, so the
  fragment stays its own line and no note is glued to it; (c) under the lock,
  truncate a torn tail back to the last newline and log one
  `ledger repaired` line. Recommended: (b) now (no semantics change), (c) only
  with your sign-off, since it auto-edits the security ledger.
- **D-219-2** ignore SIGXFSZ in `hades-gate` (it needs `libc` or a tiny
  `unsafe` call) so an over-limit write becomes a normal EFBIG error and the
  slot 125 detection path, or just never set `LimitFSIZE` on a hades unit.
  Recommended: document "no LimitFSIZE" in the unit until a hades unit exists.
- **D-219-3** real tmpfs / read-only-fs runs: blocked on the box (see below).
  Allow running them with `unshare -rm` + `nsenter` (unprivileged user
  namespace, throwaway 64 KiB tmpfs), or accept the `prlimit` short write plus
  slot 125's `/dev/full` and `chmod 0400` as the evidence.
- **D-219-4** the real tests exec `mkfifo` (GNU coreutils, GPL-3.0-or-later)
  and `prlimit` (util-linux, GPL-2.0-or-later) as external programs; nothing
  is linked or copied. Same question as D-A6 (GPL test tools) of slot 165.

## Not done (blocked)

The row asked for a real full tmpfs / loop device and a read-only remount.
The box has no small tmpfs or read-only mount the user can use, and the
unprivileged route (`unshare -rm` + `nsenter` + `mount -t tmpfs`) was refused
by the automatic safety review on 2026-10-21 22:57 CEST. No approval was
requested (task rule). The short write via `prlimit --fsize` exercises the
same kernel path (short `write()`, then an error on the next one) without a
mount; EROFS is not covered beyond slot 125's EACCES case.
