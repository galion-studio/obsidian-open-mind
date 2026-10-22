<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: hermes-gateway-rss (slot 244)

All times Europe/Zagreb (CEST +02:00), 2026-10-22. Box ports 21140-21147.
Logs are in /workspace/swarm/logs/hermes-gateway-rss/ on the box.

## Binaries

- before: integration-5 0d68f46, `cargo build --release -p hermes-gateway`
  -> /workspace/swarm/target/hgw-before-0d68f46
- after: 2aeeeb8 -> /workspace/swarm/target/hgw-after

Pitfall: when two worktrees share one `CARGO_TARGET_DIR`, cargo reported the
second build fresh in 0.14 s and gave a byte-identical binary. Touching the
changed sources forced a real rebuild, and `cmp` then showed the two
binaries differ. Check with `cmp` before trusting an A/B pair.

## Box soak: crates/hermes-gateway/tests/real_mem_growth.rs

```
GALION_REAL=1 HMG_BIN=.../hgw-before-0d68f46 HMG_EXPECT_GROWTH=1 HMG_SOAK_MIN=5 HMG_WARM_S=60 HMG_PORT_BASE=21140 cargo test --release -p hermes-gateway --test real_mem_growth -- --nocapture
-- mock :21140 start rss 4044 kB | warm 25788 kB | end 98924 kB | slope 18135.9 kB/min | last-first quarter +58911 kB | sent 8994 ok 6746 502 2247 429 1
-- stub :21141 start rss 4172 kB | warm 8772 kB | end 10748 kB | slope 413.7 kB/min | sent 8902 ok 8901 429 1
```

This run reproduces the leak. The test reported FAILED only because the
production-path gateway was still warming up over a 5 min window: the
flatness check failed for the stub gateway. The mock gateway's growth is
the expected positive control.

```
GALION_REAL=1 HMG_BIN=.../hgw-after HMG_SOAK_MIN=20 HMG_WARM_S=120 HMG_PORT_BASE=21145 cargo test --release -p hermes-gateway --test real_mem_growth -- --nocapture
-- mock :21145 start rss 4024 kB | warm 9916 kB | end 12232 kB thr 2 fd 7 | slope 83.5 kB/min | last-first quarter +1290 kB | sent 35982 ok 26987 502 8995 (502 = scripted mock-fail)
-- stub :21146 start rss 4184 kB | warm 10472 kB | end 12188 kB thr 2 fd 7 | slope 103.7 kB/min | last-first quarter +1326 kB | sent 35605 ok 35605
-- stub upstream hits 45628
test hermes_gateway_rss_flat_under_steady_load ... ok (1200.33 s, 01:58:20-02:18:20)
```

Threads and fds stayed flat (2 threads, 7 fds) in both gateways.

## Heap profile: crates/hermes-rs/tests/heap_growth.rs (HMG_HEAP=1, release)

```
heap: unbounded log (pre-198): 10210 B/chat retained (30160154 B over 3000 chats); clearing it frees 56450512 B
heap: default cap 256: slope 9.9 B/chat; log 224 of 5600 calls
heap: --mock (no log): slope 0.1 B/chat
heap: production-shaped (no log): slope -0.1 B/chat
heap: JSONL store index (no-sqlite builds): 485 B per new session - by design, owner decision
test heap_profile_chat_path ... ok (199 s)
```

## VPS: read-only probes only

These used only `/proc` status reads, `ps`, `ss`, `systemctl show` and
HTTP GETs. Nothing was written or restarted.

```
01:58:51  hermes-gateway-aa77864 PID 718450 VmRSS 1824 kB VmHWM 1904 kB thr 4 up 07:45:32; hermes-rs.service NRestarts=0; LISTEN 8642, 8420 (no 8652/8653)
02:09:11  PID 718450 VmRSS 1824 kB VmHWM 1904 kB; nerve journalctl -f child 946277 43684 kB (01:58 43348 kB)
02:19:32  PID 718450 VmRSS 1824 kB up 08:06:13; NRestarts=0; GET :8642/health 200; nerve /state score 61 grade D cycle 21
          cortex :8674/health ok mode live llm_enabled true hermes_handoff true incidents_open 0 escalations_open 3
```

## Gates

- `cargo fmt --all --check`: ok
- `cargo clippy -p hermes-rs -p hermes-gateway -p hermes-tools -p hermes-bench --all-targets -- -D warnings`: ok
- `cargo test -p hermes-rs -p hermes-gateway -p hermes-tools -p hermes-bench`: 18 suites, 79 passed, 0 failed
- `scripts/no-python.sh`: no-python: ok
