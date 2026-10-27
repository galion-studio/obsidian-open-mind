<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: swarm/nerve-planned-stop-tag (wave-4 slot 310, F2 of slot 271)

Base: `swarm/nerve-deploy-t273u` 9d1a4f3 (the live nerve source). **No deploy.**
Ports 21800 (new build) and 21805 (base control). Box only; the VPS was read
only (`journalctl -o json`, `curl 127.0.0.1:8670/state`).

## Problem (slot 271 F2)

`console.service` (srvx) exits 143 on SIGTERM, so every planned restart
(operator, cortex R3, cutover) logs `Main process exited, status=143` +
`Failed with result 'exit-code'`. The live nerve turned these into
`unit.exit` / `unit.failed` **high**, the cortex R3 restarted again, and
`/state` counted the restarts (`restarts_1h`), so the console went
degrading / failing-soon after a normal cutover.

## Fix

* `evt/journal.rs` `PlannedStops`: subscribes to the systemd "Stopping"
  MESSAGE_ID (`de5b426a…`, `_PID` 1 only), emits `unit.stopping` (info) and
  remembers `(INVOCATION_ID, t)` per unit (TTL 300 s, closed at Started). The
  `unit.exit` / `unit.failed` of the same unit + invocation becomes
  `unit.planned_stop` (info for clean status 0/143/130/15/TERM/INT, low for
  unclean, medium for other results such as timeout), `data.planned = true`,
  `planned_kind`, `stop_job`, `stopping_t`. A different invocation (crash after
  the restart) stays `unit.failed` high.
* `state.rs` `push_planned`: a restart whose window `(prev.t - 2 s, t]` holds a
  stop job goes to `planned_restarts_1h` instead of `restarts_1h`; one stop job
  explains one restart (multiset match). **Crash wins**: if the hub saw an
  unplanned failure (`unit.failed/exit/spawn_failed/start_failed/start_limit`,
  `oom.kill`, above info) of the unit in the same window, nothing is planned.
* `evt/mod.rs` / `runtime.rs` / `engine.rs`: hub keeps `planned_stops` and
  `unit_crashes` per unit (1 h, 64 max), the engine passes them to the state.
* `docs/nerve-event-contract.md`: kinds `unit.stopping`, `unit.planned_stop`.

Crash-wins was found by this real test (2026-10-27 22:04 +01:00): the real
hermes-rs lines of 19:52 CET hold an operator restart (stop job, clean
"Deactivated") and then the new invocation crashing with exit 2 in the same
second; the first version counted that restart as planned.

## Real test (`GALION_REAL=1`, release binary, `serve --mode event`)

`crates/openmind-nerve/tests/real_planned_stop.rs`. Journal = real VPS lines
(`tests/fixtures/planned-stop/`) through `NERVE_EVT_JOURNALCTL=replay:<fifo>`,
timestamps moved to now; `systemctl` stand-in bumps MainPID per restart.
`NERVE_EVT_DEDUPE=1`, `NERVE_EVT_CYCLE_MIN_GAP=5` (the default 600 s dedupe
folds the second identical `unit.started` key, so no event cycle runs until
the heartbeat).

Run 2026-10-27 22:08:01 → 22:08:45 +01:00, 1 passed:

| | new (21800) | base 9d1a4f3 (21805) |
|---|---|---|
| console `unit.failed` high | **0** | 3 |
| console `unit.exit` high | **0** | 3 |
| console `unit.planned_stop` info | 6 | 0 |
| console `unit.stopping` | 3 | 0 |
| console `/state` restarts_1h | **0** | 3 |
| console planned_restarts_1h | 3 | (field absent) |
| console status | **healthy** | failing-soon ("3 restarts in the last hour") |
| console escalations | 0 | 0 |
| hermes-rs `unit.failed` high | 1 | 1 |
| hermes-rs `unit.planned_stop` / `unit.stopping` | 0 / 1 | 0 / 0 |
| hermes-rs restarts_1h / planned | 1 / **0** (crash wins) | 1 / - |
| hermes-rs status | degrading | degrading |

Earlier runs: 22:03:48 +01:00 failed (pid 102 never seen: dedupe 600 s, fixed in
the test env); 22:04:53 +01:00 failed (hermes-rs planned_restarts_1h 1 → crash
wins fix); 22:07:07 +01:00 new only, passed.

Unit tests (journal.rs, real VPS fixtures): `vps_console_restarts_are_planned`
(0 failed/exit, 3 stopping, 6 planned_stop info, 3 started),
`vps_7d_crash_loops_stay_failures` (7 days of hermes-rs / gaia-cortex /
cortex-sbx-crash: hermes-rs 16 failed high, 0 planned; gaia-cortex 4 failed
high, 0 planned; sandbox 4 failed low), `planned_stop_edge_cases`;
state.rs `planned_restarts_do_not_degrade` (incl. crash wins).

## Checks (box, 2026-10-27 22:09–22:13 +01:00)

* `cargo fmt --all -- --check` ok; `clippy -p openmind-nerve --all-targets -D warnings` ok.
* `cargo test -p openmind-nerve --no-fail-fast`: lib 117/117, consumer_compat 11,
  dst_clock_replay 5, evt_mode 3, writer 12, real_* skipped without GALION_REAL.
  **Pre-existing failure**: `cortex_replay::replay_matches_the_pre_trait_build`
  (baseline_buckets 168 vs 0) fails the same way on base 9d1a4f3; not from this change.
* Release musl x86_64 (1 471 264 B) and aarch64 (rust-lld, 1 218 744 B) build;
  `cargo check` aarch64-linux-android, armv7-unknown-linux-musleabihf,
  armv7-linux-androideabi ok (armv7 gnueabihf std is not installed on the box).
* `scripts/no-python.sh` ok, `scripts/self-check.sh` ok.

## Live VPS (read only, 2026-10-27 22:08:51 +01:00)

Live nerve 9d1a4f3 `/state`: console healthy restarts_1h 0, hermes-rs healthy
restarts_1h 0 (the 17:25–17:28 restarts have aged out of the hour).

## For consumers (slot 309 cortex F3)

Planned stops now arrive as `unit.planned_stop` (+ `data.planned: true`) and
`unit.stopping`; the old cortex R3 (counts `unit.failed`) ignores them as is.
A unit-fault incident is no longer opened by a planned stop.
