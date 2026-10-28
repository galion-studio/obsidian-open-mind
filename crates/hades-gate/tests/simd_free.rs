// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Project rule: no SIMD. The workspace `.cargo/config.toml` selects the
//! portable `serial` curve25519-dalek backend. A `RUSTFLAGS` env var would
//! replace that list silently and bring the AVX2 backend back on x86_64; this
//! test catches it.

#[test]
#[allow(clippy::assertions_on_constants)]
fn curve25519_backend_is_serial() {
    assert!(
        cfg!(curve25519_dalek_backend = "serial"),
        "build without --cfg curve25519_dalek_backend=\"serial\": see .cargo/config.toml"
    );
}
