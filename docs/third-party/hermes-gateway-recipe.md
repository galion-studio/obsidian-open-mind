<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: hermes-gateway-recipe (slot 434)

No new third-party dependency and no copied code. `crates/xtask` gains a path dependency on the workspace
crate `galion-common` (MIT, std-only, no features) for SHA-256 and Europe/Zagreb time; `Cargo.lock` changes
only by that one edge. External programs run by `release-recipe`: git, tar, cargo, rustc, rust-lld (version
only), the C compiler (Debian musl-tools gcc 14.2.0, or zig 0.15.2). Nothing is shipped in a release binary.
Hermes research was not used.
