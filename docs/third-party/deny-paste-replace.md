<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code and crates: `swarm/deny-paste-replace`

Written 2026-10-21 07:30 CEST (+02:00). Licences read from each crate's published `Cargo.toml`.

| Name | Version | Licence (SPDX) | Source | Used for | Why |
|---|---|---|---|---|---|
| pastey | 0.2.3 (new in lock, latest stable on crates.io as of 2026-10-21; crate last updated 2026-05-20 07:43 CEST) | MIT OR Apache-2.0 | https://crates.io/crates/pastey/0.2.3, https://github.com/as1100k/pastey | `paste!`/`item!`/`expr!` token-pasting proc macros for gemm 0.19 and pulp 0.22 (under candle-core, voice crates only) | maintained drop-in fork named by RUSTSEC-2024-0436 itself; no dependencies of its own; same syntax and case modifiers, so gemm/pulp compile unchanged |
| paste (removed from lock) | 1.0.15 | MIT OR Apache-2.0 | https://github.com/dtolnay/paste (archived) | replaced by the shim below | unmaintained, RUSTSEC-2024-0436 |
| `vendor/paste-pastey` (our shim, crate name `paste`) | 1.0.15+galion-pastey | MIT | this repo | re-exports `pastey`'s three macros under the name `paste`, so `[patch.crates-io]` can swap it in for every `paste` user | no code copied from paste or pastey; 20 lines; written ourselves because Cargo's `[patch]` needs a crate named `paste` |

No GPL/LGPL/MPL/SSPL/BUSL, no C code, no `-sys` crate, no asm added. No transitive dependencies added (pastey has none).
`cargo deny check licenses` with the RULES.md allow-list: ok (see docs/real-tests/deny-paste-replace.md).
