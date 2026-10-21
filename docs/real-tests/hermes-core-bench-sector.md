<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test record: swarm/hermes-core-bench-sector (wave-4 slot 129)

Recorded 2026-10-21 05:38-05:50 CEST (+02:00) on the box, branch head before commit ff26603 (hermes-core 45b7980 + bench-hermes 457ef74). Slot 129 ports 19990-19999: the Hermes status API test used 19990, `real_hermes` was moved to 19992/19993 with `HERMES_REAL_PORT`. hermes-bench binds its in-process mock bank and mock LLM on kernel-assigned loopback ports. No secret was read or printed, no paid model was called, nothing was written to or restarted on the VPS.

## 1. Commands

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/hermes-core-bench-sector
cargo fmt --all --check
cargo-slot clippy --workspace --all-targets -j 2 -- -D warnings
cargo-slot test --workspace --exclude openmind-sandbox --exclude cgroup-detect -j 2
GALION_REAL=1 cargo-slot test --release -p hermes-core --test real_bench -j 2 -- --nocapture
GALION_REAL=1 HERMES_REAL_PORT=19992 cargo-slot test --release -p hermes-core --test real_hermes -j 2 -- --nocapture
scripts/no-python.sh; scripts/spdx-scan.sh
```

## 2. real_bench (GALION_REAL=1, release), run 05:42:56-05:43:08 CEST

Real `hermes` binary, real `hermes-bench` release binary as child processes, real registry, real hash-chained ledger, real governor, real loopback HTTP. 1 passed, 0 failed, all steps in 5.17 s.

| Step | What | Result |
|---|---|---|
| RB1 | `hermes sectors` / `bench plan` | bench sector: cap 3, lead `bench.lead`, ARTEMIS (ecosystem-census, hermes-agents), 3 sessions / 8 tasks, 0 gated. All 8 rows parse to whitelisted commands. Fresh dry run gives bench a slot in round one. |
| RB2 | `hermes bench run --bin .../release/hermes-bench` | 8 done / 0 failed / 0 blocked / 0 refused / 0 retries, peak 3 of 3 slots, 3614 ms wall. |
| RB3 | scores vs the committed bench-hermes record (`docs/bench/hermes-*.score.json`, 457ef74) | all six `pct` 100 = record, axes identical; vending `final_state_hash` c4b11d2642a0b6be = record (scripted and mock-llm). |
| RB4 | `hermes ledger --verify`, `hermes plan --sector bench` | 16 lines, chain sound, no torn tail, all stamps Zagreb; all 8 plan rows `done`. |
| RB5 | `hermes readiness` | 16 checks, 12 pass, exit 3. G1, G3, G4, G5, G16 pass. Fails (all owner items, unchanged by this branch): G9 no HADES config, G11 provider mock / OpenRouter 402, G14 7 open decisions; G12 unknown without `--repo`. |
| RB6 | `hermes serve --port 19990` | `GET /v1/bench` 200 (7410 bytes, last run done 8); `GET /v1/tasks?sector=bench&state=done` 200 count 8; `GET /v1/readiness` G16 pass; `POST /v1/bench` 405. Server killed, port closed afterwards. |
| RB7 | `hermes intake --sector bench --title "\`hermes-bench run --bench banking --agent live\`"` then `bench run --only bench:H1` | exit 3, outcome refused, 0 attempts, no process started (log count unchanged). `bench status` still shows the RB2 full run (done 8). |
| RB8 | `bench run --only S1.1` without / with `--rerun` | without: `already-done`, nothing executed; with: done, 100%, 400 ms, `full: false`. `last.json` still the RB2 run. G16 then fails only on the queued RB7 live row ("refused rows: bench:H1"), not on scores. |

Per-run detail of RB2 (hermes-bench one-line summaries):

```
bench:S1.1 done att 1 exit 0 pct 100  400 ms banking [scripted] via Hermes: 100% identical_to_direct=true checks=114 episodes=100% forbidden=0
bench:S1.2 done att 1 exit 0 pct 100  100 ms wallstreet [scripted] via Hermes: 100% pnl 130894 vs ref 130894 fingerprint "fnv1a64:e3f5e643e7750856"
bench:S1.3 done att 1 exit 0 pct 100  400 ms vending [scripted] via Hermes: 100% 30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be"
bench:S2.1 done att 1 exit 0 pct 100  100 ms banking [mock-llm] via Hermes: 100% episodes=100% forbidden=0
bench:S2.2 done att 1 exit 0 pct 100  100 ms wallstreet [mock-llm] via Hermes: 100% episodes=100%
bench:S2.3 done att 1 exit 0 pct 100 3104 ms vending [mock-llm] via Hermes: 100% 30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be"
bench:S3.1 done att 1 exit 0            100 ms 2909 bytes, 7 percent signs
bench:S3.2 done att 1 exit 0            100 ms 2778 bytes, 7 percent signs
spoken: The bench run finished with eight of eight runs done and zero not done, every scripted run scored one hundred percent, and no paid model was called.
```

An earlier run at 05:40:22-05:40:27 CEST (before the fix in section 4) gave the same RB1-RB6 numbers (3925 ms wall) and exposed the bug fixed below: after RB7, `bench status` showed a run with 0 done.

## 3. real_hermes (GALION_REAL=1, release, ports 19992/19993), run 05:42:36-05:42:45 CEST

R1-R8 all pass in 2.08 s with the bench sector present: 8 sectors, 50 sessions / 208 tasks (47+3 / 200+8), target 653 / useful 415, round-robin dispatch reaches all 8 sectors, cap 24 over limit 18 completes 28 (25 swarm first-session tasks + bench S1), HADES refuses the unapproved merge, deploy token replay refused, `serve --host 0.0.0.0` refused, loopback API answers.

## 4. Fix made in this restart (found by RB7)

`bench::save` replaced `last.json` with any run that was not all `already-done`, so a `--only` partial re-run or a refused-only intake run hid the full run that scored the matrix, and G16 then failed on "last run left undone" although every score stood. Now:

- `BenchRun.full` (`true` when no `--only`; older records default to `true`). Only a full run that executed something becomes `last.json`; every run is still kept as `run-<id>.json`.
- In a full run, `already-done` rows take the score, exit and artefact of the same task from the previous `last.json` ("done in run <id>: ..."), and `all_100` accepts them, so a full run that only redoes what failed before still carries every score.
- New test `only_full_runs_become_last_and_carry_earlier_scores_forward` (tests/bench.rs): run 1 with vending/mock-llm crashing twice (6 done, 1 failed, 1 blocked; G16 fails "left undone: bench:S2.3, bench:S3.2"), run 2 executes exactly the 2 missing rows and G16 passes with carried scores, run 3 `--only S1.1 --rerun` is kept as its own record and `last.json` stays run 2.
- real_bench RB7/RB8 now assert this against the real binaries.

## 5. Unit and workspace tests (05:39-05:47 CEST)

- `cargo fmt --all --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` (before the fix): 749 passed, 0 failed. hermes-core after the fix: lib 34, tests/bench 9, engine 6, gate 5, real_* gated 2 (skip without GALION_REAL): 0 failed. Final run after the fix (05:47 CEST): 184 suites, 750 passed, 0 failed, 0 ignored.
- `scripts/no-python.sh`: ok. `scripts/spdx-scan.sh`: 302 scanned, 0 missing.
- Cross-targets for hermes-core: x86_64-unknown-linux-musl and aarch64-unknown-linux-musl (rust-lld) release builds ok; armv7-unknown-linux-musleabihf and aarch64-linux-android `cargo check` ok.

## 6. VPS read-only probes (2026-10-21 05:43:19-05:43:40 CEST)

Over ssh, GET only, nothing written or restarted.

- Nerve `GET 127.0.0.1:8670/state`: 200, 13090 bytes, galion-nerve 0.1.0, mode `live`, cycle 6617-6618. 15 components: 11 healthy, `galion-one-mouth` and `cloudflared` degrading, `voice-worker` and `gaia` absent. Gaia view: ARTEMIS = [hermes-rs: healthy]. `hermes-rs` carries `gaia: {function: ARTEMIS, system: hermes-agents}`, unit active/running, port 8642, RSS 4452 kB, 0 restarts in 1 h, healthy since 2026-10-21 02:57:05 +02:00. This matches the mapping used here: `hermes-bench` -> hermes-agents / ARTEMIS, and the bench sector's Gaia systems (ecosystem-census, hermes-agents) are ARTEMIS.
- Cortex: not deployed. The shadow nerve on 8672 answers `/state` 200. The bench sector is observed through the nerve only.
- `GET 127.0.0.1:8642/health` (hermes-rs): 200.
- No bench service and no cortex unit on the VPS (`systemctl list-units` shows none). Nothing in the 19000-19999 range listens on the VPS.
- Name clash to know before any deploy: the VPS already has `/usr/local/bin/hermes`, a 193-byte bash wrapper from 2026-10-08 (the V1 Hermes CLI). The hermes-core binary is also called `hermes`. Content not read.

The benchmarks themselves cannot be checked against a VPS service, because no benchmark runs on the VPS. The "real data" check is RB3: the scores and state hashes the coordinator produced match the record committed by bench-hermes 457ef74.
