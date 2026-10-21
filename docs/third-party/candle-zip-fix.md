<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code and crates: `swarm/candle-zip-fix`

Written 2026-10-21 01:50 CEST (+02:00). Licences read from each crate's published `Cargo.toml`.

| Name | Version | Licence (SPDX) | Source | Used for | Why |
|---|---|---|---|---|---|
| candle-core (vendored copy, patched) | 0.9.2 | MIT OR Apache-2.0 | https://crates.io/crates/candle-core/0.9.2, https://github.com/huggingface/candle (3b39794, `candle-core/`) | tensor engine of the voice crates (already a dependency) | only way to use zip 9 with candle today; changes listed in `vendor/candle-core/GALION-PATCH.md` |
| zip | 9.0.5 (was 7.4.0) | MIT | https://github.com/zip-rs/zip2 | candle `.pth`/`.npz` reading (already a dependency) | fixes RUSTSEC-2026-0367/0368/0369; `default-features = false`, pure Rust |
| nix | 0.31.3 (new in lock) | MIT | https://github.com/nix-rust/nix | zip 9 on macOS/BSD only | not compiled for Linux/Android targets |
| cfg_aliases | 0.2.2 (new in lock) | MIT | https://github.com/katharostech/cfg_aliases | build helper of nix | same: macOS/BSD only |

No GPL/LGPL/MPL/SSPL/BUSL, no C code, no `-sys` crate, no asm added. The vendored crate keeps its original LICENSE.
Upstream Python test generators `tests/npy.py` and `tests/pth.py` were removed from the copy (no-Python rule).
