<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: hades-approval (crates/hades-gate)

Written 2026-10-20T19:10:00+02:00 (Europe/Zagreb). Branch `swarm/hades-approval`.
No code was copied. Three direct dependencies were added to `crates/hades-gate`.

## Direct dependencies

| Crate | Version | Licence (SPDX) | Source | Used for |
|---|---|---|---|---|
| ed25519-dalek | 2.2.0 | BSD-3-Clause | https://github.com/dalek-cryptography/curve25519-dalek/tree/main/ed25519-dalek | Verify (gate) and sign (admin `mint`) approval tokens. `verify_strict` only. Features: `std`, `zeroize`; no `fast` tables, no `rand_core`, no `pkcs8`/`serde`. |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | SHA-256 of the exact plan bytes (the plan hash a token is scoped to). Feature `force-soft`: portable Rust only, no SHA-NI/NEON intrinsics and no asm, also for the copy ed25519-dalek uses. |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | https://github.com/RustCrypto/utils | Wipe the seed and key-file buffers on drop (already pulled in by ed25519-dalek). |

## Why these and not our own code

* **Ed25519 over HMAC-SHA256.** The task allowed either. With HMAC the gate host would hold the same key the admin mints with, so anyone who reads the gate's key file can mint approvals. With Ed25519 only the 32-byte public key lives on the gate host; the seed stays with the admin. That is the property "verified with an admin public key" asks for.
* **Not hand-written.** Ed25519 needs constant-time field and scalar arithmetic; writing it ourselves is exactly the kind of NIH crypto CODE.md warns about. dalek is the most used Rust Ed25519 implementation.
* **Audits and advisories.** Quarkslab audited subtle, curve25519-dalek 1.2.1 and (briefly) ed25519-dalek 1.0.0-pre.1 for Tari Labs in June 2019: no critical findings (https://blog.quarkslab.com/security-audit-of-dalek-libraries.html, report https://blog.quarkslab.com/resources/2019-08-26-audit-dalek-libraries/19-06-594-REP.pdf). The maintainers say NCC also reviewed it (private summary; https://github.com/dalek-cryptography/ed25519-dalek/issues/100). Known advisories are fixed in the pinned versions: RUSTSEC-2022-0093 (ed25519-dalek < 2.0, double-public-key signing oracle) and RUSTSEC-2024-0344 (curve25519-dalek < 4.1.3, timing variability, https://rustsec.org/advisories/RUSTSEC-2024-0344.html). This is a recent audit of an older major version, not of 2.2.0.
* **sha2** is the RustCrypto SHA-256 (no formal public audit known to us). It is already a hard dependency of ed25519-dalek, so using it for the plan hash adds nothing new. A fourth copy of a hand-written SHA-256 (CODE.md lists three already) would be worse.

## No SIMD, no asm (project rule 1)

* curve25519-dalek 4.x builds an AVX2 backend on x86_64 by default. The new workspace file `.cargo/config.toml` sets `--cfg curve25519_dalek_backend="serial"` (portable Rust on every target; this also drops the `curve25519-dalek-derive` proc-macro from the build). `crates/hades-gate/tests/simd_free.rs` fails if a `RUSTFLAGS` env var replaces that list. Caveat: `RUSTFLAGS` or `CARGO_TARGET_<triple>_RUSTFLAGS` override `build.rustflags`; nobody should set them (rule 1 already bans target-cpu flags).
* sha2 uses `force-soft`.
* Check on the x86_64 musl release binary (unstripped build): the only `%ymm` instructions come from `memchr` (`find_avx2`, runtime-detected), which `serde_json` already pulled into this crate before this change. None come from curve25519-dalek or sha2.

## Transitive dependencies newly in Cargo.lock

From `cargo metadata` / `cargo tree -p hades-gate --target all`. All allowed licences.

| Crate | Version | Licence | Built for hades-gate? |
|---|---|---|---|
| curve25519-dalek | 4.1.3 | BSD-3-Clause | yes |
| ed25519 | 2.2.3 | Apache-2.0 OR MIT | yes |
| signature | 2.2.0 | Apache-2.0 OR MIT | yes |
| subtle | 2.6.1 | BSD-3-Clause | yes |
| digest | 0.10.7 | MIT OR Apache-2.0 | yes |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | yes |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | yes |
| generic-array | 0.14.7 | MIT | yes |
| typenum | 1.20.1 | MIT OR Apache-2.0 | yes |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | yes (x86/aarch64; unused with `force-soft`/`serial`) |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | build script only |
| semver | 1.0.28 | MIT OR Apache-2.0 | build script only |
| curve25519-dalek-derive | 0.1.1 | MIT/Apache-2.0 | no (only for the simd backend) |
| fiat-crypto | 0.2.9 | MIT OR Apache-2.0 OR BSD-1-Clause | no (only for the fiat backend) |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | no (optional, off) |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | no |
| wasi | 0.11.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | no |
| pkcs8, der, spki, const-oid, base64ct | 0.10.2, 0.7.10, 0.7.3, 0.9.6, 1.8.3 | Apache-2.0 OR MIT | no (optional, off) |

`version_check` (MIT/Apache-2.0, generic-array build script), `cfg-if`, `libc` were already in the lock.

## Not added

* No `rand`: key generation and nonces read `/dev/urandom` directly (std only).
* No `hex`/`base64`: tokens use a 15-line strict lower-case hex codec in `token.rs`.
* No HTTP or TOML crate: the localhost server and the flat config parser are small std code.
