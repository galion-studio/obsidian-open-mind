<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: ci-toolchain-pin (slot 196)

No new crates, no copied code. `Cargo.lock` unchanged.

| Item | Version | Licence | Source | Use |
|---|---|---|---|---|
| Rust toolchain (rustc, cargo, rustfmt, clippy, std for 4 targets) | 1.99.1 (58cf8f9c3 2026-10-13) | MIT OR Apache-2.0 | https://static.rust-lang.org (rustup) | the pinned compiler; already the box `stable` |
| `dtolnay/rust-toolchain` action | b725eda525362e7cf34af9a7243cac1e3f01a7ec (unchanged pin) | MIT OR Apache-2.0 | https://github.com/dtolnay/rust-toolchain | only its `toolchain:` input changed from `stable` to `"1.99.1"` |
| qemu-user-static (box test runner only, already installed) | distro | GPL-2.0 (tool, not linked, not shipped) | distro package | runs the aarch64/armv7 tuner in `real_toolchain_pin` |
