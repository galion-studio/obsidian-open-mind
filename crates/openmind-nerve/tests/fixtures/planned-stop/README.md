<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# planned-stop fixtures (slot 310)

Read-only captures from the live VPS journal (`journalctl _PID=1 UNIT=... -o json --output-fields=...`),
2026-10-27 ~21:55-22:00 CET (+01:00). Boot ID, cursor, monotonic time and seqnum ID removed; no secrets
(systemd lifecycle lines only).

- `vps-console-restarts-20261027.jsonl`: console.service 17:25:04, 17:25:06 and 17:28:29 +01:00 restarts
  (operator, cortex dec-874, cutover). Each is `Stopping` (JOB_TYPE=stop) -> exit 143 -> `Failed 'exit-code'`.
- `vps-lifecycle-7d-20261027.jsonl`: 7 days of pid-1 lifecycle lines of hermes-rs.service, gaia-cortex.service
  and cortex-sbx-crash-20261021-183907.service (real crash loops with no stop job = crash controls).
