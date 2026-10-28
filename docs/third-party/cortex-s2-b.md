<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: cortex-s2-b (T2.5 drift, T2.7 conn-rate)

- Written: 2026-10-20 23:25 CEST (+02:00), branch `swarm/cortex-s2-b`.

**No new third-party dependency and no copied code.**

| crate | version | licence | source | used for | why |
|---|---|---|---|---|---|
| galion-common (internal, path dep, default features = std only) | 0.1.0 (workspace) | MIT | `crates/galion-common` | `sha256::Sha256` incremental hasher + `hex::encode` for the drift sensor | already in the workspace (NIST-vector tested); avoids `sha2` (crates.io), which pulls `cpufeatures` / SHA-NI intrinsics (RULES.md 1: no SIMD) and 5 more crates |

`Cargo.lock` gains only the edge openmind-nerve -> galion-common; no new package entry.
`/proc/net/tcp` parsing is our own code (format from the Linux kernel docs, no code copied).
