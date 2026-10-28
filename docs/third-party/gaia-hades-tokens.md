<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: gaia-hades-tokens

Branch `swarm/gaia-hades-tokens`, 2026-10-20 (CEST, +02:00).

**No new third-party crates and no copied code.**

The only new dependency edge is internal: `crates/gaia` now depends on the
workspace crate `crates/hades-gate` (path dependency, MIT, this repo). The
`Cargo.lock` diff is that one line (`"hades-gate"` added to `gaia`'s
dependency list). Through it, `gaia` links crates that were already in
`Cargo.lock` from swarm/hades-approval (ccaab42) and are documented, with
licences and reasons, in `docs/third-party/hades-approval.md`:

| Crate | Licence (SPDX) | Source | Used for here |
|---|---|---|---|
| ed25519-dalek 2.x (no default features: `std`, `zeroize`) | BSD-3-Clause | https://github.com/dalek-cryptography/curve25519-dalek | verifying the admin's Ed25519 signature on heal-apply tokens (serial backend via `.cargo/config.toml`, no SIMD) |
| sha2 0.10 (`force-soft`) | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | plan hash (SHA-256) and the token id prefix |
| zeroize 1 | Apache-2.0 OR MIT | https://github.com/RustCrypto/utils | wiping seed buffers (admin-side `mint`, tests) |

Why not write it ourselves: Ed25519 verification and SHA-256 are exactly the
kind of code that must not be hand-rolled, and the swarm already reviewed and
adopted these crates for hades-gate. Re-using hades-gate's verifier and replay
ledger (instead of a second token scheme in gaia) is the point of this branch.

Transitive licences were re-checked with `cargo metadata` for the `gaia`
dependency closure (see docs/real-tests/gaia-hades-tokens.md, "licences"):
all on the allow-list: MIT, MIT OR Apache-2.0, BSD-3-Clause (curve25519-dalek,
ed25519-dalek, subtle) and memchr's Unlicense OR MIT (used under MIT).
