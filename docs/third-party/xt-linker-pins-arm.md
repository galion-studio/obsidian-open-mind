<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: xt-linker-pins-arm (slot 409)

No new dependency and no copied code. The change is std-only test code in
`crates/openmind-nerve/tests/real_cross_target.rs` plus comments in
`.cargo/config.toml`.

Tools used on the box only, as before (not linked into anything):

| Tool | Version | Licence | Source | Use |
|---|---|---|---|---|
| rust-lld (rustup component, LLVM lld) | rustc 1.99.1 toolchain | Apache-2.0 WITH LLVM-exception | https://github.com/rust-lang/rust | musl cross linker (already pinned by slots 145/386) |
| qemu-user-static (qemu-aarch64, qemu-arm) | 10.0.13 (Debian) | GPL-2.0 (tool only, run as a separate process, nothing linked or copied) | https://www.qemu.org | run the aarch64/armv7 binaries in the real test (already used since slot 217) |

Not added: the Android NDK (~1 GB; the prebuilt download is under the Android SDK License Agreement, not one of the RULES.md licences, although its sources are Apache-2.0 / LLVM). Linking android binaries
needs it; that is owner decision D-409-1 (docs/real-tests/xt-linker-pins-arm.md).
