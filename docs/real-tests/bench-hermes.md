<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: bench-hermes (slot 59, ports 19600-19609)

Test: `crates/hermes-bench/tests/real_hermes_bench.rs` (GALION_REAL=1). It builds the sibling release binaries,
starts `bench-banking mock` (127.0.0.1:19600), `galion-wallstreet serve` (19601) and `galion-vending serve
--seed 42 --days 30` (19602), runs the real `hermes-bench` binary for all three benchmarks with the scripted
agent and then with the mock LLM over the OpenAI wire (19605), asserts 100 % each, renders the scoreboards,
stops everything (vending via `POST /v1/shutdown`, others killed) and checks the ports are free.
No key, no paid call, no live service; all loopback.

Command (code at 36b0e18):
```
GALION_REAL=1 GALION_BENCH_WRITE_DOCS=1 nice -n 10 cargo test --offline --release -j 2 -p hermes-bench --test real_hermes_bench -- --nocapture
```
Output:
```
started 2026-10-20T23:47:38+02:00

running 1 test
started bench-banking mock pid 2789035 on 127.0.0.1:19600
galion-wallstreet: PAPER broker listening on 127.0.0.1:19601 (2026-10-20T23:47:38+02:00)
started galion-wallstreet serve pid 2789036 on 127.0.0.1:19601
started galion-vending serve pid 2789038 on 127.0.0.1:19602
$ hermes-bench run --bench all --agent scripted ...
banking [scripted] via Hermes: 100%  identical_to_direct=true checks=114 episodes=100% forbidden=0
wallstreet [scripted] via Hermes: 100%  pnl 130894 vs ref 130894 fingerprint "fnv1a64:e3f5e643e7750856" (ref "fnv1a64:e3f5e643e7750856") episodes=100%
vending [scripted] via Hermes: 100%  30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be" identical_to_reference=true replay_matches_live=true

$ hermes-bench run --bench all --agent mock-llm ...
banking [mock-llm] via Hermes: 100%  episodes=100% forbidden=0
wallstreet [mock-llm] via Hermes: 100%  episodes=100%
vending [mock-llm] via Hermes: 100%  30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be" identical_to_reference=true replay_matches_live=true

scoreboard (scripted) -> /workspace/swarm/wt/bench-hermes/crates/hermes-bench/../../docs/bench/SCOREBOARD-hermes.md
scoreboard (mock-llm) -> /workspace/swarm/wt/bench-hermes/crates/hermes-bench/../../docs/bench/SCOREBOARD-hermes.mock-llm.md
stopped galion-vending serve (127.0.0.1:19602)
stopped galion-wallstreet serve (127.0.0.1:19601)
stopped bench-banking mock (127.0.0.1:19600)
all ports 19600/19601/19602/19605 free again
test real_all_three_benchmarks_through_hermes ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.01s

ended 2026-10-20T23:47:42+02:00
```
After the run `ss -ltn` showed 0 listeners on 19600-19609. The same test passed in two further runs
(≈4 s each). An earlier run failed once with `mock LLM bind: Address already in use` (the mock's accept
thread outlived its drop); fixed in 36b0e18 by joining the thread on drop.

Live model (OpenRouter) run: **not performed** — `OPENROUTER_API_KEY` is not present in the environment
on the box (checked by count only, value never read). The live path is covered by
`tests/model_paths.rs::live_code_path_against_loopback_mock_with_placeholder_key`.

Artifacts: `docs/bench/hermes-{banking,wallstreet,vending}.{scripted,mock-llm}.score.json`,
`docs/bench/SCOREBOARD-hermes.md` (scripted), `docs/bench/SCOREBOARD-hermes.mock-llm.md`.
