<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code and dependencies: tuner-cutover

**None added.** The `tuner` crate has an empty `[dependencies]` section (std only), and `Cargo.lock` is unchanged by this branch. No code was copied from outside the repo.

The test fixtures in `crates/tuner/tests/fixtures/live/` are raw HTTP responses of our own live tuner. The static files they contain (`crates/tuner/www/*`) are already in main under the crate's MIT `LICENSE`.

Build-time tools only (not linked, not shipped): `rust-lld` from the Rust toolchain (Apache-2.0 WITH LLVM-exception, as part of rustc's distribution) as the musl linker, and `qemu-aarch64-static` (box system package, used only to run the aarch64 test, not distributed).
