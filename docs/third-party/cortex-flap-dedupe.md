<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/cortex-flap-dedupe (slot 235)

No new dependency and no copied code. `Cargo.lock` is unchanged.

| piece | uses | licence |
|---|---|---|
| `crates/gaia-cortex/src/repeat.rs` (repeat watch, ring reader, poller) | `std` (Mutex, Condvar, threads), `serde_json` (already a dependency), `galion_common::http` (workspace) | MIT (this repo) |
| `crates/gaia-cortex/tests/real_flap_dedupe.rs` | `std`, `serde_json`, `zagreb-time` (workspace) | MIT (this repo) |

Why written here instead of borrowed: the work is ~300 lines of
bookkeeping over the nerve's own `GET /evt` JSON (compare a `count` per
`seq`). A crate would add a dependency for no gain; nothing comparable on
crates.io knows this contract.
