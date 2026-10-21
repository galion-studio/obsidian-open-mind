// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Project rule: no SIMD. `.cargo/config.toml` selects the portable `serial` curve25519-dalek
//! backend; a RUSTFLAGS env var would silently replace that list and bring the AVX2 backend back
//! on x86_64. This test catches it (same check as hades-gate in the openmind repo).

#[test]
#[allow(clippy::assertions_on_constants)]
fn curve25519_backend_is_serial() {
    assert!(
        cfg!(curve25519_dalek_backend = "serial"),
        "built without --cfg curve25519_dalek_backend=\"serial\": see .cargo/config.toml"
    );
}
