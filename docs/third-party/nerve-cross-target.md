<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: nerve-cross-target (slot 217)

No new dependencies; Cargo.toml and Cargo.lock unchanged.

- The test's SHA-256 (`crates/openmind-nerve/tests/real_cross_target.rs`) is written here
  from the public FIPS 180-4 specification (constants K and H0 are from the standard), not
  copied from any crate. It is checked against the three FIPS 180-4 example digests
  (`sha256_known_answers`, always on). Chosen over a `sha2` dev-dependency to keep the
  dependency list unchanged for one test-only fingerprint.
- Runtime tools used by the gated test, not linked: `cargo` (the running one, `$CARGO`),
  `qemu-aarch64` user mode (GPL-2.0, Debian package; executed as an external program only,
  nothing linked or copied).
