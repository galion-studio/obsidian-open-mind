<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party code: swarm/gaia-land

Written 2026-10-20 19:00 CEST (+02:00).

**No new dependency and no copied code.**

- The 11 Gaia crates from `ws/gaia` are std-only Rust. Their only dependency is the workspace crate `zagreb-time` (path dependency; `gaia-minerva` also uses the workspace crate `repo-check`). Both are ours (MIT).
- `Cargo.lock` changed by additions only: the 11 `gaia*` path packages. No registry crate was added, removed or bumped (`git diff 13a296c -- Cargo.lock` has no removed lines).
- The JSON writer (`gaia-core/src/json.rs`), the systemctl/ss text parsers (`gaia/src/view.rs`) and the ownership table are written here, not borrowed: each is under 300 lines, and a crate such as serde_json would add a dependency to a crate that is meant to stay std-only (UNIVERSAL-CPU.md).
- Transitive licences are unchanged from main 13a296c (zagreb-time pulls chrono / chrono-tz / phf / num-traits / iana-time-zone / siphasher / autocfg, MIT OR Apache-2.0 except phf and phf_shared (MIT); all already in main).
