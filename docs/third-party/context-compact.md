<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: context-compact (crate openmind-compact)

Written 2026-10-21 17:45 +02:00.

No new external dependencies and no copied code.

* The library has **zero dependencies**.
* The CLI (feature `cli`, on by default) uses `serde_json`, which the workspace already
  has (`workspace.dependencies`, MIT OR Apache-2.0). Cargo.lock gains only the
  `openmind-compact` package entry.
* Algorithms are implemented from the papers, not from code: BM25 (Robertson & Zaragoza
  2009), SimHash (Charikar 2002) with the 4-band Hamming ≤ 3 index (Manku, Jain, Das Sarma,
  WWW 2007), and the splitmix64 finaliser constants (public domain, Vigna).
* Considered and **not** used (see the research doc for licences): `bm25` (MIT), `simhash`
  (MIT), `gaoya` (MIT), `probminhash` (MIT OR Apache-2.0), `text-splitter` (MIT),
  `tiktoken-rs` (MIT), `tokenizers` (Apache-2.0), `candle-core` (MIT OR Apache-2.0),
  `rust-bert` (Apache-2.0, needs libtorch), `keyword_extraction` (LGPL-3.0, not allowed),
  Selective Context (no licence, not allowed).
