<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 309: cortex R3 skips planned stops (slot 271 F3, D-271-2 option A)

Branch `swarm/cortex-r3-planned-stop`, base `cortex-live-merge-3` 73583abf
(= live `/opt/galion/rust/bin/gaia-cortex-73583ab-llm`). **NO deploy.**
All times Europe/Zagreb (CET, +01:00).

## Problem (slot 271)

`systemctl restart console` -> SIGTERM -> srvx wrapper exits 143 ->
systemd `Result=exit-code` -> nerve `unit.exit` (143) + `unit.failed`
(exit-code) -> cortex `R3-known-crash` -> restart (live dec-874,
2026-10-27 17:25:06 +01:00). An operator/cutover restart was treated as a crash.

## Fix

New rule `R5b-planned-stop` (Info, no action, no incident) evaluated in
`Rules::decide` before the flap book, so planned stops never count toward
R2. Planned when, for a unit subject with failures/exits and **no crash
evidence** (OOM, start-limit, restart-scheduled, core dump, watchdog,
signal result, killed by KILL/other, any non-143/130 exit):

* a planned marker (systemd `job_type` stop/restart on unit.stopped, nerve
  `data.planned=true`, or kind `unit.planned_stop`) covers every failure, or
* every failure is a stop-signal exit (exited 143/130, killed TERM/INT) and
  either a marker was seen for the unit within `PLANNED_HOLD_S`=10 s or the
  unit is active again in the same burst.

`engine::plan` keeps planned subjects Info/non-hard. `repeat::arm` also
watches stop markers and exit evidence so nerve dedupe (600 s fold) cannot
hide them.

## Evidence (read-only VPS probes)

* `journalctl _PID=1 -u console.service` 2026-10-27 17:24:50–17:28:40
  +01:00: 3 restarts x (Stopping, exit 143, failed exit-code, Stopped
  job_type=restart, Consumed, Started) -> fixture
  `crates/gaia-cortex/tests/fixtures/console-planned-restart-20261027.jsonl`
  (cursor/boot/monotonic fields stripped).
* 7-day `_PID=1` export (2026-10-20 → 2026-10-27 ~22:05 +01:00) of every
  unit that logged "Failed with result": 217 lines, 45 process exits
  (console 143 x4, hermes-rs exit 2 x16, gaia-cortex exit 2 x4, openmind-queue
  killed 9 x1, sandbox/test units exit 1/3/101) -> fixture
  `vps-failing-units-7d-20261027.jsonl`.

## Real tests

`GALION_REAL=1 cargo test -p gaia-cortex --test real_r3_planned_stop`
(release binaries on the box, nerve 21790, cortex 21791, dead 21799; live
nerve 9d1a4f3 built from source, FIFO journal replay, dedupe 600; cortex
coalesce 1500, repeat poll 2, live limits). Passed in 86.6 s, 22:03–22:04 +01:00.

| phase | cortex | verdicts | restart requests |
|---|---|---|---|
| BEFORE (live console replay) | base 73583ab | R3-known-crash, R3-known-crash, R2-circuit-open | 2 |
| AFTER (same replay) | slot 309 | 3x R5b-planned-stop | 0 (circuit closed) |
| crash control (exit 1) | slot 309 | R3-known-crash | 1 |
| kill -9 control | slot 309 | R3-known-crash | 1 |
| mixed (planned restart then folded crash) | slot 309 | R5b, R9-process-exit, R3-known-crash | 1 |

`cargo test -p gaia-cortex --test r3_week_replay` (7-day real VPS data,
deterministic): 47 failure bursts; **4 planned = exactly the 4 console 143
restarts**; hermes-rs exit-2 crashes stay R3 / R2-circuit-open; gaia-cortex
stays R1-protected; all unknown/sandbox units stay R4. No crash planned.

## Checks

* `cargo fmt --all --check` ok; clippy `-D warnings` clean (gaia-cortex
  all-targets, workspace minus openmind-sandbox / cgroup-detect / hermes-tools).
* gaia-cortex: lib 40 (11 new), pipeline 24, incident 11, flap 7,
  io_unlock 6, week replay 1 — all green.
* workspace `--no-fail-fast`: 1195 passed, 1 failed, 4 ignored. The failure
  `openmind-nerve replay_corpus every_saved_capture_is_in_the_corpus`
  (missing live:replay200/207) is box-environment and fails identically on
  base 73583ab — pre-existing, not slot 309.
* `scripts/no-python.sh` ok; spdx-scan missing 0; no new dependencies.
* Release: x86_64-musl 1,094,432 B static-pie; aarch64-musl 927,168 B
  (linker rust-lld); `cargo check` aarch64/armv7 android + armv7-musleabihf ok.

## Open items / owner decisions

1. F3 "query ActiveState via nerve, drop if started < 10 s ago" needs nerve
   I/O; replaced by the 10 s planned hold + same-burst active-again.
2. Split burst where `unit.failed` (143) arrives before the job line, with
   no Stopping event, still falls to R3 — slot 310 F2 (map Stopping
   `de5b426a` as `unit.planned_stop` / `data.planned`) closes it; this rule
   already honours both.
3. Live cortex config says `"actions":"dry-run"` while dec-874 was reported
   executed — observation only, not changed.
4. Deploy: owner decision; not done here.
