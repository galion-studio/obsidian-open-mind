<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: release-artifacts-refresh (slot 312)

No new dependencies and no copied code. The new test
`crates/galion-realtest/tests/real_release_artifacts.rs` uses only `std`
(hand-written HTTP/1.1 GET and ELF header reads). Build tools used on the box,
not linked: zig 0.15.2 (MIT, https://ziglang.org) as the C cross compiler,
rust-lld from the Rust toolchain (Apache-2.0 WITH LLVM-exception).
