<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: ci-release-artefacts (slot 216, 2026-10-21 CEST +02:00)

## New dependencies

| crate | version | licence (SPDX) | source | used for | why not our own |
|---|---|---|---|---|---|
| miniz_oxide | 0.8.9 (already in Cargo.lock via galion-backup) | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide | raw deflate/inflate for the release `.tar.gz` (galion-release) | a deflate encoder is ~2k lines; this is pure Rust, no SIMD feature, no C, same pin and features (`default-features = false, features = ["with-alloc"]`) as galion-backup |
| adler2 | 2.0.1 (already in Cargo.lock) | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 | transitive (miniz_oxide) | - |

No new crate enters Cargo.lock: the lock diff only adds the `galion-release`
package entry and its existing `miniz_oxide` edge. No GitHub Action was added
or re-pinned in release.yml (checkout, rust-toolchain, sbom-action and
cosign-installer keep their previous SHA pins).

## Copied code

| file | from | licence |
|---|---|---|
| crates/galion-release/src/sha256.rs | crates/galion-backup/src/sha256.rs (this repo) | MIT, same copyright |
| crates/galion-release/src/gzip.rs | crates/galion-backup/src/gzip.rs (this repo); `gunzip` added | MIT, same copyright |

Both are first-party. Shared-crate consolidation with galion-backup /
galion-deploy is the open `shared-tar-crate` decision (slot 122), not done here.

## Tools used only for testing (not linked, not shipped)

`qemu-aarch64-static` (qemu-user-static, GPL-2.0, box/CI package), GNU
`tar` and coreutils `sha256sum` (GPL-3.0) are run by the GALION_REAL test as
independent cross-checks of our archive and SHA256SUMS. They are already in
the tools-only "pending owner ruling" table (docs/third-party/README.md, D15).
