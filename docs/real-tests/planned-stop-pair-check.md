<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 328: planned-stop pair check (nerve slot 310 + cortex slot 309)

Branch `swarm/planned-stop-pair-check`, base `swarm/cortex-r3-planned-stop`
bdde4a07 (slot 309). Nerve under test: `swarm/nerve-planned-stop-tag`
12956b45 (slot 310), built separately. **Throwaway integration, NO deploy.**
Box only, loopback, ports 21980-21989. All times Europe/Zagreb (CET, +01:00).

## Why two binaries and not one merged tree

A textual merge of 12956b45 into bdde4a07 conflicts in 11 files
(`Cargo.lock`, `galion-common/src/http.rs`, and 9 files of `openmind-nerve`:
config, engine, incident, lib, main, score, Cargo.toml, tests/cortex_replay,
tests/support). The nerve branch sits on the live nerve lineage
(9d1a4f3, `nerve-deploy-t273u`); the cortex branch sits on the cortex
lineage (73583ab, `cortex-live-merge-3`), which carries a different
openmind-nerve library (score v2, T3.2 correlator, protected hosts: score.rs
698 vs 270 lines, incident.rs 789 vs 345). Live runs them the same way: two
binaries built from two trees, talking over HTTP (`/events/stream`,
`/actions`). So the pair is tested as it runs live; the merge was aborted.

## Finding F-328-1 (fixed here): the pair never reached R5b

First run 2026-10-28 00:16 +01:00 (cortex bdde4a07 unchanged + nerve
12956b45): **no restarts, no R3** (safe), but **no R5b either**. The tagged
nerve no longer sends `unit.exit` / `unit.failed` for a planned stop, only
`unit.planned_stop` (+ `unit.stopping`). The cortex's
`Subject::stop_shaped()` only looked at failures and `unit.exit`, so
`planned_stop()` returned `None` and the bursts fell to `R5-recovered` /
`R99-default` ("unit:console.service unit.planned_stop"). The R5b rule was
dead code against the new nerve, and the decision log did not say "planned
stop".

Fix (`crates/gaia-cortex/src/rules.rs`, 1 line + doc): a
`unit.planned_stop` event is stop-shaped too. Crash evidence still wins: a
tagged event that carries a non-stop exit status (exit 1 during a stop job)
is not R5b, and it is never a failure, so it never restarts.

New unit tests: `nerve_tagged_restart_is_r5b_not_r5_or_r99` (the exact
burst of the e2e run, plus the burst cut before Started, plus a lone
Stopping line stays non-R5b), `nerve_tagged_unclean_stop_is_not_r5b_and_never_restarts`.

## Real test (`GALION_REAL=1`, release binaries)

`crates/gaia-cortex/tests/real_planned_stop_pair.rs`:

```
GALION_REAL=1 \
GALION_NERVE_BIN=<openmind-nerve 12956b45 x86_64-musl> \
GALION_NERVE_BASE_BIN=/workspace/swarm/artifacts/openmind-nerve-9d1a4f3-x86_64-musl \
cargo test --release -p gaia-cortex --test real_planned_stop_pair -- --nocapture --test-threads 1
```

Nerve 12956b45 release x86_64-musl 1 471 264 B, sha256
`6fc73f4f94439ab79317df3ca206eaabae1f334ba636c2293ae6f185301295ec`.
Base nerve = the live artifact 9d1a4f3 (sha256 check OK). Journal = the
real VPS console.service lines of 2026-10-27 17:25:04 / 17:25:06 / 17:28:29
+01:00 through the nerve's FIFO replay (dedupe 600 s = live); cortex
coalesce 1500 ms, repeat poll 2 s, live limits; nerve dry-run.

Run 2 (with the fix) 2026-10-28 00:18:36 -> 00:20:14 +01:00, **1 passed**
(98.3 s):

| phase | nerve | nerve ring (console) | cortex decisions | restart requests |
|---|---|---|---|---|
| pair: 3 live planned restarts | 12956b45 | unit.planned_stop info, unit.stopping info; **no unit.failed / unit.exit** | R5b, R5b, R5-recovered, R5b | **0** (circuit closed, actions_requested 0) |
| pair, live cortex 73583ab (no R5b rule) | 12956b45 | as above | R5-recovered, R99-default, R99-default | **0** |
| split: Stopping, 12 s gap, then 143 + failed | 12956b45 | unit.planned_stop info | R99-default (lone Stopping, info), **R5b** | **0** |
| split control | 9d1a4f3 (live) | unit.exit high, unit.failed high | **R3-known-crash** | 1 |
| crash: exit 1 + failed | 12956b45 | unit.exit high, unit.failed high | R3-known-crash | 1 |
| kill -9: killed KILL + signal | 12956b45 | unit.exit high, unit.failed high | R3-known-crash | 1 |
| planned restart, then the NEW invocation exits 1 (hermes-rs 19:52 shape) | 12956b45 | planned_stop info for the old invocation + unit.failed high for the new | R3-known-crash | 1 |

Run 1 (cortex without the fix) 2026-10-28 00:16:56 +01:00, failed on the
R5b assert: decisions R5-recovered, R99-default, R5-recovered,
R5-recovered; restart requests 0.

Run 3 (all controls incl. `GALION_CORTEX_BASE_BIN`=73583ab) 2026-10-28 00:24:40 -> 00:26:45
+01:00, **1 passed** (126.3 s), same verdicts as run 2 in every phase. The
"live cortex" row comes from run 3.

The split row closes slot 309 open item 2: with the live nerve the split
burst (exit 143 + failed arriving > 10 s after the Stopping line) is still
R3 and a restart; with the 310 nerve it is R5b and no restart.

Regression with the live nerve (slot 309 test re-run on this branch,
`GALION_NERVE_BIN`=9d1a4f3, `GALION_CORTEX_BASE_BIN`=73583ab,
2026-10-28 00:21:30 -> 00:22:55 +01:00, 1 passed, 86.6 s): BEFORE (73583ab)
R3, R3, R2-circuit-open, 2 restarts (dec-874 reproduced); AFTER 3x R5b,
0 restarts; crash / kill -9 R3, 1 restart each; mixed R5b, R9, R3, 1 restart.

## Live VPS (read only, 2026-10-28 00:20:28 +01:00)

`systemctl show`: gaia-cortex `/opt/galion/rust/bin/gaia-cortex-73583ab-llm`
active NRestarts 0; galion-nerve `/opt/galion/rust/bin/openmind-nerve-9d1a4f3`
active NRestarts 0; console `galion-console-serve-2d846fb8` active
NRestarts 0. Nerve `/state` console: healthy, restarts_1h 0, unit active
running. Cortex `/stats`: actions_requested 2, actions_executed 2,
escalations_open 1. Neither slot 309 nor slot 310 is live.

## Deploy note (owner decision, not done)

Deploy the two together, cortex first: the fixed cortex is correct against
both the live nerve (job_type markers, test above) and the 310 nerve. The 310
nerve with the old live cortex 73583ab is also safe (run 3: R5/R99 info, 0
restarts), it only logs planned stops as R5/R99 instead of R5b. Deploy order and
restarts are for Mate / Slavik (live restart of gaia-cortex and galion-nerve).

No new dependencies.
