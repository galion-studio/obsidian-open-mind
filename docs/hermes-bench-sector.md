<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes bench sector: Hermes coordinates the benchmark runs

Written 2026-10-21 05:05 CEST (+02:00). Branch `swarm/hermes-core-bench-sector`. It is hermes-core 45b7980 with bench-hermes 457ef74 merged in (merge ff26603).
Code: `crates/hermes-core/src/bench.rs`, built-in plan `crates/hermes-core/plans/bench.md`, registry entry `[[sector]] id = "bench"` in `config/hermes/sectors.toml`.
Real-test record: `docs/real-tests/hermes-core-bench-sector.md`.

## 1. What changed

Before this branch, hermes-core coordinated seven swarm sectors, and the benchmarks (B1 banking, B2 Wall Street, B3 vending, `plans/benchmarks.md`) were run by hand with `hermes-bench run`. Now the benchmarks are a sector of their own, and Hermes runs them like any other work:

| Piece | Where | What it does |
|---|---|---|
| Sector | `sectors.toml`: `id = "bench"`, lead `bench.lead`, `max_concurrent = 3`, Gaia ARTEMIS: `ecosystem-census` and `hermes-agents` | Hermes knows the sector. G1 requires it. |
| Plan | `crates/hermes-core/plans/bench.md`, compiled in (`plan_builtin = true`) | 3 sessions, 8 tasks. S1 has the 3 scripted runs. S2 has the 3 mock-LLM runs, each after its scripted run. S3 has the 2 scoreboards, each after its session. |
| Whitelist | `bench::Spec::parse_command` | Each task title carries the exact `hermes-bench` command between backticks. It is accepted only as `run --bench banking/wallstreet/vending --agent scripted/mock-llm/gateway [--seed N] [--days 1-365] [--max-shifts 1-10000]` or `scoreboard --agent scripted/mock-llm/gateway`. Everything else is refused before any process starts: the `live` agent, every `--*-url`, `--out`, `--dir`, `--llm-port`, duplicate flags, stray words and other subcommands. |
| Coordinator | `bench::run`, CLI `hermes bench run --bin <hermes-bench>` | Runs the bench tasks as real `hermes-bench` processes, in dependency order, through the governor (bench cap 3). Each child gets a cleared environment with only `PATH`, `HOME`, `TMPDIR` and `TZ`, so `GALION_BENCH_LIVE_MODEL`, `OPENROUTER_API_KEY` and any other secret in Hermes's environment never reach it. Each run has a timeout (default 900 s; the child is killed when it expires). A stale score file is deleted before each run. A run counts as done only if it exits 0 and its score JSON parses with a `pct`. |
| Retry | `[retry] max_error_attempts` (2) | A crash or a missing artefact is retried once. After that the task is failed, and its dependents are blocked, never run. |
| Ledger | the same hash-chained JSONL ledger | `dispatched`, `stopped`, `retry-scheduled`, `completed`, `failed`, `blocked` and `refused` lines, with agent `bench.<task>.a<n>`, so `hermes plan --sector bench`, `/v1/tasks?sector=bench` and `hermes ledger --retries` show bench runs like any task. |
| Record | `<state>/bench/last.json`, `run-<id>.json` | Every row's outcome, attempts, exit, pct, ms and artefact path, plus one spoken sentence. Every run is kept as `run-<id>.json`. Only a full run (no `--only`, `full: true`) that executed something becomes `last.json`, so a no-op run, a partial re-run or a refused intake row never hides the run that scored the matrix. In a full run, `already-done` rows carry the score and artefact of the previous `last.json`, so a run that only redoes what failed before still holds every score. |
| Readiness | G16 | Passes when the sector and its built-in plan are present, every bench task parses to an offline command the HADES gate lets through, the whitelist refuses three probes (`--agent live`, a remote `--bank-url`, `--out /etc/x`), and the last recorded run completed every plan task with every scripted run at 100%. |
| API | `GET /v1/bench` | The parsed matrix, task states and the last run. The API stays GET-only (POST gives 405). |
| Intake | `hermes intake --sector bench --title "\`hermes-bench run ...\`"` | An operator can queue extra runs, for example another seed. A queued `--agent live` or URL row is refused, and G16 names it until it is dealt with. |

## 2. Commands

```
hermes bench plan                         # the matrix: task, state, deps, exact hermes-bench argv
hermes bench run --bin target/release/hermes-bench --sha <commit> [--out DIR] [--rerun] [--only S1.1,S2.1] [--cap N] [--timeout-secs N] [--time-scale F]
hermes bench status                       # the last recorded run
hermes readiness                          # G16 is the bench entry
```

Without `--rerun`, tasks that are already done in the ledger are skipped (`already-done`). `--time-scale` multiplies the policy backoff (30 s × 0.01 = 0.3 s by default) so a box retry does not wait half a minute.

## 3. Result on the box (release build; first 2026-10-21 05:00-05:01 CEST, re-run 05:43:02-05:43:06 CEST)

All 8 runs were done on the first attempt with 0 retries. Peak was 3 of 3 slots, and the run took 3.3 s wall (3.6 s in the 05:43 re-run, same scores).

