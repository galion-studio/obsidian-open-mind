<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-gateway: benchmark tools behind `HERMES_RS_BENCH=1`

Branch `swarm/hermes-gateway-bench` (wave 4, slot 128), based on `swarm/bench-hermes` 457ef74.
Implements option (b) of decision **H4** in `docs/bench/hermes-bench.md` as an opt-in path. It is
off by default and is not deployed anywhere. Turning it on anywhere other than a local test run is
still Slavik's call (see "Owner decisions").

## What it does
`hermes-gateway`, built with `--features bench` and started with `HERMES_RS_BENCH=1`, adds the
`hermes-bench` adapters to its tool registry. These are the same adapters the standalone
`hermes-bench` runner uses:

| Set | Tools | Default backend | Override (loopback `http` only) |
|---|---|---|---|
| B1 banking | 12 (`bank_*`, `payee_*`, `*_draft`, `drafts_list`, `audit_trail`) | in-process `bench-banking` mock on 127.0.0.1:<ephemeral> | `HERMES_RS_BENCH_BANK_URL` |
| B2 Wall Street | 13 `ws_*` (`ws_kill_reset` stays operator-only, never registered) | in-process PAPER broker (fixture) | `HERMES_RS_BENCH_WS_URL` |
| B3 vending | 11 `vending_*` | in-process sim, seed `HERMES_RS_BENCH_SEED` (42), `HERMES_RS_BENCH_DAYS` (30), optional `HERMES_RS_BENCH_VEND_LOG` | `HERMES_RS_BENCH_VEND_URL` |

`HERMES_RS_BENCH_SETS=banking,vending` picks a subset. With all three sets on, the model sees
36 bench tools, plus `clock` and `delegate_task`.
Start-up log line:
`hermes-gateway bench on: 36 tools (banking@http://127.0.0.1:N, wallstreet@in-process galion-wallstreet, vending@in-process galion-vending), local mock/paper/sim only, ws_kill_reset operator-only`.

Code: `crates/hermes-bench/src/gateway.rs` (`register_from_env`) and `crates/hermes-gateway/src/main.rs`
(`wire_bench`, called after MCP discovery and before the boss/cron agents are built).

## Safety properties (tested)
- **Two locks.** It needs both the cargo feature `bench` (off by default) and the env switch.
  A default build contains no bench code: only a 352 B stub that logs
  `HERMES_RS_BENCH ignored: this build has no bench feature`.
- **Fails closed.** If the gateway bind is not loopback, a backend URL is not loopback, a set name is
  unknown, the seed or days value is malformed, a name looks money-moving
  (`adapters::is_money_move_name`) or clashes with an existing tool (for example an MCP tool), the
  gateway logs `bench config error: …` and exits 2 without listening. Nothing is half-registered.
- **No money moves.** Banking stops at drafts with status `awaiting_owner`. Wall Street trades only
  on paper, and vending moves only simulated cents. No live Revolut, broker or supplier is reachable,
  because a non-loopback URL is refused before a socket opens.
- **No paid model.** This branch does not touch the gateway's provider selection. The real test runs
  the gateway against a loopback mock LLM with a cleared environment: 0 requests carried an
  `Authorization` header.

## Build and run
```
cargo build --release -p hermes-gateway --features bench
HERMES_RS_BENCH=1 HERMES_RS_HOST=127.0.0.1 API_SERVER_KEY=… hermes-gateway
GALION_REAL=1 cargo test --release -p hermes-gateway --features bench --test real_gateway_bench -- --nocapture
```
The real test (`crates/hermes-gateway/tests/real_gateway_bench.rs`, ports 19980-19989) has
`required-features = ["bench"]`, so plain workspace runs skip it. Evidence is in
`docs/real-tests/hermes-gateway-bench.md`.

## Size and targets
| Build (x86_64 gnu, release, opt-level z) | Bytes |
|---|---|
| base 457ef74, default features | 1,845,960 |
| this branch, default features | 1,846,312 (+352) |
| this branch, `--features bench` | 2,460,936 (+614,976; over the 2 MB T3 target) |
| this branch, x86_64 musl static, `--no-default-features --features bench` | 1,557,280 |

x86_64 musl builds both with and without default features. aarch64 musl builds with
`--no-default-features --features bench` (rust-lld). Its default `sqlite` feature still needs a
C cross-compiler for bundled SQLite; that problem predates this branch (slot 117). `cargo check`
passes on aarch64-linux-android and armv7-unknown-linux-musleabihf (`--no-default-features`).
The armv7 gnueabihf target is not installed on the box.

## Owner decisions (not decided here)
| # | Decision | Options | Evidence / note |
|---|---|---|---|
| H4 | Gateway wiring | (a) keep `hermes-bench` standalone and leave this branch unmerged; (b) merge as an opt-in path, default build unchanged (this branch); (c) also enable the feature in a shadow build | default binary +352 B; the bench build is 2.46 MB, over the 2 MB target |
| H4-1 | Public bind | (a) refuse on non-loopback bind (today); (b) allow behind the API key | the bench tools are mocks, but a public gateway would expose them to anyone who holds the key |
| H4-2 | Cron agent | (a) the bench tools are also visible to cron jobs (today; cron is off by default); (b) give them to chat only | one-line change in `main.rs` if (b) |
| H4-3 | Feature in prod builds | (a) never (today); (b) shadow only (e.g. 8652, after the slot 117 leak fix) | `serde_json/preserve_order` is unified into bench builds, so JSON key order in gateway responses can differ from default builds (see slot 146 serde-order-pin) |
