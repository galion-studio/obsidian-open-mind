<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# nerve-cycle-latency (wave-4 slot 344, N-332-1)

Branch `swarm/nerve-cycle-latency` off nerve-merge-6 `9baa450c`. Box + VPS
read-only, 2026-10-28 03:06-03:55 +01:00 (Europe/Zagreb). NO deploy, no live
change.

## 1. Finding N-332-1 on the live VPS (read-only, 03:25-03:27 +01:00)

Slot 332 saw the live nerve `cycle` move 36 -> 37 in ~8 min although
`interval_s` is 30.

| probe | value |
|---|---|
| unit | `galion-nerve.service`, ExecStart (drop-in) `/opt/galion/rust/bin/openmind-nerve-9d1a4f3 serve --mode event`, env `NERVE_MODE=event NERVE_INTERVAL=30 NERVE_COMPONENTS=/etc/galion-nerve/components-live.json`, active since 2026-10-27 18:15:00 +01:00, NRestarts 0 |
| `/health` 03:25:38 | cycle 45, score 64, system degraded |
| `/api/v1/state` | ts 03:15:01, cycle 45, **cycle_ms 1011**, interval_s 30; galion-nerve component extra.cycle_ms 973 |
| per-component HTTP | 9 probed components all `latency_ms` 0 (memory, hermes-rs, desk, tuner, one, one-mouth 503, console 302, galion-health) |
| `/api/v1/stats` | mode event, **heartbeat_s 900**, cycle_debounce_s 5, cycle_min_gap_s 60, engine_cycles_total 45, engine_cycles_last_hour 4, last_wake 03:15:01 |
| load average 03:25 | 18.0 / 15.5 / 15.5 |

Root cause: **not a slow probe**. The live unit runs event mode, where the
full engine cycle runs at start, on the 900 s heartbeat and debounced after
stack events (`evt/runtime.rs`, by design since the event-mode work), so
45 cycles in 9 h (~12 min/cycle, 4/h on quiet hours) is expected; a cycle
itself takes ~1 s. `interval_s` 30 only drives the polling mode, which made
the state misleading. Fix for that: `/api/v1/state` now has `cadence`
("event: ... interval_s unused" vs "poll: full cycle every interval_s").

## 2. The real latency hazard found while reproducing

`collect()` probed every service component **sequentially**, and
`util::http` set `timeout` as the per-`read()` timeout, not a total budget:

- k components that accept and never answer cost k x 3 s (+2 s per deep
  route) per cycle;
- a component that drips one byte per read timeout (slowloris-style, or a
  wedged server flushing slowly) stalls the cycle **without bound** (up to the
  512 KiB cap); in event mode that blocks the whole event loop.

Fix (this branch):

1. `util::http`: `timeout` is the total budget for connect + write + read
   (read timeout re-armed to the remaining budget). Budget spent before the
   header block ended -> error `timeout after N ms (incomplete header, B)`;
   complete header with a still-streaming body keeps the old behaviour
   (status from header). No reply -> `timeout after N ms (no reply)`.
2. `collect`: HTTP health (+deep) probes are collected as jobs and run after
   the systemd/journal pass on up to `NERVE_PROBE_PARALLEL` threads (default
   8, clamp 1..16, the caller is one worker, failed spawn only lowers
   parallelism, 256 KiB stacks; results in component order). Live unit has
   TasksMax=64 / MemoryMax=64M: 7 extra short-lived threads fit.
3. `/api/v1/state`: `cadence` + `probe_parallel`.
4. README "What one cycle does": cadence + probe bounds.

No new deps, std only.

## 3. Real test `tests/real_cycle_latency.rs` (GALION_REAL=1, box, 03:41 +01:00)

Real `openmind-nerve serve` (poll mode, interval 5 s), 9 components: 6
accept-and-hold (:22141), 1 slow drip (status line, then one header byte per
200 ms forever, :22142), 1 healthy with deep route (:22143), 1 dead port
(:22144). Ports 22140-22149.

| leg | result |
|---|---|
| A default parallel 8 (nerve :22140) | cycles 1-4 cycle_ms **3034 / 3152 / 3269 / 3136** (limit one 3 s budget + slack 4500, and interval+timeout 8000); cycles every ~5 s; hang-1..6 failing (http null), drip failing `health probe down (timeout after 3003 ms (incomplete header, 40 B))` (never a false 200), ok healthy + deep ok, dead failing; `cadence` poll, `probe_parallel` 8 |
| B control `NERVE_PROBE_PARALLEL=1` (:22145) | first cycle **21625 ms** (7 x 3 s sequential walk, now bounded) |
| negative control: same test on base 9baa450c src | **FAILED: nerve :22140 reached only [] in 60s** - the drip component stalls the first cycle forever |

## 4. Gates (box, 03:30-03:55 +01:00)

- `cargo fmt --all --check` ok; `clippy -p openmind-nerve --all-targets -D warnings` ok.
- `cargo test -p openmind-nerve`: lib 124/0 (incl. new util `http_hung_peer_costs_one_timeout`,
  `http_slow_drip_is_bounded_by_total_deadline`, `http_full_header_then_stall_keeps_status`,
  `http_normal_reply_and_refused`, collect `http_jobs_run_bounded_parallel_in_order`), all
  integration tests green.
- GALION_REAL=1 (REAL_NERVE_PORT_BASE=22140, test-threads 1): real_cycle_latency 1/1, real_box 1/1,
  real_console_deep 1/1, real_consumer_compat 2/2, real_flood_baseline 2/2, real_flood_cycle_gap 2/2,
  real_own_services 3/3, real_planned_stop 1/1, real_publish_recoveries 2/2, real_staged_console 2/2,
  real_wire_gaps 3/3, real_wiring 1/1, staged_components 2/2, writer 12/12.
- Pre-existing, **same failure on base 9baa450c** (not this change): real_cortex_sensor
  `real_cortex_sensors_on_the_box` golden mismatch at line 138; real_dst_clock
  `real_vps_state_replayed_through_the_fold...` ("findings differ at the fold", line 386, with
  NERVE_DST_SSH=slavik-vps read-only `/state` x3).
- Release x86_64 musl solo `-p openmind-nerve`: 1,483,552 B vs base 1,479,456 B (+4,096 B);
  `size-budget.sh --check-dir` ok (stripped). aarch64 musl `cargo check` ok.
- `no-python.sh` ok; `spdx-scan.sh` 560 scanned, 0 missing.

## Owner notes

- D-344-1: N-332-1 closed as "by design" (event-mode cadence). If 4 full
  cycles/h is too slow for health verdicts, lower `NERVE_EVT_HEARTBEAT`
  (e.g. 300) at the next nerve deploy - owner choice, not changed here.
- D-344-2: ship with the next nerve deploy candidate (fold onto nerve-merge-6
  9baa450c line); default `NERVE_PROBE_PARALLEL` 8 needs no unit change.