| Task | Run | Result |
|---|---|---|
| S1.1 | banking scripted | 100%, identical_to_direct=true, 114 checks, forbidden 0 |
| S1.2 | wallstreet scripted | 100%, P&L 130894 = reference, same ticket fingerprint |
| S1.3 | vending scripted, seed 42, 30 days | 100%, 30 shifts, 443 tool calls, total_bp 10000, state hash c4b11d2642a0b6be |
| S2.1-S2.3 | the same three over the mock-LLM wire | 100% each (vending 2.9 s) |
| S3.1, S3.2 | scoreboards | 2909 and 2778 bytes, 7 × "100%" each |

Every score's `pct` and axes are identical to the record committed by bench-hermes 457ef74 (`docs/bench/hermes-*.score.json`), and the vending `final_state_hash` matches too. The full log is in the real-test record.

## 4. Safety

- No paid model: the `live` agent is not in `AGENTS` and is refused with "it calls a paid model (OpenRouter) and waits for Slavik's live-bench decision H1". Nothing here reads a key.
- No money: B1 has drafts only, B2 a paper broker, B3 simulated cents (bench crates, unchanged).
- No VPS: runs are local processes on the box. The VPS was only probed read-only (see the real-test record).
- Ports: the Hermes status API test uses 19990 (slot 129 range 19990-19999). hermes-bench binds its in-process mock bank and mock LLM on kernel-assigned loopback ports, as it always does (no flag for them is allowed through the whitelist).
- HADES: every bench task classifies as code or read-only (asserted in `builtin_plan_parses_into_eight_offline_runs`), and the coordinator still asks `gate.preview` and refuses any task the gate would hold.

## 5. Open items / owner decisions

1. **Live bench runs (H1, Slavik).** A live OpenRouter run would be a new whitelist agent plus spend approval. It is deliberately impossible today.
2. **Gaia mapping of the six bench crates (Slavik may move).** They were unmapped after the merge, and the guard test failed. They are mapped by the galleon-arena precedent: five to ecosystem-census / ARTEMIS, and hermes-bench to hermes-agents / ARTEMIS (see `docs/gaia-ownership.md`).
3. Bench cap 3: chosen as one slot per benchmark and to match the box build queue. The sector plan said 5 agents. Raising it is one line.
4. **Clearing a refused intake row (Hermes design, no owner money).** An operator-queued `--agent live` row stays refused in the ledger and keeps G16 failing ("refused rows") until it is dealt with; hermes-core has no command to withdraw an intake row yet. Options: add `hermes intake --withdraw KEY` (ledger event, HADES-free since nothing runs), or let G16 ignore refused intake rows. Not decided here.
5. Name clash before any deploy: the VPS already has `/usr/local/bin/hermes` (V1 CLI wrapper). The hermes-core binary is also `hermes`.
6. Not done: the bench sector does not deploy anything or schedule runs by itself (there is no cron). A run is started by an operator or by Hermes's `bench run`.

## 6. Gateway end to end (slot 162, swarm/hermes-bench-e2e, 2026-10-21 08:30 CEST +02:00)

The sector now also drives `hermes-gateway`. New agent `gateway` in the whitelist
(`AGENTS = scripted, mock-llm, gateway`; `live` still refused) and in hermes-bench
(`--agent gateway`, `crates/hermes-bench/src/e2e.rs`). Plan: session S4 (S4.1 banking, S4.2
Wall Street, S4.3 vending 42/30, each after its mock-LLM run) and scoreboard S3.3 (after S4);
12 tasks, 4 sessions (`planner_*` in `sectors.toml` updated, G4 212 = 212).

How a gateway run works: hermes-bench starts its usual backends, puts loopback HTTP fronts in
front of the paper broker and the vending sim, starts a mock LLM, then spawns the
`hermes-gateway` file next to itself with a cleared environment, `HERMES_RS_BENCH=1`,
`HERMES_RS_HOST=127.0.0.1`, MCP off, a per-run test key, and the three backend URLs. Every
episode or shift is one `POST /v1/chat/completions`. The gateway must log
`bench on: 36 tools`, or the run fails (a default build, a missing binary, a non-loopback
anything: exit 1). There is no flag or env for the binary, ports or URLs, so the whitelist
stays closed. Banking drafts are read back from the gateway's own tools; the banking card gets
the axis `payments stay drafts awaiting_owner (gateway tools)`.

G16 now also needs a gateway run for each benchmark in the plan and every gateway run at 100%
(`MUST_BE_100 = scripted, gateway`); its evidence says `gateway e2e runs 3/3`.
Result: real_bench RB1-RB9 pass, all 12 tasks done, gateway scores = the mock-LLM record
(banking + 1 axis), vending hash c4b11d2642a0b6be; see `docs/real-tests/hermes-bench-e2e.md`.

Building for the sector: `cargo build --release -p hermes-bench -p hermes-gateway --features hermes-gateway/bench`.
Owner items added: E1 below.

- **E1 (Slavik, H4 follow-up):** G16 now requires the bench build of `hermes-gateway` beside
  `hermes-bench` wherever `hermes bench run` runs. Options: (a) keep (today; the box is the only
  place it runs), (b) make S4 optional when no bench gateway is present (G16 would then pass
  without the end-to-end proof), (c) ship a bench-feature gateway only in a shadow build (H4-3 b).
