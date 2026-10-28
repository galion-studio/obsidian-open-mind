<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-heartbeat-tune (wave-4 slot 369, D-344-1 prep)

Branch `swarm/nerve-heartbeat-tune` off `swarm/nerve-cycle-latency` 1fff59e5.
Box only; VPS read-only. Ports 22390-22399. No deploy, no live change.

## 1. Live VPS (read-only, 06:21-06:57 +01:00)

- `galion-nerve` active; journal start lines `event mode: heartbeat=900s`
  (unit sets no `NERVE_EVT_HEARTBEAT` -> default).
- `/stats` 06:21:55: `config.heartbeat_s` 900, `engine_cycles_total` 57 in
  `uptime_s` 43615 (4.7/h incl. event cycles), `engine_cycles_last_hour` 4,
  `wakes_by.heartbeat` 48. 06:57:32: 60 cycles in 45752 s, last hour 5.
- `/api/v1/state` 06:31: `cycle_ms` 873-1011, score 44; process CPU 5 s
  total, RSS ~4.3 MB; load average 21.7 / 19.3 / 19.2 (8 CPUs) - 24.2 at 06:57.
- `/healthz` ok; cortex :8674 `/health` 200 (cortex live).
- Note: `wakes_by.journal` went 40 -> 98 and `http` 28 -> 52 between 06:21
  and 06:31 (other activity on the VPS, not this slot).
- Note: `/opt/galion/openmind` HEAD reads 202887d5 (read via `git rev-parse`;
  RULES.md still says 2f5a730).

## 2. Box real tests (`GALION_REAL=1`, release)

```
GALION_REAL=1 cargo test --release -p openmind-nerve --test real_heartbeat_tune -- --nocapture
```

### real_heartbeat_bounds (06:26:05-06:26:07 +01:00) - pass

```
369 bounds env=None          -> heartbeat_s 900 clamped_from null cycles/h 4.0  warn=false
369 bounds env=Some("300")   -> heartbeat_s 300 clamped_from null cycles/h 12.0 warn=false
369 bounds env=Some("60")    -> heartbeat_s 120 clamped_from 60   cycles/h 30.0 warn=true
369 bounds env=Some("5000")  -> heartbeat_s 900 clamped_from 5000 cycles/h 4.0  warn=true
369 bounds env=Some("120")   -> heartbeat_s 120 clamped_from null cycles/h 30.0 warn=false
369 bounds env="abc"  -> exit Some(2)
369 bounds env="300s" -> exit Some(2)
369 bounds env="-1"   -> exit Some(2)
```

Also checks stderr `event mode: heartbeat=<n>s` and the
`out of bounds 120..=900 s` warning exactly when clamped.

### real_heartbeat_cycles_per_hour (REAL_HB_WINDOW_S=1860, 06:26:10-06:57:16 +01:00) - pass

Three quiet event-mode nerves side by side (journal/pidfd off, one healthy
HTTP component, test polls `/stats` once a second), window opened 5 s after
the start cycle:

```
hb=120s window=1860s heartbeat_wakes=15 engine_cycles=15 -> 29.03 cycles/h (theory 30.00); gaps_s=[119, 120, 119, 120, 120, 120, 120, 120, 119, 120, 119, 120, 120, 120]; cpu 0.91 s in window = 1.761 cpu-s/h (0.061 cpu-s/cycle); rss 5440 KiB; cycle_ms 31
hb=300s window=1860s heartbeat_wakes=6 engine_cycles=6 -> 11.61 cycles/h (theory 12.00); gaps_s=[300, 299, 299, 299, 300]; cpu 0.65 s in window = 1.258 cpu-s/h (0.108 cpu-s/cycle); rss 5448 KiB; cycle_ms 33
hb=900s window=1860s heartbeat_wakes=2 engine_cycles=2 -> 3.87 cycles/h (theory 4.00); gaps_s=[899]; cpu 0.55 s in window = 1.064 cpu-s/h (0.275 cpu-s/cycle); rss 5432 KiB; cycle_ms 32
```

Reading: cadence = heartbeat to +-1 s; every quiet cycle is a heartbeat
wake. The cpu-s/cycle column includes the fixed idle cost (HTTP server
answering the 1 Hz polling), so the marginal cost is the slope:
300 vs 900 = (1.258-1.064)/(11.61-3.87) ≈ 0.025 cpu-s per extra cycle;
120 vs 900 ≈ 0.028. RSS is flat (~5.4 MB). The box has no systemd
(inventory step cheaper than live; live `cycle_ms` ~1 s vs box ~32 ms).

## 3. Gates (06:24-06:33 +01:00)

- `cargo fmt --all --check` ok.
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
- `cargo test -p openmind-nerve` 25 suites 190 pass / 0 fail (new unit tests
  `heartbeat_bounds_validated`, `heartbeat_cycles_per_hour_tradeoff`,
  `heartbeat_in_stats_config`; `config_defaults_and_guards` now expects 1 -> 120).
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --no-fail-fast`
  288 suites 1368 pass / 0 fail / 6 ignored.
- Release x86_64 musl `-p openmind-nerve` 1,483,552 B (same as base);
  `size-budget.sh --check-dir` ok. `cargo check` aarch64-musl, aarch64-android,
  armv7-musleabihf ok.
- `no-python.sh` ok; `spdx-scan.sh` 561 scanned, 0 missing. No new deps
  (Cargo.lock unchanged). No Hermes use.
- Sandbox tests not run (sandbox_vps_only; not touched by this change).

## 4. Test fixture changes

- `real_flood_baseline.rs`, `real_flood_cycle_gap.rs` keep their 60 s test
  heartbeat via `NERVE_EVT_HEARTBEAT_TEST_FLOOR=1` (their timing needs a
  heartbeat within ~75 s).
- `journal_native_rss.rs` 3600 -> 900 (3600 is now clamped; the test only
  needs "no heartbeat during the run").

## Owner decisions

- D-344-1 (open): keep 900 or set `NERVE_EVT_HEARTBEAT=300` at the next nerve
  deploy (see `docs/nerve-heartbeat-tune.md`). Default unchanged here.
- D-369-1: bounds 120..=900 with clamp+warn (chosen so a bad value never
  crash-loops the unit) vs hard reject (exit 2) for out-of-range values.
- D-369-2: ship with the next nerve deploy candidate (on top of
  nerve-cycle-latency 1fff59e5); no unit change needed for the default.
