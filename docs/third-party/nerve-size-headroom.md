<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party: nerve-size-headroom (slot 348)

No new crates, no new code from outside. Cargo.lock unchanged. Uses linker options of rust-lld
(LLVM lld, ships with every rustup toolchain, Apache-2.0 WITH LLVM-exception): `-z
pack-relative-relocs` and `--icf=all`. musl 1.2.5 (MIT, inside the Rust musl targets) handles
DT_RELR at static-pie start.
