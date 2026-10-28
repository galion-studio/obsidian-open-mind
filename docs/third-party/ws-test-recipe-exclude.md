<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: ws-test-recipe-exclude (slot 335)

No new dependencies. `crates/xtask` is std only (Cargo.lock gains only the
`xtask` package itself, no registry crates). The `cargo xtask` convention
(a workspace binary crate plus a cargo alias) follows matklad/cargo-xtask
(https://github.com/matklad/cargo-xtask, MIT OR Apache-2.0); no code copied.
`just` was not used: it is not installed on the box or the runners and would
be a non-Rust recipe file.
