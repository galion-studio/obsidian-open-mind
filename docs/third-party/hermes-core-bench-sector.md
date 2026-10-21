<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/hermes-core-bench-sector

Written 2026-10-21 05:06 CEST (+02:00).

**No new dependency and no copied code.**

- `hermes-core` keeps its dependency list from swarm/hermes-core 45b7980 unchanged: serde, serde_json, toml, sha2 (force-soft), and the in-repo zagreb-time, hades-gate and gaia-core.
- The bench coordinator does not link `hermes-bench`. It runs the `hermes-bench` binary as a child process (std `process::Command`), so hermes-core does not pull in hermes-rs, the bench crates or their dependencies.
- The merge of swarm/bench-hermes 457ef74 brings in that branch's crates (`bench-banking`, `bench-scoreboard`, `galion-bench`, `galion-vending`, `galion-wallstreet`, `hermes-bench`). Their dependencies are documented on that branch under `docs/third-party/`. The `Cargo.lock` conflict was resolved by keeping hermes-core's lock and letting cargo fill in the rest (`cargo metadata --offline`). Against hermes-core 45b7980, `git diff 45b7980 HEAD -- Cargo.lock` adds exactly six path-crate entries (the six above, all version 0.1.0, no `source`). No registry crate was added or changed.
