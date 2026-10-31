<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# hermes-tools sandboxed suite behind the VPS gate (slot 444)

**Short version:** the 10 tests in `crates/hermes-tools/tests/sandboxed.rs`
start real sandboxes (bwrap/landlock/cgroups). They now all go through
`vps_sandbox_gate::sandbox_real()` first, so on a laptop, CI or the swarm box
they print SKIP / REFUSED and start nothing. A lint test makes sure a new
test in that file cannot skip the gate.

## Change

- `sandboxed.rs`: the shared `skip(test)` helper calls
  `vps_sandbox_gate::sandbox_real(test)` before the old CI / features checks;
  all 10 tests call `skip("<name>")`. The one test that had its own direct
  gate call now relies on the helper.
- `crates/vps-sandbox-gate/tests/every_sandbox_real_test_is_gated.rs`: new
  test `every_test_in_a_sandbox_running_suite_reaches_the_gate` scans every
  file listed in `SANDBOX_SUITES` or calling `openmind_sandbox::features()`
  and fails naming each `#[test]` that does not reach the gate (directly or
  through a helper that calls it). Self-test
  `gate_reach_check_sees_direct_helper_and_missing`.

## Evidence (box, 2026-10-31)

| check | result |
|---|---|
| `cargo test -p vps-sandbox-gate -p hermes-tools` | ok; sandboxed 10 pass via SKIP; lint 3 pass |
| mutation: gate removed from `skip()` | lint FAILS and names all 10 tests |
| process watch, no env / `GALION_REAL=1` / `GALION_REAL=1 GALION_REAL_SANDBOX=1` | 0 child processes, 0 new cgroups each; output SKIP, SKIP-no-opt-in, REFUSED (not the VPS) |
| `cargo test --workspace --no-fail-fast` (GALION_REAL unset) | 2,095 passed, 0 failed, 28 ignored |
| `cargo fmt --check`, `cargo clippy -D warnings` | ok |

## Not in scope (owner note)

`crates/openmind-sandbox/tests/sandbox.rs` (31 tests) also uses
`features()` and is the sandbox crate's own suite; the lint skips that
directory by design. Whether it should be VPS-gated too is a separate call
(**D-444-1**, rec: yes, as a follow-up slot).

Rollback: `git revert` the commit. Nothing deployed.
