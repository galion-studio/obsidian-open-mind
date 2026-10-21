<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: gaia-queue-fuzz (slot 221)

No new dependencies and no copied code. `Cargo.toml` and `Cargo.lock` are
unchanged.

The fuzz harness (`crates/openmind-queue/tests/fuzz/mod.rs`) is std only:
a SplitMix64 PRNG, byte/token mutators, a time/iteration budget and
`std::panic::catch_unwind`. It follows the in-house layout of
`crates/galion-guard/tests/fuzz_parser.rs` (`GALION_FUZZ_SECS`,
`GALION_FUZZ_SEED`) rather than pulling in `cargo-fuzz`/libFuzzer
(needs nightly and a C++ runtime) or `proptest`/`quickcheck` (new
dependency trees for what a 200-line harness does). The SplitMix64
constants are the published public-domain algorithm (Vigna); written here
from the description, not copied.

The targets and the real test use only crates `openmind-queue` already
depends on (`serde`, `serde_json`).
