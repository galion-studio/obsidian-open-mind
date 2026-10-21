<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: toolchain-aarch64 (slot 145)

No new crates and no copied code. Cargo.lock unchanged.

Tools used (not dependencies, not shipped):
| tool | version | licence | source | use |
|---|---|---|---|---|
| rust-lld (LLVM lld, shipped in rustup) | rustc stable on the box | Apache-2.0 WITH LLVM-exception | https://github.com/rust-lang/rust | linker for aarch64/armv7 musl |
| zig cc | 0.15.2 (already on the box, scripts/install-zig.sh) | MIT | https://ziglang.org | C cross compiler for bundled SQLite, per shell only |
| qemu-user-static | box package | GPL-2.0 (tool only, run not linked) | https://www.qemu.org | running aarch64/armv7 binaries in the real test |
