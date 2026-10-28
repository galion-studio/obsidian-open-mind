<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: real-ports-batch-3 (wave-4 slot 383)

No new third-party dependencies and no copied code. The only new dependency
edge is an in-workspace path dev-dependency: `hermes-gateway` ->
`galion-realtest` (deps serde_json + zagreb-time, both already in
Cargo.lock). `openmind-nerve` already had it. `ports::Block` is std-only
(`std::sync::OnceLock`). Hermes-research was not used ($0).
