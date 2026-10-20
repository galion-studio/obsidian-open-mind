<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: swarm/health-land

No new third-party dependency and no copied external code.

| crate | version | licence | source | use |
|---|---|---|---|---|
| zagreb-time (in-tree, already on main) | 0.1.0 | MIT | crates/zagreb-time | `now_rfc3339()` for Europe/Zagreb stamps |

galion-health itself is std-only. Its source is our own code, landed from base.git `ws/foundation` e43d7ab (`crates/galion-health`). The Cargo.lock change is one new `galion-health` package entry. Transitive deps are those zagreb-time already pulls on main (`cargo tree -p galion-health`: chrono 0.4.45, chrono-tz 0.10.4, iana-time-zone 0.1.65, num-traits 0.2.19, siphasher 1.0.4 are MIT OR Apache-2.0; phf/phf_shared 0.12.1 are MIT), so nothing new.
