<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hades sensor fixtures (cortex T2.13)

Synthesised (no live data), byte-for-byte in the formats the producers write:

- `ledger.jsonl`: hades-gate `Ledger::note` / `Ledger::consume` lines (`crates/hades-gate/src/ledger.rs`, ccaab42;
  unchanged on swarm/hades-redteam e6ceca7) with the exact `Refusal::reason` strings.
- `audit.jsonl`: hermes-tools `Tools::refuse` / `done` lines (swarm/hermes-tools 8ccdfcc, `src/tools.rs`,
  `exec::summary` for `sandbox.not_enforced` = `{layer: why}`).

Keys, nonces, token hashes and paths in them are fake. The tests assert none of them leave the sensor.
