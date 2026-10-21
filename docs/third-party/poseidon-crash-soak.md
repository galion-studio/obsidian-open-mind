<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: poseidon-crash-soak (slot 206)

No new dependencies and no copied code. The crash/soak suite, the heap
test (std `GlobalAlloc` counting wrapper over `System`) and the fixes use
only std plus crates already in `openmind-queue` (`serde`, `serde_json`,
`zagreb-time`, `openmind-provider`). The random source is an in-test
xorshift64*, not the `rand` crate.

Merged in-house code: `swarm/poseidon-refusal-persist` eec2d99 (slot 161,
the `refusals.dat` sidecar), which itself adds no dependencies.
