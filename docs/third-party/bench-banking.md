<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party code: bench-banking (swarm slot 51)

Written 2026-10-20 23:16 CEST (+02:00).

No new external dependencies and no copied code.

| Dependency | Version | Licence | Source | Use |
|---|---|---|---|---|
| serde_json | workspace (1.x) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | JSON (already in the workspace) |
| zagreb-time | in-tree | MIT | crates/zagreb-time | Zagreb RFC 3339 stamps (pulls chrono / chrono-tz, already in the workspace) |
| galion-bench | in-tree (swarm/bench-harness ea8315c) | MIT | crates/galion-bench | Scorecard in the score file; MockServer cross-check test |

HTTP, IBAN mod-97, FNV-1a and the mock are written here with std only (small, and no HTTP crate is used elsewhere
in the workspace's servers either).
