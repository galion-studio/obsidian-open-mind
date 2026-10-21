<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: hades-option-b

2026-10-21 04:55 CEST +02:00

**No new dependencies and no copied code.** `Cargo.toml` and `Cargo.lock` are unchanged from e6ceca7.
The typed plan module and its tests use only `std`, `hades-gate` itself, `serde_json` and `ed25519-dalek`
(both already dependencies of hades-gate; MIT OR Apache-2.0 and BSD-3-Clause). A serialisation crate for the
canonical form (e.g. `postcard`, `bincode`) was considered and not used: a line-based text form is human-readable
on the approval phone, trivially canonical, and needs no new dependency.
