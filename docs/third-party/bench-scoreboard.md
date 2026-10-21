<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: bench-scoreboard

No new third-party dependency and no copied code.

| Crate | Version | Licence | Source | Use | Why |
|---|---|---|---|---|---|
| serde_json | workspace (1.0.154 in Cargo.lock) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | parse score JSONs | already a workspace dependency |
| galion-bench | in-tree (swarm/bench-harness ea8315c) | MIT | this repo | `Scorecard`, `now_zagreb()` | shared harness, the single scoring rule |
