<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party: approval console (branch swarm/approval-console-20261031)

Written 2026-10-31 09:17 CET (+01:00) Europe/Zagreb, approval-system agent C.

New crate `crates/galion-owner-key` (owner key formats, owner-approval checks, root
install helper). `galion-one` now depends on it and on `serde_json` (workspace).

| crate | version | licence | why |
|---|---|---|---|
| p256 (features ecdsa, std) | 0.13.2 | Apache-2.0 OR MIT | WebAuthn ES256 (passkey) signature check |
| ed25519-dalek (std, zeroize) | 2.2.0 | BSD-3-Clause | Ed25519 key-file signatures, records |
| sha2 (std, force-soft-compact) | 0.10.9 | MIT OR Apache-2.0 | sha256 for fingerprints and challenges |
| serde_json | workspace | MIT OR Apache-2.0 | JSON (already in the workspace) |

Transitive, all allow-listed (measured with `cargo metadata --offline`): base16ct, base64ct,
block-buffer, cfg-if, const-oid, cpufeatures, crypto-bigint, crypto-common, der, digest, ecdsa,
ed25519, elliptic-curve, ff, generic-array (MIT), getrandom, group, hmac, pkcs8, primeorder,
rand_core, rfc6979, sec1, signature, spki, typenum, zeroize (MIT OR Apache-2.0 / Apache-2.0 OR MIT);
curve25519-dalek 4.1.3 and subtle 2.6.1 (BSD-3-Clause). No GPL, no git or path sources.
ed25519-dalek, sha2 and curve25519-dalek were already in Cargo.lock (hermes-rs); new to the
lockfile are p256 and its RustCrypto ECC helpers.

Test only: Chrome's DevTools virtual WebAuthn authenticator via playwright-core (box tool at
/workspace/swarm/demo/galion-one-v2/tool, Apache-2.0), not part of any build.
