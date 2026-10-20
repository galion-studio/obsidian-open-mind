<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/deploy-rust (slot 39)

**No new dependencies and no copied code.** `crates/galion-deploy` uses std only, and
the only change to `Cargo.lock` is the new `galion-deploy` package entry. The tests
have no dev-dependencies either.

| need | done with | why not a crate |
|---|---|---|
| SHA-256 (`sha_old=`/`sha_new=`, journal, dedup) | `src/sha256.rs`, written from FIPS 180-4 and checked against the NIST vectors (empty, `abc`, 448-bit, 1,000,000 × `a`) | about 70 lines; `sha2` would bring `digest`, `generic-array`, `cpufeatures` (and SIMD paths, which swarm rule 1 forbids) |
| Europe/Zagreb timestamps | `src/zagreb.rs`, the std-only module of `sim/deploy-sim` (same project, MIT) plus two `date` formats | `chrono` + `chrono-tz` are heavy for two format strings |
| signal(2), umask(2), kill(2) | three `extern "C"` declarations in `src/sys.rs` against the C library std already links | the `libc` crate (MIT OR Apache-2.0) would be fine licence-wise, but three declarations are not worth a dependency |
| HTTP health probe | `src/http.rs`, one GET over `TcpStream` | `ureq`/`reqwest` pull TLS stacks; health URLs are plain HTTP on loopback (other schemes still go through curl, as before) |
| JSON key count in the desk rehearsal | `json_top_keys` in `tests/support/mod.rs` | the test only counts top-level keys |

Licences: everything in this branch is MIT, `Copyright (c) 3000 Galion / galion-studio`.
