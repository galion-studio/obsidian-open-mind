<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: cross-cc-restore (slot 338)

No new crate dependencies and no copied code in this branch. The new test uses
only `galion-realtest` and `serde_json` (already in the workspace).

Box-only build/test tools (not linked, not committed, not shipped):

| tool | version | licence (SPDX) | source | used for | why not ours |
|---|---|---|---|---|---|
| zig | 0.15.2 | MIT | https://ziglang.org/download/0.15.2/zig-x86_64-linux-0.15.2.tar.xz | C cross compiler for bundled SQLite (cc-rs) on aarch64/armv7 musl | a C toolchain with musl libc for many targets in one 54 MB tarball; already the pinned choice of `scripts/install-zig.sh` |
| minisign-verify | 0.2 | MIT | https://crates.io/crates/minisign-verify | one-off signature check of the zig tarball (scratch crate) | small, audited, pure Rust |
| qemu-user (qemu-aarch64, qemu-arm) | 10.0.13 (Debian 1:10.0.13+ds-0+deb13u1) | GPL-2.0-only | https://deb.debian.org/debian/pool/main/q/qemu/ | running the cross-built test binaries on the x86_64 box | test runner only (like the existing `qemu-aarch64-static` use in real_toolchain_aarch64.rs); never linked, vendored or distributed, so the GPL does not reach our code |

The zig-compiled C (SQLite amalgamation, public domain) and the musl libc
objects (MIT) that end up in the binaries are the same as with any other C
compiler; no zig runtime is linked (rust-lld links, Rust's self-contained musl).
