<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: bench-hermes (crate `hermes-bench`)

No new third-party dependency and no copied code. `Cargo.lock` gained only the in-tree `hermes-bench` entry.

| Crate | Version | Licence | Source | Use |
|---|---|---|---|---|
| serde_json | workspace (Cargo.lock) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | tool args/results, score JSON |
| hermes-rs | in-tree, `default-features = false` (no SQLite/C) | MIT | this repo | Tool/Registry, Agent loop, Provider, CurlProvider |
| bench-banking, galion-wallstreet, galion-vending | in-tree (swarm/bench-banking adda699, swarm/bench-wallstreet 4b1e127, swarm/bench-vending a9daaf1) | MIT | this repo | the systems under test and their reference scorers |
| galion-bench, bench-scoreboard, zagreb-time | in-tree | MIT | this repo | Scorecard, scoreboard rendering, +02:00 timestamps |

Runtime tool: the system `curl` binary (already used by hermes-rs `CurlProvider`), loopback only except the
opt-in live path.
