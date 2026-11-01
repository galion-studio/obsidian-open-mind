<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes agent tools: zero-spend behaviour test (Rust, mock model)

`crates/hermes-gateway/tests/real_agent_behaviour.rs`, gated `GALION_REAL=1`. Rust port of the
bash + Python harness from 6cdbc2c0 (`scripts/real/hermes-agent-behaviour.sh`, not carried over).
No Python anywhere.

The test builds `hermes-mock-model` (crates/hermes-tools, release, own target dir under
CARGO_TARGET_TMPDIR; `HAB_MOCK=<path>` skips that), then runs two passes, each with a fresh
temp folder, mock model (127.0.0.1:19171) and real `hermes-gateway` (127.0.0.1:19170), both in
slot 17's range 19170-19179. Live tool layout (guard workspace = files root = tools workspace,
files write on, 4096 MB, browser/vision off), `HERMES_RS_PROVIDER_LOOPBACK_ONLY=1`, no provider
key, cost $0. A random test-only gateway key is generated and never printed.

Checks: every live tool offered (8); terminal runs in the files root; read/write/edit/list/search;
galion-files list/read; rm -rf /, curl, /etc/shadow and a ../ write refused; nothing escapes the
root; timeout_s honoured; a folder outside the workspace is hidden; the workspace shell script is
refused (strict) or runs and writes count-3.txt (script mode); a script with curl is refused in
both; both sessions answered; no key reaches the mock; canary intact; both listeners gone after
teardown (new). Strict 30 checks, script mode 31. `HAB_ONLY=strict|script` runs one pass.

Run: `GALION_REAL=1 cargo test -p hermes-gateway --test real_agent_behaviour -- --nocapture`

Runs on 2026-11-01 (CET), on the box, branch swarm/hermes-behaviour-rust-20261101 (on 4de4a982 =
live bbf6d1be + script mode):
- 13:56:37 strict: 30/30 PASS.
- 13:56:38 script mode (HERMES_RS_GUARD_SCRIPT_MODE=1): 31/31 PASS.
- Without GALION_REAL=1 the test skips (1 passed, 0.00 s). clippy -D warnings and fmt clean.
