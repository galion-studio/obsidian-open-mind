<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: bench-wallstreet (`crates/galion-wallstreet`)

Written 2026-10-20 23:15 CEST (+02:00).

No new third-party crates. No copied code.

| Dependency | Version | Licence | Source | Use | Why |
|---|---|---|---|---|---|
| serde (workspace) | 1 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | derive Serialize/Deserialize | already in the workspace |
| serde_json (workspace) | 1 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | tickets, reports, HTTP bodies | already in the workspace |
| zagreb-time (in-tree) | 0.1.0 | MIT | crates/zagreb-time | Europe/Zagreb timestamps | in-tree |
| galion-bench (in-tree, dev only) | 0.1.0 | MIT | swarm/bench-harness ea8315c | MockServer, client, Scorecard in tests | shared harness; kept out of the binary so it links no HTTP client |

Transitive set is unchanged from main 13a296c (serde, serde_json, chrono, chrono-tz and their existing deps). HTTP server
and fixture parsers are hand-written std. Market data fixtures are synthetic and generated in-house (no third-party data).
