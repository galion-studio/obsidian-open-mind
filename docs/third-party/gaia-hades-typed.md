<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: gaia-hades-typed (wave 4 slot 152)

No new third-party crate and no copied code. Cargo.lock gains two edges, no new packages.

| Edge | Version | Licence (SPDX) | Source | Used for | Why |
|---|---|---|---|---|---|
| gaia-hades -> hades-gate (path) | workspace 0.1.0 | MIT | this repo, crates/hades-gate | `typed::{Plan, Step, ...}` grammar and `adapter::gaia_action` | one grammar, one signer: a copy in gaia-hades could drift from what the gate parses. `gaia` already links hades-gate, so the gaia binary gains no crate. |
| gaia-hades -> ed25519-dalek (dev only) | 2.2.0 (already locked) | BSD-3-Clause | https://github.com/dalek-cryptography/curve25519-dalek | sign throwaway test tokens in tests/typed_gate.rs | same spec as hades-gate (`default-features = false`, `std`, `zeroize`), so no feature change. |

Transitive licences are unchanged from integration-3 + hades-option-b (docs/third-party/hades-approval.md,
docs/third-party/hades-option-b.md).
