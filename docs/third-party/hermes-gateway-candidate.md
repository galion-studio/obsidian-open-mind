<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: hermes-gateway-candidate (slot 416)

No new dependencies. The two new tests use only crates already in the workspace (`serde_json` for body
compares, std for processes and TCP). The folded 657a9988 commit adds `libc` (MIT OR Apache-2.0, already
in `Cargo.lock`) as a direct dependency of `hermes-tools`. Build tools: rustc 1.99.1, rust-lld, Debian
musl-tools 1.2.5 gcc 14.2.0 (box only, not shipped); zig 0.15.2 used only for the comparison build.
