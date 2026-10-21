<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: openmind-nerve, nerve-client (swarm/nerve)

**New dependencies: none.** `nerve-client` is std-only. `openmind-nerve` uses
only crates that the 13a296c workspace already uses.

The full normal-dependency closure is listed below. It comes from
`cargo tree -p openmind-nerve -e normal --format '{p} | {l}'`, run 2026-10-20 19:36 CEST (+02:00).

| crate | version | licence | source |
|---|---|---|---|
| serde / serde_core / serde_derive | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| itoa | 1.0.18 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| zmij | 1.0.23 | MIT | https://github.com/dtolnay/zmij |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| quote | 1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| syn | 3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| zagreb-time (workspace) | 0.1.0 | MIT | this repo |
| chrono | 0.4.45 | MIT OR Apache-2.0 | https://github.com/chronotope/chrono |
| chrono-tz | 0.10.4 | MIT OR Apache-2.0 | https://github.com/chronotope/chrono-tz |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | https://github.com/strawlab/iana-time-zone |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits |
| phf / phf_shared | 0.12.1 | MIT | https://github.com/rust-phf/rust-phf |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | https://github.com/jedisct1/rust-siphash |

There are no copyleft, unknown or unlicensed crates, and no C or asm code.

Algorithms that were written fresh, not copied:

- `util::days_from_civil`: the well-known proleptic-Gregorian day-count formula, as
  described publicly by H. Hinnant ("chrono-Compatible Low-Level Date
  Algorithms", public domain / free use). Re-derived here; no code was copied.
- The median/MAD robust baseline (1.4826 x MAD) is textbook statistics.
- The cloudflared and zerolog line formats were read from captured logs. No
  cloudflared code is used.
