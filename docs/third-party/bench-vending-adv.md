<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: bench-vending-adv (slot 133)

2026-10-21 05:05 CEST (+02:00). **No new dependencies and no copied code.** `Cargo.lock` is unchanged.

The hard level and the `pricing` axis use only what `galion-vending` and `hermes-bench` already depended on on
`swarm/bench-hermes` 457ef74 (`serde`, `serde_json`, and the in-repo crates `galion-bench`, `zagreb-time`,
`hermes-rs`, `bench-banking`, `galion-wallstreet`), with the licences documented in `docs/third-party/bench-vending.md`
and `docs/third-party/bench-hermes.md`.

The margin-optimal price `p* = b + c/2` is derived in `docs/bench/vending-hard.md` from the published linear demand
model. It is a textbook first-order condition, not copied from anywhere.
