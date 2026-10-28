<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: swarm/serde-order-pin (2026-10-21 CEST +02:00)

No new crate is added and Cargo.lock does not change. The pin turns on a
feature of an existing dependency, which links three crates that were already
in Cargo.lock (used by hermes-rs, openmind-memory, link-watch, desk-hermes)
into 18 more binaries:

| Crate | Version (Cargo.lock) | Licence (SPDX, from `cargo metadata`) | Source | Used for |
|---|---|---|---|---|
| serde_json, feature `preserve_order` | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | `Map` keeps insertion order |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap | backing map of `preserve_order` |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown | indexmap's table |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent | indexmap lookup trait |

Why not write it ourselves: the feature is serde_json's own, already compiled
in every `--workspace` build; a hand-made ordered map would be a second
`Value` type. Size cost measured on host release builds (opt-level z, LTO):
+2 304 B (hades-gate) to +10 720 B (gaia); openmind-queue -1 176 B.
No copied code.
