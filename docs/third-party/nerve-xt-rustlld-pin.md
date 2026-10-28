<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-xt-rustlld-pin (slot 386)

No new dependency and no copied code. `rust-lld` is the LLVM linker that
ships with every rustup toolchain (`rustlib/<host>/bin/rust-lld`, part of the
Rust distribution, MIT OR Apache-2.0 with LLVM's Apache-2.0 WITH
LLVM-exception); it was already used for aarch64/armv7 musl (slot 145) and by
`scripts/ucpu-gate.sh` / `crates/galion-deploy/tests/real_build.rs` for x86_64.
The config parser in `real_cross_target.rs` is a few lines of std code (no
`toml` crate). Cargo.lock unchanged.
