<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: bench-vending

Written 2026-10-20 23:14 CEST (+02:00).

**No new third-party crates.** `galion-vending` uses only crates that are already in the workspace `Cargo.lock`.

| Crate | Version | Licence (SPDX) | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| serde (+ serde_derive, serde_core) | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | (de)serialising tool calls, state hash, score | workspace dependency already |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | JSONL journal, HTTP bodies | workspace dependency already |
| transitive: itoa 1.0.18 (MIT OR Apache-2.0), memchr 2.8.3 (Unlicense OR MIT, used under MIT), zmij 1.0.23 (MIT), proc-macro2 1.0.107 / quote 1.0.47 / syn (MIT OR Apache-2.0), unicode-ident 1.0.27 ((MIT OR Apache-2.0) AND Unicode-3.0) | | allowed | crates.io | | |

Dev-only, in-tree: `galion-bench` (MockServer, client, Scorecard; branch swarm/bench-harness) and `zagreb-time`.

**Algorithms written from published descriptions, no code copied:**
- SplitMix64 finaliser (public domain, Vigna): constants only.
- FNV-1a 64 (public domain): constants only.

The licences were checked with `cargo metadata` on 2026-10-20 23:12 CEST (+02:00).
