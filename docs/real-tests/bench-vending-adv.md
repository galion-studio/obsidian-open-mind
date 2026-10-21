<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: bench-vending-adv (slot 133, ports 20030-20039)

Box: the shared 8-vCPU swarm box. Binaries were release builds from this branch (`swarm/bench-vending-adv`, base
`swarm/bench-hermes` 457ef74). All times are CEST (+02:00).

## 1. galion-vending binary, hard level (2026-10-21 05:02:06 to 05:02:22 +02:00)
`GALION_REAL=1 GALION_BENCH_WRITE_DOCS=1 cargo test --release -p galion-vending --test real_vending_hard -- --nocapture`
```
standard bench seed 42: "100.00"% hash "c4b11d2642a0b6be" (published, unchanged)
bench --level insane -> exit 2
hard bench seed 42: "100.00"% hash "21317eedd059a608" end cash 206257 c, incidents 51/51, stolen 45, pricing 360/360
hard bench seed 42, published standard policy: "94.00"%
hard seeds 1-200 via CLI (826 ms, 400 runs): scripted 200/200 at 100 % (min fill 9047 bp of 10000 = 90.47 %); standard policy mean 7572 bp, min 5000 bp, 82 capped at 50 %, 0 at 100 %
hard journal 502 lines (header format 2, level hard); resume after day 15 and replay == bench
pid 3759170 listening http://127.0.0.1:20030 seed=42 days=30 level=hard
GET /health -> {"ok":true,"crate":"galion-vending","version":"0.1.0","seed":42,"day":1,"finished":false,"level":"hard"}
GET /v1/bench?level=hard -> "100.00"% (== CLI)
day 1: all six prices at p* = base + cost/2 -> pricing 12/12
day 2: water at 1 c -> pricing 22/24, units_below_cost 8
POST /v1/reset?level=insane -> 400 {"ok":false,"error":"unknown level 'insane' (standard|hard)"}
POST /v1/reset (standard) -> status without level; GET /v1/bench == published standard ("c4b11d2642a0b6be")
POST /v1/shutdown -> exit exit status: 0, port 20030 free
wrote .../docs/bench/vending-hard.score.json at "2026-10-21T05:02:22+02:00"
test result: ok. 1 passed; 0 failed
```

## 2. Hard B3 through Hermes, real binaries (2026-10-21 05:02:30 to 05:02:58 +02:00)
`GALION_REAL=1 GALION_BENCH_WRITE_DOCS=1 cargo test --release -p hermes-bench --test real_hermes_vending_hard -- --nocapture`
(galion-vending served on 127.0.0.1:20035 with `--level hard`, mock LLM on 127.0.0.1:20036, bank URL 20037 passed but never started)
```
started galion-vending serve --level hard on 127.0.0.1:20035
vending-hard [scripted] via Hermes: 100%  30 shifts, 471 tool calls, total_bp 10000 (ref 10000), hash "21317eedd059a608" identical_to_reference=true replay_matches_live=true
vending-hard [mock-llm] via Hermes: 100%  30 shifts, 471 tool calls, total_bp 10000 (ref 10000), hash "21317eedd059a608" identical_to_reference=true replay_matches_live=true
mock-llm via Hermes (OpenAI wire on 127.0.0.1:20036): 501 model API calls, hash "21317eedd059a608"
stopped galion-vending (127.0.0.1:20035)
ports 20035/20036/20037 free again
test result: ok. 1 passed; 0 failed
```
At 05:02:58 +02:00, `ss -ltn` showed no listeners on 20030-20039.

## 3. Standard level unchanged, byte for byte (05:00 and again 05:04 +02:00)
The 457ef74 release binary (built before any change) and the final binary of this branch were compared:
- `bench --seed S --days 30` for S = 1..200: 0 differing files (all 200 at 10000 bp)
- `run --seed 42 --days 30 --log`: journal byte-identical (474 lines), stdout identical
- `schedule --seed 42`, `tools`, `bench --seed 42 --days 7/60/90`: identical

## 4. Sweeps on the final binary (05:04 +02:00)
- hard seeds 1-1000, scripted: 999 at 100 %. Seed 909 scored 9993 bp (fill 89.38 %).
- hard seeds 1-1000, `--policy standard`: mean 7703 bp, min 5000, max 9400, 380 capped, 0 at 100 %. 444 lies,
  all `broken_bulk_promise`.

## 5. Cross-arch determinism (05:03 +02:00)
x86_64-unknown-linux-musl (static-pie) vs aarch64-unknown-linux-musl (static, run under `qemu-aarch64`). The
sha256 of stdout matched for every case:
```
bench --seed 42                                   SAME [10000 c4b11d2642a0b6be]
bench --seed 42 --level hard                      SAME [10000 21317eedd059a608]
bench --seed 42 --level hard --policy standard    SAME [9400 3f84910e7aad63b6]
bench --seed 7 --level hard --days 90             SAME [10000 25f7b1ada2189e79]
```
`cargo check -p galion-vending -p hermes-bench` passes for armv7-unknown-linux-musleabihf and aarch64-linux-android.

