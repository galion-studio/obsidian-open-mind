<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: ci-artefact-rehearsal (slot 228)

No new dependency and no copied code. `Cargo.lock` is unchanged.
The new rehearsal checks (`crates/galion-release/src/rehearsal.rs`) and the
real test use std only; packaging keeps the existing `miniz_oxide 0.8.9`
(MIT OR Zlib OR Apache-2.0, already in `Cargo.lock`, see
`docs/third-party/ci-release-artefacts.md`). Box tools used by the real test,
not shipped: `qemu-aarch64-static` (QEMU, GPL-2.0, run as an external
program only), GNU `sha256sum` (coreutils, run as an external program only).
