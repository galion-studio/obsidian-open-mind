<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party notes — swarm/cpu-only-rust

No new crates added. Vendored `candle-core` 0.9.2 (MIT OR Apache-2.0,
https://github.com/huggingface/candle) was further restricted: GPU/accelerator
Cargo features and optional deps removed (see `vendor/candle-core/GALION-PATCH.md`
item 7). Existing zip≥9 and no-SIMD Galion patches are unchanged.
