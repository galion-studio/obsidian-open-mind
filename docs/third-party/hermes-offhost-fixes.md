<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: hermes-offhost-fixes (wave-4 slot 314)

No new dependencies. `Cargo.toml` and `Cargo.lock` are unchanged.

- HMAC-SHA256 is RFC 2104 written in about 30 lines over the existing
  `galion_common::sha256::Sha256` (already in the tree) and checked against
  the RFC 4231 test vectors. A `hmac`/`sha2` crate would add two crates for
  one function; a hand-rolled cipher was not written (encryption at rest is
  owner decision D2, and would use an audited MIT/Apache crate).
- The SQLite sanitiser uses `rusqlite` (MIT), already a dependency behind
  the `sqlite` feature. Without that feature the DB is never sanitised and is
  omitted from snapshots.
- The real test uses only std, `serde_json` (MIT/Apache-2.0, already used)
  and `hermes-rs`.