## 6. VPS, read-only (05:03:07 to 05:03:17 +02:00)
Over ssh, GETs and `ss` only. No vending/bench service exists on the VPS, so there is no live B3 to compare with:
```
nerve 127.0.0.1:8670 /state -> 200, 13075 B: version 0.1.0, mode live, cycle 6537, cycle_ms 381, 15 components, 2 alerts, 0 probe_errors
shadow nerve 127.0.0.1:8672 /healthz -> 200; /state mode observe-only, cycle 53, has cortex/cortex_events/cortex_sensors
  (cortex keys bleeding/ddos/exposure/vulnerabilities; cortex.score null) -> the cortex runs only inside the shadow nerve, not live
no listeners in 19500-19699 / 20000-20099; no vending/bench systemd units
```
Note: the first probe saved the /state body to `/tmp/.n` on the VPS to measure its size and deleted it in the same
command. That was the only write, and it was outside every service directory.

Sandbox: not run anywhere (sandbox_vps_only). Local workspace tests exclude openmind-sandbox and cgroup-detect.

## 7. Restart re-verification (2026-10-21 05:39 to 05:45 +02:00, sole agent after the first agent stopped at 7fcb607)
Everything above was re-run from scratch on 7fcb607, and the Hermes real test was extended.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`: clean. `cargo test --workspace --exclude
  openmind-sandbox --exclude cgroup-detect`: **601 passed, 0 failed**.
- `real_vending_hard` (GALION_REAL=1, 05:39:50-05:39:52): same output as section 1 (pid on 127.0.0.1:20030, standard
  `c4b11d2642a0b6be`, hard `21317eedd059a608`, 200/200, standard policy mean 7572 bp, 82 capped).
- **Fresh baseline**: 457ef74 was checked out into a separate worktree and built release (`-p galion-vending`).
  Compared with this branch built the same way: `bench --seed S` for **S = 1..1000: 0 differing**, `tools` and
  `schedule --seed 42` identical, `run --seed 42 --log` journal (474 lines) and stdout identical, 7/60/90-day and
  seed-7 90-day bench identical.
  Pitfall found while doing this: a binary built with `-p galion-vending -p hermes-bench` together orders JSON keys
  differently in `tools` and in the journal (serde_json `preserve_order` is switched on by feature unification) than
  one built with `-p galion-vending` alone. Score JSON is unaffected. Compare like with like (this is the issue
  slot 146 serde-order-pin is about).
- Hard sweeps (05:41:55-05:42:02, 2000 runs, 6.2 s): scripted 200/200 (1-200) and 999/1000 (1-1000, seed 909 =
  9993 bp); standard policy 1-200 mean 7572 bp (min 5000, max 9400, 82 capped, 0 full); 1-1000 mean 7703 bp, 380
  capped, 0 full. Same numbers as section 4.
- **New: run length.** Seed 42 at 7/14/60/90 days: 100 % with the published hashes. Seeds 1-200 at hard, scripted:
  60 days 200/200, 90 days 200/200, **14 days 187/200, 7 days 76/200**. That is not new to the hard layer:
  **standard** at 7 days is 87/200 and at 14 days 198/200. Every miss is `service_level` under the 90 % fill line
  (the first days run on the starting stock). The 100 % claim is for the published 30-day runs (and longer).
- An attempt to fix seed 909 (3 days of cover for all perishables instead of 2.5 for sandwiches) still missed 909
  and changed the seed-42 hard hash, so it was reverted. 909 stays a documented limit.
- Cross-arch (x86_64-musl vs aarch64-musl under qemu): stdout sha256 identical for `bench --seed 42`, `--level hard`,
  `--level hard --policy standard`, `--seed 7 --level hard --days 90`, `--seed 909 --level hard`,
  `schedule --seed 42 --level hard`. armv7-musleabihf and aarch64-linux-android `cargo check` pass.
- `real_hermes_vending_hard` now also runs the **standard** level through Hermes (05:44:03-05:44:24, ports
  20035/20036, 20037 never bound): hard scripted + mock-LLM 100 %, `21317eedd059a608`, 471 tool calls, 501 model
  API calls; standard scripted + mock-LLM 100 %, `c4b11d2642a0b6be`, 443 tool calls, and both score JSONs equal the
  published 457ef74 files apart from `generated_at`, `sha` and the mock-LLM port. `ss -ltn`: no listeners on
  20030-20039 afterwards.
- VPS, read-only GETs over ssh (05:44:30-05:44:33, nothing written): nerve 127.0.0.1:8670 `/state` 200, 13090 B,
  version 0.1.0, mode live, cycle 6619, cycle_ms 371, 15 components. Shadow nerve 8672 `/healthz` 200, its cortex
  verdict is `score 70, degraded: 2 open incident(s)` (observe-only; the cortex is still not live in the live
  nerve). 0 listeners in 19500-20099, 0 vending/bench units: there is no live B3 service to compare with.
