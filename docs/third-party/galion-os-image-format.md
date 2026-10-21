<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: galion-os-image-format (`galion-admin image verify`)

Checked 2026-10-21 18:25 CEST (+02:00) with `cargo tree -e normal --target aarch64-unknown-linux-musl`
and `cargo metadata` (licence field of each package). No code was copied.

## New direct dependencies

| Name | Version | Licence (SPDX) | Source | Used for | Why not write it ourselves |
|---|---|---|---|---|---|
| ed25519-dalek | 2.2.0 (pinned `=2.2.0`) | BSD-3-Clause | https://github.com/dalek-cryptography/curve25519-dalek/tree/main/ed25519-dalek | Ed25519 `verify_strict` (device) and signing (builder example) | Hand-written curve arithmetic is the classic way to ship a broken verifier. Same crate, version and features (`default-features = false`, `std`, `zeroize`: no precomputed tables, no rand) as hades-gate in the openmind repo (docs/third-party/hades-approval.md there), so the image key format and verification semantics match HADES exactly. |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | streaming SHA-256 of payload files and key fingerprints | Already pulled in by ed25519-dalek; `force-soft` feature = portable Rust, no SHA-NI/NEON intrinsics, no asm (project rule). The existing in-crate `sha256.rs` (audit log) is one-shot only; streaming GB-sized files needs an incremental hasher. |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | https://github.com/RustCrypto/utils | wipe seed text and seed bytes after use (builder) and key-file buffers | Already pulled in by ed25519-dalek; compiler-barrier wiping is easy to get wrong. |

## Transitive (normal) dependencies added

| Name | Version | Licence |
|---|---|---|
| curve25519-dalek | 4.1.3 | BSD-3-Clause (serial backend forced by `.cargo/config.toml`; `tests/simd_free.rs` checks it) |
| ed25519 | 2.2.3 | Apache-2.0 OR MIT |
| signature | 2.2.0 | Apache-2.0 OR MIT |
| subtle | 2.6.1 | BSD-3-Clause |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 (compiled in, unused with `force-soft`) |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 (already a dependency) |

Build-only (curve25519-dalek build script): rustc_version 0.4.1 and semver 1.0.28, both MIT OR
Apache-2.0. All licences are on the allowed list; nothing GPL/LGPL/MPL/AGPL is linked.

Test-only tools (not linked, not committed): `qemu-aarch64` / `qemu-arm` (GPL-2.0, Debian
packages on the dev box) ran the cross-built unit and real tests. A `hades-gate` release build
from the openmind repo (integration-5 0d68f46, MIT) was used as the key generator in the real
test; it is run as a separate binary, not linked.

Binary size impact: release static galion-admin x86_64-musl 0.87 MB (slot 165) → 1.06 MB
(1,063,048 B); aarch64-musl 1,070,592 B; armv7-musleabihf 1,039,724 B.
