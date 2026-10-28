<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 341 cortex-live-merge-4: real-test record and deploy checklist

Branch `swarm/cortex-live-merge-4`, base `cortex-live-merge-3` 73583abf (= the
live cortex build `gaia-cortex-73583ab-llm`). All times Europe/Zagreb (CET,
+01:00), 2026-10-28. Box loopback only, slot 341 ports 22110-22119.
**NO deploy** (the live cortex deploy is a separate parent job).

## What is merged

| Step | Commit | Content |
|---|---|---|
| base | `73583abf` | cortex-live-merge-3 (live build): merge-2 + nerve-io-unlock + R4b-sandbox-oom |
| 1 | `e00f9cff` (merges `40a17ab1`) | cortex-replay-golden-fix (slot 324): `live:replay200` / `live:replay207` in the nerve replay corpus + 2 blessed expected files; 8 old goldens 0 drift |
| 2 | `969d51cb` (merges `30f2de69`) | planned-stop-pair-check (slot 328) = cortex-r3-planned-stop `bdde4a07` (slot 309, R5b-planned-stop + real tests + 7-day replay) + F-328-1 fix (a nerve-tagged `unit.planned_stop` is stop-shaped, so the tagged nerve 12956b45 reaches R5b instead of R5/R99) |
| 3 | this commit | `REAL_CORTEX_PORT_BASE` compile-time port block for `real_r3_planned_stop` and `real_planned_stop_pair` (defaults unchanged: 21790 / 21980), so merge re-runs stay in their own slot range; this record |

Both merges were clean (no conflicts). Folding `30f2de69` carries `bdde4a07`,
so the row's "cortex-r3-planned-stop bdde4a0" is included in full, plus the
slot-328 fix that was needed for it to work with the tagged nerve.

## Gates (box, 02:26-02:48 CET)

- `cargo fmt -p gaia-cortex -p openmind-nerve -- --check` ok.
- `cargo clippy -p gaia-cortex --all-targets -D warnings` ok (default, `--features llm-advisor`, and with `REAL_CORTEX_PORT_BASE=22110`).
- `cargo clippy -p openmind-nerve --all-targets -D warnings` ok.
- `cargo test -p gaia-cortex --all-targets`: **101/0** (lib 42 = 29 + 11 R5b + 2 F-328-1; binary_e2e 1, flap_dedupe 7, incident_lifecycle 11, io_unlock 6, pipeline 24, r3_week_replay 1, real suites gated -> skip-pass 8). Same with `--features llm-advisor`: **101/0**.
- `cargo test -p openmind-nerve --test replay_corpus`: **4/4** (`every_saved_capture_is_in_the_corpus` ok against the box capture root, `corpus_replays_without_drift_or_flaps`, `drift_is_detected`, `a_flapping_capture_is_caught`).
- `cargo test -p openmind-nerve --test cortex_replay`: **5/5**.
- `scripts/no-python.sh` ok.
- `cargo build --release -p gaia-cortex --features llm-advisor --target x86_64-unknown-linux-musl` ok: 1 147 680 B, static, sha256 `7ca6512b1ecb1d66eea6446498c92812796fd2d98374a6b5c24ce2e00edd49a2` (box artifact `/workspace/swarm/artifacts/gaia-cortex-<sha>-llm-x86_64-musl`, not installed anywhere).

## Real tests (`GALION_REAL=1`, release, box loopback, `REAL_CORTEX_PORT_BASE=22110`)

Binaries: nerve 9d1a4f3 = live (`openmind-nerve-9d1a4f3-x86_64-musl`, sha256 ok),
tagged nerve 12956b45 (`openmind-nerve-12956b4-x86_64-musl`, sha256
`6fc73f4f...5ec` ok), base cortex = live `gaia-cortex-73583ab-llm-x86_64-musl`
(sha256 `3cdab111...af5` ok). Journal = real VPS console.service lines
2026-10-27 17:25:04 / 17:25:06 / 17:28:29 through the nerve FIFO replay.

