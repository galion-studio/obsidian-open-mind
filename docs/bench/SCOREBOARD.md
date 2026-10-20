<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Benchmark scoreboard (banking, Wall Street, vending machine)

Branch `swarm/bench-scoreboard`, base main 13a296c. Rubric: `/workspace/swarm/plans/benchmarks.md`.
Started 2026-10-20T22:58:00+02:00 (Europe/Zagreb, CEST). No money moves: sandbox, paper and
recorded fixtures only. Live mutating Revolut calls need a separate written approval from Slavik.

| Benchmark | Score | Source branch @ SHA | Score JSON | Folded in (Zagreb) | Failing axes |
|---|---|---|---|---|---|
| B1 Banking (mock Revolut-shaped bank) | PENDING | swarm/bench-banking @ — | — | — | not yet reported |
| B2 Wall Street (paper broker + delayed data) | PENDING | swarm/bench-wallstreet @ — | — | — | not yet reported |
| B3 Vending machine (`galion-vending`, seeded ≥30 days) | PENDING | swarm/bench-vending @ — | — | — | not yet reported |

## How a score is folded in
1. A sibling branch commits a score JSON (expected under `docs/bench/` or `evidence/bench/<name>/`)
   in the `galion-bench` `Scorecard::to_json` shape `{"pct":N,"axes":[{"name":..,"score":0..100}]}`
   (swarm/bench-harness, in progress), plus `tests` and a `ts`; `ts` is Zagreb
   with an offset, never `Z`.
2. The scoreboard row takes `pct`, the branch head SHA, the JSON path and the time it was read.
3. Anything short of 100 % lists every axis below 100 in "Failing axes".
4. A score only counts if it is backed by named GALION_REAL=1 Rust tests on that branch.

## Poll log
Sibling worktrees `/workspace/swarm/wt/bench-{banking,wallstreet,vending}` and base.git refs
`swarm/bench-*` were polled every 3 minutes (no busy loop).
