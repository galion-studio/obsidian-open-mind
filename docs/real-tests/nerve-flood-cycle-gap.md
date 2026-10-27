<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Nerve flood cycle-gap bucket spread (wave-4 slot 278 / P-233-1 rest)

Run: Europe/Zagreb (CET +01:00). Box only, NO deploy. Ports 21480-21489.
Branch `swarm/nerve-flood-cycle-gap` on nerve-flood-baseline `5da6eed`.

## Bug (slot 233 / live)

Event-mode nerve (`heartbeat_s` 900) added the cloudflared `total_requests`
delta since the previous cycle into **one** 10 s access bucket. Detector
`rps = count/60` then saw ~10 req/s against baseline 0.00 → **CRITICAL**
`ddos:edge-cloudflared`. Real rate was 0.6–0.8 req/s. The same gap also made
"N errors in 5 min" count journal lines since the last cycle (~15 min).

Slot 252 (`swarm/nerve-flood-baseline`) added the detector guard
(zero-baseline / first-bucket skip / ≥3×). This slot finishes P-233-1.

## Fix

1. **`Stream::record_count_spread(prev_t, now, n, errors)`** (`access.rs`):
   spread the counter delta evenly across one 10 s bucket per 10 s of the
   gap. A 900 s / 612-request dump becomes ~6–7 requests per bucket; the
   last-minute window then shows ~0.7 req/s → `normal`.
2. **`Engine` tunnel path** (`engine.rs`): `tunnel_prev` keeps `(req, err, t)`
   and calls `record_count_spread` instead of `record_count`.
3. **Journal "errors in 5 min"** (`probe::journal_counts_since` +
   `collect.rs`): component `journal_errors` only counts lines with
   `t >= now - 300` (unknown `t == 0` still counted). Full scan window still
   feeds the cortex via `Collected.lines`.

Slot 252 guards remain as a safety net when the gap is still ≤ 60 s (all
spread buckets sit inside the evaluate minute).

IP-bearing streams (access logs, `nerve-api`) are unchanged.

## Unit / lib tests

- `counter_only_900s_cycle_gap_spread_is_normal` — 612 over 900 s → normal
- `counter_only_short_gap_spread_fills_minute` — 60 s gap still not flood
- `record_count_spread_noop_on_zero`
- `journal_counts_since_drops_older_than_window`
- `cycle_gap_900s_tunnel_delta_is_not_flood` (engine `cycle_with`)
- Slot 252 guards retained

## Real test

```
export PATH=/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=/workspace/swarm/target/nerve-flood-cycle-gap
cd /workspace/swarm/wt/nerve-flood-cycle-gap
GALION_REAL=1 nice -n 10 cargo test --release -p openmind-nerve \
  --test real_flood_cycle_gap -- --nocapture --test-threads=1
```

- `real_cycle_gap_counter_dump_is_not_critical` — event mode, heartbeat 60 s,
  mock metrics on :21481, +612 between cycles, assert stream level ≠ `flood`
  and no CRITICAL `ddos:edge-cloudflared`. Nerve :21480.
- `real_ip_bearing_api_flood_still_critical` — 800 GETs to nerve-api on
  :21482 still reach `flood`.

Full 900 s → `normal` is proven by the lib/engine unit tests above (real
binary cannot wait 900 s cheaply; heartbeat here is 60 s so the slot 252
cap still applies on the box run).

## Owner

- No VPS deploy (task: box only). Live nerve stays until approved (D-233-1).
- Redeploy of nerve is an owner decision.

## Evidence

Box run at 2026-10-27 17:55:41 +0100 Europe/Zagreb (CET +01:00):

```
GALION_REAL=1 cargo test --release -p openmind-nerve --test real_flood_cycle_gap -- --nocapture --test-threads=1
test real_cycle_gap_counter_dump_is_not_critical ... ok
test real_ip_bearing_api_flood_still_critical ... ok
test result: ok. 2 passed; 0 failed; finished in ~74 s
```

Lib (`cargo test -p openmind-nerve --lib`): 107/0 including cycle-gap spread, short-gap guard, journal_counts_since, engine cycle_with 900 s. fmt ok. clippy -D warnings ok. no-python ok. No new crates.
VPS not touched. No deploy.
