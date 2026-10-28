<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: fmt-android-baseline (slot 399)

No new dependencies, no copied code. Cargo.lock unchanged. Tools used only at
build time and already in use by `scripts/ucpu-gate.sh`: zig 0.15.2 (MIT,
https://github.com/ziglang/zig) as the fallback C compiler/archiver for
Android `cargo check`; an Android NDK clang/llvm-ar (Apache-2.0 WITH
LLVM-exception) is preferred when present. Hermes research not used.
