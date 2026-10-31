<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party: swarm/approval-core-20261031 (crates/galion-self-modify)

Written 2026-10-31 (+01:00) Europe/Zagreb. No code copied; dependencies only.

| crate | version | licence (SPDX) | source | used for | why not our own |
|---|---|---|---|---|---|
| p256 | 0.13.2 | Apache-2.0 OR MIT | https://github.com/RustCrypto/elliptic-curves/tree/master/p256 | verify the owner's WebAuthn/passkey ES256 (ECDSA P-256/SHA-256) assertion; tests also sign with it | hand-written elliptic-curve crypto is exactly what must not be home-made; RustCrypto is pure Rust, no asm/SIMD, audited family |
| libc | 0.2.191 (already in lock) | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | `O_NOFOLLOW`, `geteuid`, `flock` (constants differ per arch) | hard-coding per-arch flag values is a bug source |

Already in the workspace and reused with the same features as hermes-rs: ed25519-dalek 2, sha2 0.10
(force-soft-compact), getrandom 0.2, serde_json (workspace pin).

Transitive crates new to Cargo.lock (all `Apache-2.0 OR MIT` / `MIT/Apache-2.0`, pure Rust):
base16ct 0.2.0, crypto-bigint 0.5.5, ecdsa 0.16.9, elliptic-curve 0.13.8, ff 0.13.1, group 0.13.0,
hmac 0.12.1, primeorder 0.13.6, rfc6979 0.4.0, sec1 0.7.3 (checked with `cargo metadata`).
