<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Galion patch notice for this copy of candle-core 0.9.2

This directory is `candle-core` 0.9.2 as published on crates.io
(https://crates.io/crates/candle-core/0.9.2, upstream https://github.com/huggingface/candle,
commit 3b39794c14b1de8ba4c2d10d49354d557a415547, path `candle-core`), licence MIT OR Apache-2.0.
The original copyright and licence (`LICENSE`, Apache-2.0 text as shipped in the crate) are kept.

Modified 2026-10-21 by Grok Bot for galion-studio (swarm/candle-zip-fix), only so it can use
`zip` 9 (RUSTSEC-2026-0367, -0368, -0369 are fixed in zip >= 9.0.0):

1. `Cargo.toml`, `Cargo.toml.orig`: `zip` requirement 7.2.0 -> 9.0.5 (still `default-features = false`).
2. `src/npy.rs` (2 places): `ZipFile::name()` returns `ZipResult<Cow<str>>` in zip 9; add `?`.
3. `src/pickle.rs` (1 place): `ZipArchive::file_names()` yields `ZipResult<Cow<str>>` in zip 9; collect into a
   `Result` and add `?`.
4. Removed the upstream Python test-data generators `tests/npy.py`, `tests/pth.py` (repo rule: no Python files;
   the generated fixtures `tests/test.npz`, `tests/*.pt`, `tests/*.pth` are kept).

5. `tests/test.pt`, `tests/test_with_key.pt` (byte-identical to the published crate: sha256 2bc164fa…fe25 and
   2775d95f…b747) were missing from the first commit because the repo `.gitignore` ignores `*.pt`; added 2026-10-21
   (swarm/candle-zip-rebase) with a `!vendor/candle-core/tests/*.pt` exception, so `tests/pth_tests.rs` runs.

Each source change carries a `galion candle-zip-fix` comment. Nothing else differs from the published crate.
Remove this directory and the `[patch.crates-io]` entry in the workspace `Cargo.toml` once a candle release
requires zip >= 9 (as of 2026-10-21 neither 0.9.2, 0.10.x, 0.11.0 nor upstream main does).
