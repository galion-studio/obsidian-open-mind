<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/memory-real

Written 2026-10-20T19:24:00+02:00 (Europe/Zagreb).

**No new dependencies and no copied code.**

- The real-life suite (`crates/openmind-memory/tests/real_memory.rs`) uses only
  what `openmind-memory` already depends on: `rusqlite` 0.32 (`bundled`, MIT,
  https://github.com/rusqlite/rusqlite) to open the fixture `vectors.db`
  read-only, and `serde_json` (MIT OR Apache-2.0,
  https://github.com/serde-rs/json). HTTP is a hand-written `std::net::TcpStream`
  client, percentiles are computed in-test, and the fixture text uses an LCG
  (no `rand`).
- `scripts/real/memory-real.sh` uses bash, `ss`, `sha256sum` and `stat` only.
- The search changes are a re-implementation of the semantics in the VPS
  rewrite (`vps-main-20261020`, `crates/openmind-memory/src/index.rs`, our own
  MIT code, same project). No text was copied from it.
- `Cargo.lock` is unchanged by this branch, so the transitive licence set is the
  same as main 13a296c.
