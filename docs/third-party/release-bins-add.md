<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party: release-bins-add (wave-4 slot 337)

No new dependency and no copied code. Cargo.lock changes only come from the two merged branches
(gaia-cortex, nerve-cortex-contract, xtask workspace crates; already documented on their branches).
The ELF `.symtab` check in `real_release_repro.rs` and the curl-literal check in
`real_release_smoke.rs` are written from the ELF64 header layout (System V gABI), no code copied.
Hermes research was not used ($0).
