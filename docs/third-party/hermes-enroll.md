<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/hermes-enroll

Written 2026-10-20T20:58:00+02:00 (Europe/Zagreb).

## New dependencies of `crates/hermes-rs` (root workspace)

| Crate | Version | Licence (SPDX) | Source | Used for | Why not our own |
|---|---|---|---|---|---|
| ed25519-dalek | 2.2.0 (`default-features = false`, `std`, `zeroize`) | BSD-3-Clause | https://github.com/dalek-cryptography/curve25519-dalek | `VerifyingKey::verify_strict` and `is_weak` for pairing payloads and credential claims | Signature verification must never be hand-rolled. omnode already signs with exactly this crate (`edge/openmind-node`), so device and Hermes use the same implementation and the same strict rules. |
| sha2 | 0.10.9 (`default-features = false`, `std`) | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | SHA-256 for device id / fingerprint recomputation and for storing credential hashes | Already pulled in by ed25519-dalek (SHA-512); same crate omnode uses for the device id. |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom | 32 random bytes per credential | OS randomness on Linux, Android and every musl target without `/dev/urandom` handling of our own; same crate omnode uses. |

Transitive crates added to `Cargo.lock` and compiled for the targets (`cargo tree -p
hermes-rs -e normal`): curve25519-dalek 4.1.3 (BSD-3-Clause), curve25519-dalek-derive
0.1.1 (MIT/Apache-2.0, proc-macro), ed25519 2.2.3 (Apache-2.0 OR MIT), signature 2.2.0
(Apache-2.0 OR MIT), digest 0.10.7, block-buffer 0.10.4, crypto-common 0.1.7,
cpufeatures 0.2.17, typenum 1.20.1 (all MIT OR Apache-2.0), generic-array 0.14.7
(MIT), subtle 2.6.1 (BSD-3-Clause), zeroize 1.9.1 (Apache-2.0 OR MIT), plus build-only
rustc_version 0.4.1 / semver 1.0.28 (MIT OR Apache-2.0). Lock-only entries that are
optional and not compiled with our features: pkcs8 0.10.2, der 0.7.10, spki 0.7.3,
const-oid 0.9.6, base64ct 1.8.3, rand_core 0.6.4 (all Apache-2.0 OR MIT), fiat-crypto
0.2.9 (MIT OR Apache-2.0 OR BSD-1-Clause), wasi 0.11.1 (wasm only).

Licence check of the whole resolved graph (`cargo metadata --locked
--filter-platform x86_64-unknown-linux-musl`, 107 packages incl. workspace crates):
42 x MIT, 42 x MIT OR Apache-2.0, 11 x Apache-2.0 OR MIT, 5 x MIT/Apache-2.0,
3 x BSD-3-Clause, 1 x BSD-2-Clause OR Apache-2.0 OR MIT, 1 x (MIT OR Apache-2.0)
AND Unicode-3.0, 1 x Unlicense OR MIT (taken under MIT), 1 x Zlib. All on the
allowlist; no GPL/LGPL/AGPL/MPL/SSPL/BUSL/unlicensed.

Note on CPU features: curve25519-dalek 4 compiles its AVX2/AVX-512 backend on x86_64
and picks it at run time (cpufeatures); sha2 does the same for SHA-NI. Neither needs a
target-cpu/target-feature flag or inline asm in our code, and `scripts/ucpu-gate.sh
grep` passes. omnode already ships the same crates.

## omnode (`edge/openmind-node`)

No new dependencies; `edge/openmind-node/Cargo.lock` is unchanged. The new
`src/enroll.rs` uses std, the existing `ed25519-dalek` (signing the claim through
`Identity::sign`), `getrandom`, `serde_json`, and calls the system `curl` binary only
for `https://` URLs (as hermes-rs already does for providers).

## Copied code

`crates/hermes-rs/src/enroll/verify.rs` is a port of `verify`/`verify_document` from
`edge/openmind-node/src/pair.rs` (this repository, MIT, Copyright (c) 3000 Galion /
galion-studio, branch swarm/edge-boot 753676e), rewritten without serde derive; the
file header names the source. No third-party code was copied.
