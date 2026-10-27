<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: galion-pyarchive (wave-4 slot 330)

No new crates enter Cargo.lock: every dependency below was already locked by other workspace crates.

| Crate | Version | Licence (SPDX) | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| sha2 (`default-features = false`, `force-soft`) | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | SHA-256 of each Phase A file for the `sha256sum -c` manifest | Already locked (hades-gate uses the same `force-soft` spec, so no SHA-NI/SIMD paths, swarm rule 1); NIST vectors `abc` and empty checked in unit tests |
| serde_json (workspace) | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | `--json` report | Workspace standard |
| zagreb-time (in-repo) | 0.1.0 | MIT | crates/zagreb-time | Europe/Zagreb RFC 3339 stamps, mtime labels | In-repo |

Transitive (all already in Cargo.lock; from `cargo tree -p galion-pyarchive -e normal`): block-buffer, cfg-if, chrono,
chrono-tz, cpufeatures, crypto-common, digest, hashbrown, iana-time-zone, itoa, num-traits, serde_core, siphasher,
typenum (MIT OR Apache-2.0); equivalent, indexmap (Apache-2.0 OR MIT); generic-array, phf, phf_shared, zmij (MIT);
foldhash (Zlib); memchr (Unlicense OR MIT, used under MIT). No GPL/LGPL/MPL. No copied code.
