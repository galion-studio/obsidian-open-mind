<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: cortex-nerve-io-unlock (slot 247)

No new dependencies and no copied code. The plan / call / commit split, the
courier thread (`std::sync::mpsc::sync_channel`, bounded, `try_send`) and the
per-batch breaker use only the Rust standard library and the crates
`gaia-cortex` already had (serde_json, galion-common, nerve-cortex-contract,
openmind-nerve, zagreb-time). `Cargo.lock` is unchanged.

Checked 2026-10-22 02:08 CEST (+02:00): a crate (e.g. a circuit-breaker
crate) was not worth it for a ten-line "stop after the first lost reply in
this batch" rule.
