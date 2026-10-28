<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-merge-9 (slot 430)

No new dependencies and no copied code. `Cargo.lock` is unchanged by the
two folds (50a3011c and 5a3b5048 add none). The new test
`crates/openmind-nerve/tests/real_deploy_recipes.rs` uses only std,
serde_json, zagreb-time and galion-realtest (all in-tree / already listed).
`rust-lld` is the linker that ships with every rustup toolchain (already
documented in docs/third-party/nerve-xt-rustlld-pin.md and
xt-linker-pins-arm.md).