| Test | Time (CET) | Result |
|---|---|---|
| `real_r3_planned_stop` (nerve 9d1a4f3) | 02:37:42-02:39:36 | **1/1 ok** (86.5 s). BEFORE (live cortex 73583ab): R3, R3, R2-circuit-open, 2 restart requests (live dec-874 reproduced). AFTER (this branch): R5b, R5b, R5b, **0 restart requests**, circuit closed. crash exit 1 -> R3 (1 req); kill -9 -> R3 (1 req); planned then folded crash -> R5b, R9, R3 (1 req) |
| `real_planned_stop_pair` (nerve 12956b45 + base 9d1a4f3) | 02:39:36-02:41:49 | **1/1 ok** (126.4 s). pair: R5b, R5b, R5-recovered, R5b, 0 req; pair with live cortex: R5, R99, R99, 0 req; split tagged: R99 (lone Stopping), R5b, 0 req; split base nerve: R3 (1 req); crash/kill -9/restart-then-crash: R3 (1 req each) |
| `real_t51_chaos` (`GALION_T51_PORT_BASE=22110`, nerve bb287cc) | 02:42:07-02:43:23 | **2/2 ok** (69.3 s). S7 hung nerve decision 3639 ms with /health+/stats 0 ms; S8 resubscribe 913 ms; S9 llm 0 calls, 0 non-loopback sockets |

Not re-run here (hardcoded slot-258 ports 21270-21279, unchanged code paths
since merge-3, green there 2026-10-27 17:39-17:43): `real_nerve_io_unlock`,
`real_api_load`, `real_t237_backfill`, `real_flap_dedupe`.

Logs: `/workspace/swarm/logs/cortex-live-merge-4-{gates,real-r3,real-pair,real-t51}.log`.

## Live VPS, read-only (02:42 CET)

- `gaia-cortex` active, ExecStart `/opt/galion/rust/bin/gaia-cortex-73583ab-llm`, NRestarts 0, active since 2026-10-27 18:17:08 CET. **Not this branch.**
- `/health` (:8674): mode live, llm_enabled true, hermes_handoff true, busy false, incidents_open 2, escalations_open 1, handoffs 1, events_in 106.
- `galion-nerve` active, `openmind-nerve-9d1a4f3 serve --mode event` (the untagged nerve; 12956b45 is not deployed). /health cycle 42, score 64, system degraded.
- Live `/decisions?n=500` histogram: R99 38, R11 12, R5 5, R9 3, R1 3, R3 2, R10 2, R2 1, R3b 1.
- **Live false positive this branch fixes**: `R3-known-crash unit:console.service` at 2026-10-27 19:28:35 CET. Journal (read-only) at 19:28:33: `Stopping console.service` -> `status=143` -> `Failed with result 'exit-code'` -> `Stopped` -> `Started` (a planned restart, same shape as the 17:25 fixture). This branch, with the live nerve 9d1a4f3, decides R5b for this shape (AFTER phase above): no restart request.
- **Live true positive this branch keeps**: `R3-known-crash unit:hermes-rs.service` 19:45:28 + R3b 19:45:34: journal shows `status=2/INVALIDARGUMENT` on every start (real crash loop after a stop/start). Crash evidence wins in R5b, so this stays R3 (restart-then-crash phase above).

## Deploy checklist (parent/owner only; slot 341 does NOT deploy)

1. Artifact: the musl build above (or rebuild from this SHA with `--features llm-advisor --target x86_64-unknown-linux-musl`). Install as `/opt/galion/rust/bin/gaia-cortex-<sha>-llm`; keep `gaia-cortex-73583ab-llm` for rollback.
2. Drop-in `ExecStart=` -> new binary; keep the current env (`CORTEX_ACTIONS=live`, LLM on, Hermes handoff on, per-hour caps).
3. Works with the live nerve 9d1a4f3 (job markers / 143 exit) **and** with the tagged nerve 12956b45 (`unit.planned_stop`, F-328-1). Order is free: cortex first is safe (R5b via markers); nerve 12956b45 alone with the live cortex 73583ab gives R5/R99 (no restarts, but no R5b label) - so deploy cortex with or before nerve 310.
4. After restart: `/health` live + busy false; console planned restart (if one happens) -> `R5b-planned-stop` with 0 nerve `/actions`; any unit with exit != 143/130, OOM, KILL or core dump -> still R3.
5. Rollback: `ExecStart=` back to `gaia-cortex-73583ab-llm`, daemon-reload, restart.

## Owner decisions

- D-341-1: deploy this merge-4 as the next live cortex (replaces 73583ab; adds R5b + F-328-1 + replay corpus entries). Recommended: yes, together with or before nerve 12956b45 (slot 310 / nerve deploy candidate).
- Still open from merge-3: D-245-1 caps 3 vs 2, D-245-2 backfill burst (already live since 73583ab), D-247-1..5.
