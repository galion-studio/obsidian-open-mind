<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: galion-accounts (branch swarm/galion-accounts-20261101)

Written 2026-11-01 14:58 CET (+01:00) Europe/Zagreb.

New crate `crates/galion-accounts`, which sets up numbered accounts and payments. Licences
were measured with `cargo metadata`.

## Default build (what ships)

| crate | version | licence | why |
|---|---|---|---|
| argon2 (features alloc) | 0.5.3 | MIT OR Apache-2.0 | keyed argon2id lookup hash of the account number |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | OS randomness for numbers, tokens and references |
| rusqlite (bundled) | 0.32.1 | MIT | storage. Already used by 6 workspace crates |
| serde_json | workspace | MIT OR Apache-2.0 | JSON |
| galion-common | workspace | MIT | HTTP server/client and SHA-256 (in-repo) |

Transitive dependencies, all allowed:

- **MIT OR Apache-2.0:** blake2, password-hash, base64ct, digest, block-buffer, crypto-common,
  cpufeatures, typenum, rand_core, cfg-if, libc, hashlink, hashbrown, ahash, once_cell,
  bitflags, smallvec, itoa, indexmap, equivalent, serde_core, fallible-iterator,
  fallible-streaming-iterator.
- **MIT:** generic-array, zmij.
- **Other allowed licences:** subtle (BSD-3-Clause), foldhash (Zlib), memchr (Unlicense OR
  MIT), zerocopy (BSD-2-Clause OR Apache-2.0 OR MIT).
- **SQLite C source:** libsqlite3-sys 0.30.1 is MIT and bundles the public-domain SQLite C
  source, compiled with `cc`. This is the same precedent as the rest of the workspace.

The lockfile gains argon2, blake2, password-hash and cpufeatures for the default build,
plus the optional reqwest tree below. Existing packages keep their versions.

**Flag (needs a decision, see DESIGN 2.3):** argon2 0.5 uses `cpufeatures` to pick an AVX2
block function at runtime on x86_64. It is Rust intrinsics, not assembly, but it is a SIMD
code path.

## Optional feature `reqwest` (off by default, not shipped)

reqwest 0.12.28 with `blocking` and `rustls-tls-native-roots` replaces the curl subprocess.
It pulls in:

- **MIT:** hyper, tokio, tower, http-body, bytes, mio and others.
- **Apache-2.0 OR ISC OR MIT:** rustls, hyper-rustls, rustls-native-certs.
- **ISC:** rustls-webpki, untrusted.
- **Unicode-3.0:** the icu_* crates, through idna.
- **Apache-2.0 AND ISC:** **ring** 0.17.14. **It contains C and assembly**, which the
  swarm "no asm" rule forbids. That is why the feature is off.
- **openssl-probe**, MIT OR Apache-2.0: pure Rust that only looks for certificate paths.
  It does not link OpenSSL.

The `rustls-tls` feature is avoided on purpose: it pulls in `webpki-roots`
(CDLA-Permissive-2.0, not allowed). `r-efi` (MIT OR Apache-2.0 OR LGPL-2.1+) only appears
for UEFI targets. We take it under MIT.

## Tools used only on the box (not in any build)

- **OpenSSL CLI:** used once to compute the fixed Stripe signature vectors in
  `tests/stripe_signature.rs`, as an independent cross-check.
- **Chrome with a small Node CDP script** (`/workspace/swarm/tmp/accounts/shot.mjs`): used
  for the /account screenshots.
