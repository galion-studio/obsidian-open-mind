<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code and dependencies: swarm/gaia-live

Checked 2026-10-20 19:15 CEST (+02:00) with `cargo metadata --offline`.

**No new crate enters Cargo.lock** and no code was copied from outside the repo. The branch adds three dependency edges inside the workspace:

| Crate | Version | Licence (SPDX) | Source | Used for | Why not write it ourselves |
|---|---|---|---|---|---|
| serde_json (gaia-live, gaia) | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | parse nerve `/events` replies and the restart ledger; build nerve event bodies | already a workspace dependency (openmind-nerve, guardian-tick, ...); a hand-written JSON parser is a bug risk |
| zagreb-time (gaia-live) | 0.1.0 (in-repo) | MIT | crates/zagreb-time | Europe/Zagreb stamps in the ledger | ours |

Transitive crates of serde_json, all already in Cargo.lock: serde_core 1.0.229 (MIT OR Apache-2.0), itoa 1.0.18 (MIT OR Apache-2.0), memchr 2.8.3 (Unlicense OR MIT, used under MIT), zmij 1.0.23 (MIT). zagreb-time brings chrono and chrono-tz, which are unchanged and were already there through gaia-core.

The new `openmind-nerve` binary uses only std, plus serde_json and zagreb-time, which that crate already depended on.

The ported gaia-live M1 code (crates/gaia-live/src/lib.rs, tests/live.rs, crates/gaia/tests/live.rs, and the `status --live` additions) comes from the uncommitted box worktree /workspace/oom-ws/gaia. It is project code (MIT, Galion), not third-party. That directory was only read, never modified.
