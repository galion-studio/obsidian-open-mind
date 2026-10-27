<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Nerve flood zero-baseline / first-bucket skip (wave-4 slot 252)

Run: Europe/Zagreb (CET +01:00). Box only, NO deploy. Ports 21210-21219.
Branch `swarm/nerve-flood-baseline` on nerve-event-driven `bb287cc`.

## Bug (slot 233 / live)

Event-mode nerve (`heartbeat_s` 900) adds the cloudflared `total_requests`
delta since the previous cycle into **one** 10 s access bucket
(`engine.rs` → `access::Stream::record_count`). The detector then computes
`rps = count/60`. A quiet 0.6–0.8 req/s edge looked like 10+ req/s against
baseline 0.00 (167/168 buckets empty) and went **CRITICAL**
(`ddos:edge-cloudflared`). The 30 s shadow nerve saw no DDoS.

## Fix

In `access.rs` `Stream::evaluate`, for **counter-only** streams
(`has_ips == false`):

1. **Zero-baseline**: if baseline median is 0, never emit `flood` (cap at
   `elevated` / warn).
2. **First-bucket skip**: if the baseline window has no non-empty bucket
   (the dump is the first data after a long gap), same cap.
3. **≥3× real baseline**: even with a non-zero baseline, `flood` also
   requires `rps >= 3 × baseline`.

IP-bearing streams (access logs, `nerve-api`) are unchanged so a real API
flood still CRITICAL.

This is the P-233-1 *partial* (detector guard). Spreading the counter
delta across one bucket per 10 s of the gap is left for a follow-up.

## Unit tests (`access.rs`)

- `counter_only_zero_baseline_dump_is_not_flood` — 612 into one bucket → not flood
- `counter_only_first_bucket_after_gap_is_not_flood` — silence then dump → not flood
- `counter_only_real_baseline_triple_still_floods` — 2 req/s baseline, 50 req/s → flood
- `ip_bearing_flood_without_baseline_still_fires` — nerve-api style still flood

## Real test

```
export PATH=/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=/workspace/swarm/target/nerve-flood-baseline
cd /workspace/swarm/wt/nerve-flood-baseline
GALION_REAL=1 nice -n 10 cargo test --release -p openmind-nerve \
  --test real_flood_baseline -- --nocapture --test-threads=1
```

- `real_900s_style_counter_dump_is_not_critical` — event mode, heartbeat 60 s,
  mock metrics on :21211, +612 requests between cycles, assert stream level
  ≠ `flood` and no CRITICAL `ddos:edge-cloudflared` incident. Nerve :21210.
- `real_ip_bearing_api_flood_still_critical` — 800 GETs to nerve-api on
  :21212 still reach `flood`.

## Not done / owner

- No VPS deploy (task: box only). Live nerve stays `bb287cc` until approved.
- Full cycle-gap window math (spread delta across buckets; "errors in 5 min"
  real window) remains P-233-1 follow-up.

## Evidence (this run)

Box run at 2026-10-27 17:44:23 +0100 Europe/Zagreb (CET +01:00):

```
GALION_REAL=1 cargo test --release -p openmind-nerve --test real_flood_baseline -- --nocapture --test-threads=1
test real_900s_style_counter_dump_is_not_critical ... ok
test real_ip_bearing_api_flood_still_critical ... ok
test result: ok. 2 passed; 0 failed; finished in ~74 s
```

Unit (`cargo test -p openmind-nerve --lib access::tests`): 9/9 including the four new cases.
Lib: 102/0. fmt ok. clippy -D warnings ok. no-python ok. No new crates.
VPS not touched. No deploy.
