<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-bench: the three benchmarks through the Hermes tool layer

Branch `swarm/bench-hermes` (wave 3, slot 59). Crate `crates/hermes-bench`, binary `hermes-bench`.
Every benchmark tool call is a hermes-rs `Tool` executed by the hermes-rs `Agent` loop (`agent::Agent::run`);
no score on this page bypasses that loop.

## Tool registries (exact sibling names)
| Benchmark | Tools | Backend |
|---|---|---|
| B1 banking | 12 `bench_banking::tools::specs()` names (`bank_*`, `payee_*`, `*_draft`, `drafts_list`, `audit_*`) | bench-banking Mock (HTTP loopback) |
| B2 wallstreet | 13 model tools `ws_*` + `ws_kill_reset` **operator-only** (not offered to a model) | galion-wallstreet PAPER broker (HTTP loopback or in-process) |
| B3 vending | 11 `vending_<ToolCall>` tools → `POST /v1/call` | galion-vending sim (HTTP loopback or in-process) |

`hermes-bench tools` prints the registries. Money guard: `adapters::is_money_move_name` rejects any tool
whose name tokens include send/transfer/pay/withdraw/topup/buy/sell/order/place/... (`*_draft` and
`drafts_list` exempt); a unit test asserts no registered tool matches. B1 payments/cards/disputes exist only
as drafts; B2 orders go to the PAPER broker; B3 is a simulation. HTTP transports refuse non-loopback hosts.

## Agents (pluggable)
| `--agent` | Provider | Default | Notes |
|---|---|---|---|
| `scripted` | in-process `ScriptedProvider` (deterministic `Brain`) | yes | the sibling reference operators ported to emit tool calls |
| `mock-llm` | hermes-rs `CurlProvider` → `MockLlm` on 127.0.0.1 (OpenAI wire) | no | same Brain behind a real HTTP chat-completions endpoint; no key sent |
| `live` | hermes-rs `CurlProvider` → OpenRouter | **off** | needs `GALION_BENCH_LIVE_MODEL=1` and `OPENROUTER_API_KEY` in env; key read by name by the provider, never printed; `GALION_BENCH_MODEL` (default `openai/gpt-4o-mini`), `GALION_BENCH_USD_CAP` (default 0.50, max 5), call cap 800 |

## Faithfulness (why 100 % via scripted proves the adapter)
- **B1**: each of the 10 sibling axes is run twice — direct and with every tool call routed through a
  one-call Hermes turn (`Via` hook added to `bench_banking::axes`); all 114 checks must be identical.
  Plus 6 Hermes episodes (balances, rent draft, FX quote, card-freeze draft, dispute draft, refuse "send now")
  gated by `handle_ask`; forbidden-call count and audit-chain verification are axes.
- **B2**: the reference ticket is executed step by step through Hermes tools and compared to
  `ticket::run_fresh`, `compliance::screen`, `backtest::reference`, `news::context`, `Universe::fixture`
  (P&L 130894 cents, fingerprint `fnv1a64:e3f5e643e7750856` both ways), scored on the 9 sibling axes;
  plus 2 episodes (paper trade, insider-tip refusal).
- **B3**: one Hermes turn per shift (`ScriptedOperator` port); the result must equal
  `galion_vending::bench(seed, days)` (final state hash `c4b11d2642a0b6be`, total_bp 10000 at seed 42 / 30 days)
  and the offline `journal::replay` of the server log must equal the live `/v1/score`. A `days_played` axis
  stops an agent that quits early from scoring 100.

A lazy model (never calls tools) scores < 50 % on B1 and < 100 % on B3 (`tests/model_paths.rs`).

## Run
```
hermes-bench run --bench all --agent scripted --out DIR          # in-process backends
hermes-bench run --bench all --agent mock-llm --llm-port 19605   # over the OpenAI wire
hermes-bench run --bench all --bank-url http://127.0.0.1:19600 --ws-url http://127.0.0.1:19601 \
  --vend-url http://127.0.0.1:19602 --vend-log LOG               # against running servers
hermes-bench scoreboard --dir DIR --sha SHA --agent scripted --out SCOREBOARD-hermes.md
GALION_REAL=1 cargo test --release -p hermes-bench --test real_hermes_bench -- --nocapture
```
Scores: `hermes-<bench>.<agent>.score.json` (bench-scoreboard compatible), board `SCOREBOARD-hermes.md`.

## Owner decisions (not decided here)
| # | Decision | Options | Evidence |
|---|---|---|---|
| H1 | Live model run | (a) none; (b) approve one run: model X, USD cap Y via `GALION_BENCH_LIVE_MODEL=1` | path implemented and tested against a loopback mock with a placeholder key; no key on the box |
| H2 | `ws_kill_reset` | (a) operator-only (today); (b) offer to the model | it resets the paper book; a model should not clear its own audit/kill state |
| H3 | Vending score for a model | (a) sibling `/v1/score` axes as-is; (b) also require equality with the reference (only meaningful for the scripted agent) | scripted run is equal to reference; live models will diverge legitimately |
| H4 | Gateway wiring | (a) keep `hermes-bench` standalone; (b) register these registries in hermes-gateway behind `HERMES_RS_BENCH=1` | (b) implemented opt-in on `swarm/hermes-gateway-bench` (feature `bench` + env switch, off by default), see `hermes-gateway-bench.md`; not merged, not deployed |
| H5 | B1 D1–D4 | see `hermes-banking-tools.md` / `hermes-v2-redesign.md` §6 | unchanged here |
