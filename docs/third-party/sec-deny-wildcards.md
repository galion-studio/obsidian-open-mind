<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: sec-deny-wildcards (slot 201)

No new dependencies and no copied code. Cargo.lock is unchanged.

Tool used (not a dependency, not committed): cargo-deny 0.20.2
(MIT OR Apache-2.0, https://github.com/EmbarkStudios/cargo-deny), the
binary already on the box at /workspace/swarm/tools/bin/cargo-deny.

Build helper (not a dependency, not committed): zig 0.15.2 (MIT,
https://ziglang.org) at /workspace/tools, used only as `zig cc` for the C
code in libsqlite3-sys when cross-checking aarch64/armv7 musl and android.
