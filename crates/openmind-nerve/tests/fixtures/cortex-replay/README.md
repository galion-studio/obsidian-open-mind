<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# cortex-replay fixture (synthetic)

Synthesised on 2026-10-20 22:58 CEST (+02:00) for T1.2 (`Sensor` trait); no host data, hostnames are `*.example.test`.
It makes every built-in sensor fire once: `listeners` (public :9999, unmanaged :8652 :8653), `scanners`
(Vite console + 3 paths), `edge` (refused :3011 :4001), `components` (stuck loading 8002, restarts, error rate,
memory-heavy), `ddos` (tuner access-log flood 20 req/s vs 0.1 baseline).

`../cortex-replay.expected.{json,md}` were produced by the **pre-trait** binary (integration-1 8fd26f6):
`env -i NERVE_DATA_DIR=/tmp/x openmind-nerve replay . [--md]`. The new build must match them byte for byte
once the two additive finding keys `kind` and `source` are removed (tests/cortex_replay.rs).
`../cortex-replay.expected.preserve-order.md` is the same pre-trait binary built with `cargo build --workspace`:
workspace feature unification turns on serde_json `preserve_order`, which changes health-body key order and gaia
table row order in the markdown (pre-existing; not caused by the trait).
