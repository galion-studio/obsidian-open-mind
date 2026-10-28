<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: swarm/hermes-gateway-2mib (slot 329)

Written 2026-10-28 00:50 +01:00 (Europe/Zagreb).

No new crates. Cargo.lock only gains `rusqlite` in hermes-gateway's dependency list (dev-dependency, already locked at
0.32.1 for hermes-rs, MIT).

| Item | Licence | Change |
|---|---|---|
| sha2 0.10.9 (RustCrypto) | MIT OR Apache-2.0 | feature `force-soft-compact` enabled in hermes-rs (portable compact implementation, no SIMD) |
| rusqlite 0.32.1 | MIT | dev-dependency of hermes-gateway (`bundled`) for real_size_2mib.rs legacy DB fixture |
| SQLite (bundled by libsqlite3-sys 0.30.1) | public domain | compile-time options via `LIBSQLITE3_FLAGS` and `CFLAGS_<musl triple>` (see docs/real-tests/hermes-gateway-2mib.md) |
| rust-lld | Apache-2.0 WITH LLVM-exception (toolchain, not shipped) | `-zpack-relative-relocs` for x86_64 musl hermes-gateway |
