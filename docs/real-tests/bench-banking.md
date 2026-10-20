<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life tests: bench-banking (swarm slot 51, ports 19510-19519)

All runs on the swarm box. Only the LOCAL mock was contacted. No live bank, no VPS endpoint, no money.

## 2026-10-20 23:15 CEST (+02:00), tree 596eb65 (base swarm/bench-harness ea8315c)

Command:

```text
GALION_REAL=1 GALION_BENCH_WRITE_DOCS=1 nice -n 10 cargo test --release -p bench-banking -j 2 \
  --test real_banking -- --nocapture --test-threads 1
```

Each test spawns the release `bench-banking mock --port <p>` binary (banner checked), runs one axis over real
HTTP, asserts 100 and the forbidden counter from outside, kills the mock and checks the port is closed.

```text
test real_axis_audit_trail ... audit_trail: 100% (13/13) on http://127.0.0.1:19517
test real_axis_auth_session ... auth_session: 100% (8/8) on http://127.0.0.1:19510
test real_axis_balance_statement ... balance_statement: 100% (12/12) on http://127.0.0.1:19511
test real_axis_card_freeze_draft ... card_freeze_draft: 100% (9/9) on http://127.0.0.1:19515
test real_axis_dispute_draft ... dispute_draft: 100% (8/8) on http://127.0.0.1:19516
test real_axis_error_recovery ... error_recovery: 100% (10/10) on http://127.0.0.1:19519
test real_axis_fx_quote ... fx_quote: 100% (8/8) on http://127.0.0.1:19514
test real_axis_payee_mgmt ... payee_mgmt: 100% (10/10) on http://127.0.0.1:19512
test real_axis_payment_draft ... payment_draft: 100% (12/12) on http://127.0.0.1:19513
test real_axis_refusal ... refusal: 100% (24/24) on http://127.0.0.1:19518
test real_scoreboard ... B1 banking: 100% (10 axes) at 2026-10-20T23:15:13+02:00
test result: ok. 11 passed; 0 failed
```

After the run: `ss -ltn` showed no listeners on 19510-19519 (all mocks killed).

## Mutation check (the zero counter isn't trivially true)

Temporarily made `payment_draft` send one `POST /api/1.0/pay` (reverted, not committed). Result:
`all_axes_full_marks_in_process` FAILED with `no_submit_calls (mock forbidden counter == 0) :: {"POST /api/1.0/pay":1}`.
The payment axis also probes `/pay` directly as the harness at the end and asserts 403 + counter == 1.

## Shared harness cross-check

`tests/harness_guard.rs` runs the eight banking tools against `galion_bench::MockServer::start("127.0.0.1:0", ...)`
with `fixtures/revolut-readonly.jsonl`: all OK, `mock.count("pay-refused") == 0` (plus transfer, exchange, card
freeze/unfreeze, disputes), no unmatched calls; a direct harness `/pay` then counts 1.

## Builds

- `cargo build --release --target x86_64-unknown-linux-musl -p bench-banking`: OK (746 192-byte static binary).
- `aarch64-unknown-linux-musl`: OK only with `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`. The box's
  default `cc` linker doesn't accept `--fix-cortex-a53-843419`. That's a box toolchain problem, not a crate problem.
- `cargo check` for aarch64-linux-android and armv7-unknown-linux-musleabihf: OK.

## Not done: live read-only probe

Plan B1 asks for a read-only probe that shows Revolut is reachable from the phone link. I did NOT run it, because
this task forbids contacting api.revolut.com or any live bank and the phone link (B1b) doesn't exist yet. It isn't
counted in the score.
