<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: swarm/cortex-sensor

2026-10-20 23:05 CEST (+02:00). No new third-party dependency and no copied code.

- `openmind-nerve` gains a **dev-dependency** on the in-repo crate `openmind-schema` (MIT, this repository) so the
  contract test can deserialize cortex events into the B33 `openmind_schema::NerveEvent` type. Its own dependencies
  (`openmind-config`, `toml`, `serde`, `serde_json`, `zagreb-time`) are already in the workspace lock; nothing new
  enters the lock file except that dependency edge. The runtime binary still depends on std + serde + serde_json +
  zagreb-time only.
