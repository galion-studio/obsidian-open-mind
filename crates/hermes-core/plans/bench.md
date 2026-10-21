<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes plan: bench sector (benchmark runs coordinated by Hermes)

Written 2026-10-21 04:55 CEST (+02:00) for swarm/hermes-core-bench-sector.
This plan is built into the `hermes` binary (`plan_builtin = true` in
`config/hermes/sectors.toml`); it is the run matrix of the benchmarks sector
(`/workspace/swarm/plans/benchmarks.md`, B1 banking, B2 Wall Street, B3
vending) on the `hermes-bench` runner (swarm/bench-hermes 457ef74).

Each task title carries the exact `hermes-bench` command between backticks.
`hermes bench run` parses it, accepts only the flags listed in
`crates/hermes-core/src/bench.rs` and refuses everything else, so no row can
reach a live model, a non-loopback backend or real money. Rows depend on each
other by task or session id, as in every sector plan.

### S1: Scripted reference runs (in-process backends, no network, zero spend)

| # | task | target | useful | inputs / deps |
|---|---|---|---|---|
| S1.1 | `hermes-bench run --bench banking --agent scripted` (B1, in-process mock bank, drafts only) | 1 | 1 | - |
| S1.2 | `hermes-bench run --bench wallstreet --agent scripted` (B2, paper broker on recorded fixtures) | 1 | 1 | - |
| S1.3 | `hermes-bench run --bench vending --agent scripted --seed 42 --days 30` (B3, seeded 30-day sim) | 1 | 1 | - |

### S2: Mock-LLM wire runs (loopback OpenAI-compatible mock, hermes-rs curl path, zero spend)

| # | task | target | useful | inputs / deps |
|---|---|---|---|---|
| S2.1 | `hermes-bench run --bench banking --agent mock-llm` (same scripts over the wire) | 1 | 1 | S1.1 |
| S2.2 | `hermes-bench run --bench wallstreet --agent mock-llm` (same scripts over the wire) | 1 | 1 | S1.2 |
| S2.3 | `hermes-bench run --bench vending --agent mock-llm --seed 42 --days 30` (same scripts over the wire) | 1 | 1 | S1.3 |

### S3: Scoreboards from this run's score files

| # | task | target | useful | inputs / deps |
|---|---|---|---|---|
| S3.1 | `hermes-bench scoreboard --agent scripted` (three percentages, Zagreb stamps) | 1 | 1 | S1 |
| S3.2 | `hermes-bench scoreboard --agent mock-llm` (three percentages, Zagreb stamps) | 1 | 1 | S2 |
| S3.3 | `hermes-bench scoreboard --agent gateway` (three percentages, Zagreb stamps; added by slot 162) | 1 | 1 | S4 |

### S4: End-to-end through hermes-gateway (HERMES_RS_BENCH=1, loopback only, mock LLM, zero spend)

Added 2026-10-21 08:20 CEST (+02:00) by swarm/hermes-bench-e2e (slot 162).
The same scripts again, but the agent loop and the bench tools run in a real
`hermes-gateway` process (the `--features bench` build next to
`hermes-bench`), reached over `POST /v1/chat/completions` on 127.0.0.1. Its
model is the loopback mock LLM and its tools reach the run's own mock bank,
paper broker and vending sim over loopback HTTP. Banking stops at drafts
`awaiting_owner`, read back from the gateway's own tools.

| # | task | target | useful | inputs / deps |
|---|---|---|---|---|
| S4.1 | `hermes-bench run --bench banking --agent gateway` (B1 episodes through the gateway, drafts awaiting_owner) | 1 | 1 | S2.1 |
| S4.2 | `hermes-bench run --bench wallstreet --agent gateway` (B2 episodes through the gateway, paper broker) | 1 | 1 | S2.2 |
| S4.3 | `hermes-bench run --bench vending --agent gateway --seed 42 --days 30` (B3 30 shifts through the gateway) | 1 | 1 | S2.3 |

## Not in this plan

- Live model runs (`--agent live`, OpenRouter). They cost money and wait for
  Slavik's live-bench decision (H1); `hermes bench run` refuses the `live`
  agent even when an operator queues it by intake.
- Live Revolut or brokerage calls of any kind. No money moves. Ever.
