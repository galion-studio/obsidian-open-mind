<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cortex-escalations-persist (wave-4 slot 431)

2026-10-28, Europe/Zagreb (CET +01:00). Branch `swarm/cortex-escalations-persist` off
`cortex-escalations-diag` 9ece3df4. Box only, loopback, ports 23010-23019 (only 23010 used).
VPS read-only. Nothing deployed, no live config change. Hermes research not used.

## Unit + engine tests

- `sink.rs` (6 new): `parse_esc_id_reads_both_shapes`, `restart_keeps_open_items_and_their_open_time`,
  `closed_and_acked_items_stay_closed_after_restart`, `torn_last_line_is_skipped_and_the_file_is_realigned`,
  `legacy_duplicate_ids_ack_only_the_latest_and_missing_file_is_empty`, `reload_respects_the_ring_cap`.
- `zagreb-time`: `to_unix_round_trips_from_unix_across_the_dst_switch` (2026-10-25 01:59:59 CEST / 02:00 CET).
- `tests/escalation_persist.rs` (5, real engine): restart keeps open + unique ids over 3 processes,
  autoclose off by default after restart, closed stay closed, torn last line, live 37-line file.
- Found + fixed while testing: legacy duplicate ids (`esc-3` x4 in the live file) lost their open
  time after the first of them closed, so 3 of 37 would never expire by age. Open times are now
  forgotten only when no open item has that id.

## GALION_REAL: `tests/real_escalation_persist.rs` (release cortex, port 23010)

`GALION_REAL=1 cargo test --release -p gaia-cortex --test real_escalation_persist -- --nocapture --test-threads 1`
Passed twice: 23:33:41 (glibc release), 23:38:37 (static x86_64 musl,
`GALION_CORTEX_BIN`, 1,123,104 B). Output of the musl run:

```
[431 2026-10-28T23:38:38+01:00] run 1 open 3 ["esc-1793227117-3", "esc-1793227117-6", "esc-1793227117-9"]
[431 2026-10-28T23:38:38+01:00] after SIGTERM restart: open 3 reloaded 3 epoch 1793227118 health.escalations_open 3
[431 2026-10-28T23:38:40+01:00] run 2 after ack + 1 new: ["esc-1793227117-6", "esc-1793227117-9", "esc-1793227118-6"]
[431 2026-10-28T23:38:40+01:00] SIGKILL; run-2 stderr: gaia-cortex: escalations reloaded: Reload { lines: 3, opened: 3, acked: 0, closed: 0, unmatched: 0, skipped: 0, repaired_newline: false, open: 3, max_epoch: Some(1793227117) }, id epoch 1793227118
[431 2026-10-28T23:38:40+01:00] torn line: open 3 skipped 1
[431 2026-10-28T23:38:41+01:00] final open 4 ids ["esc-1793227117-6", "esc-1793227117-9", "esc-1793227118-6", "esc-1793227120-3"] epochs {1793227117, 1793227118, 1793227120} log lines 7 (ack 1)
[431 2026-10-28T23:38:41+01:00] live file, default config: open 37; stderr: gaia-cortex: escalations reloaded: Reload { lines: 37, opened: 37, acked: 0, closed: 0, unmatched: 0, skipped: 0, repaired_newline: false, open: 37, max_epoch: None }, id epoch 1793227121
[431 2026-10-28T23:38:41+01:00] drop-in 93 env: open 0, closed_expired 37, close lines 37
[431 2026-10-28T23:38:41+01:00] PASS
```

What it shows: open survives SIGTERM and SIGKILL; an ack in process 2 of an item from process 1
sticks; after the 3 units recover (`unit.started`) nothing closes with the default config
(`closed_total` 0); a torn last line is skipped and the next record is readable after another
restart; ids unique over 5 processes; every cortex start 100-101 ms (with reload).

Two first-run failures (test bugs, fixed): 3 failures in one burst fold into one R0 stack-wide
escalation (now one burst per unit); and "epoch > previous process epoch" is too strong when that
process wrote no escalation (the guarantee is "past every epoch in the log"; documented).

## Live VPS (read-only, 2026-10-28 23:34 CET)

- `GET 127.0.0.1:8674/health`: `escalations_open` 0, `incidents_open` 2, mode live, nerve true;
  gaia-cortex pid 1249123 `/opt/galion/rust/bin/gaia-cortex-58a12e6-llm`, active since 17:37:56 CET.
- `GET /escalations`: open 0, total 0.
- `/var/lib/gaia-cortex/escalations.jsonl`: 37 lines, 36,096 B, mtime 06:29 CET, ends in `\n`;
  37 records (34 ids, `esc-3` x4), 0 ack, 0 close; sha256 prefix 33c21973a75e0e8b = the fixture.
- Nerve `127.0.0.1:8670/healthz` ok.

## Gates

fmt ok; clippy `--workspace --all-targets -D warnings` (excl. openmind-sandbox, cgroup-detect) ok;
clippy gaia-cortex `--features llm-advisor` ok; workspace tests (excl. openmind-sandbox,
cgroup-detect, hermes-tools: sandbox_vps_only) 247 suites, 1241 passed, 0 failed, 4 ignored;
no-python ok; spdx 441 scanned / 0 missing; release `--locked` x86_64 + aarch64 musl builds ok;
armv7-musleabihf, aarch64-android, armv7-androideabi `cargo check` (gaia-cortex, zagreb-time) ok.
No new dependency (zagreb-time already used chrono).
