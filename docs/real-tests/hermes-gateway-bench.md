<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: hermes-gateway with `HERMES_RS_BENCH=1` (slot 128)

Box only, loopback only, ports 19980-19989. No VPS was touched; the bench wiring is not deployed and
there is no live counterpart to compare. No sandbox run (sandbox_vps_only), no paid model, no key.

## Commands
```
GALION_REAL=1 cargo-slot test -j 2 --release -p hermes-gateway --features bench --test real_gateway_bench -- --nocapture
```
Run 2026-10-21 04:44:30-04:44:31 +02:00 (first run 04:44:11 +02:00 identical except it used
`ws_quote ACME`, which the paper broker refused with `unknown_symbol`; the test now uses `ws_universe`).

## Output (run 2, verbatim; the in-process bank port is ephemeral)
```
2026-10-21T04:44:30+02:00 started hermes-gateway pid 3679584 on 127.0.0.1:19980 (on)
2026-10-21T04:44:30+02:00 hermes-gateway bench on: 36 tools (banking@http://127.0.0.1:35049, wallstreet@in-process galion-wallstreet, vending@in-process galion-vending), local mock/paper/sim only, ws_kill_reset operator-only
2026-10-21T04:44:30+02:00 POST /v1/chat/completions :19980 -> 200 in 70 ms, 8 tool calls
  vending_status error=false {"kind":"status","day":1,"days":30,"weekday":1,"bank_cents":50000,...
  bank_balance error=false {"accounts":[{"id":"acc-eur-main","name":"Main EUR","currency":"EUR","balance":2450.75,...
  payment_draft error=false {"draft_id":"draft-pay-001","kind":"payment","status":"awaiting_owner",...
  ws_account error=false {"mode":"paper","as_of":"2026-08-10","starting_cash_cents":10000000,...
  ws_universe error=false {"symbols":{"GLXA":{"ticker":"GLXA","name":"Galaxia Compute",...
  ws_kill_reset error=true {"error":"Unknown tool: ws_kill_reset"}
  bank_transfer error=true {"error":"Unknown tool: bank_transfer"}
  clock error=false "2026-10-21T04:44:30+02:00"
2026-10-21T04:44:30+02:00 offered to the model: 38 tools
2026-10-21T04:44:30+02:00 mock LLM: 9 requests, 0 with auth header, models ["hermes-bench/mock-llm"]
2026-10-21T04:44:30+02:00 started hermes-gateway pid 3679601 on 127.0.0.1:19982 (off)
2026-10-21T04:44:30+02:00 POST /v1/chat/completions :19982 -> 200 in 17 ms, 1 tool calls
2026-10-21T04:44:30+02:00 bench off: offered ["clock", "delegate_task"]
2026-10-21T04:44:30+02:00 started hermes-gateway pid 3679616 on 127.0.0.1:19985 (http-bank)
2026-10-21T04:44:30+02:00 hermes-gateway bench on: 12 tools (banking@http://127.0.0.1:19983), local mock/paper/sim only, ws_kill_reset operator-only
2026-10-21T04:44:30+02:00 POST /v1/chat/completions :19985 -> 200 in 22 ms, 2 tool calls
2026-10-21T04:44:30+02:00 bank mock 127.0.0.1:19983 counters.total 0 -> 3
2026-10-21T04:44:30+02:00 public-bind: exit 2, ... bench config error: HERMES_RS_BENCH=1 refused: the gateway bind is not loopback (bench tools are for local runs only)
2026-10-21T04:44:31+02:00 remote-backend: exit 2, ... bench config error: HERMES_RS_BENCH_WS_URL: bench backends must be loopback http (127.0.0.1 / ::1 / localhost), got http://10.0.0.5:8080
2026-10-21T04:44:31+02:00 unknown-set: exit 2, ... bench config error: HERMES_RS_BENCH_SETS: unknown set "revolut" (allowed: banking, wallstreet, vending)
2026-10-21T04:44:31+02:00 all slot-128 gateway ports free again
test real_gateway_bench_loopback ... ok
```

## What it proves
1. With the switch on, a chat turn sent through the real gateway (`POST /v1/chat/completions`,
   real hermes-rs agent loop, real curl provider pointed at a loopback mock model) reaches all
   three backends. A payment stays a draft (`awaiting_owner`). `ws_kill_reset` and made-up money
   tools are `Unknown tool`.
2. With the switch off, the same binary offers only `clock` and `delegate_task`.
3. With `HERMES_RS_BENCH_BANK_URL`, the gateway talks HTTP to a mock that the test owns: its own
   counters go from 0 to 3.
4. Misconfiguration fails closed: exit 2, never listening.
5. Teardown: every child is killed, ports 19980/19981/19982/19984/19985 are free again. The test's
   bank mock thread on 19983 ends with the test process.

## Extra smoke: static musl binary
2026-10-21 04:51:38 +02:00: started `x86_64-unknown-linux-musl/release/hermes-gateway`
(`--no-default-features --features bench`, 1,557,280 B) with `HERMES_RS_BENCH=1` on 127.0.0.1:19986
under `timeout 5`. Result: `/health` 200, log `bench on: 36 tools (…)` then
`listening on 127.0.0.1:19986 mock=false loopback=true`. The process exited when the timeout
fired and the port is free.
