<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: real-ports-batch-2 (wave-4 slot 375)

No new third-party dependencies and no copied code. New dev-dependency edges
are in-workspace path crates only: `galion-realtest` (deps serde_json +
zagreb-time, both already in Cargo.lock) for console-static, console-ui,
room and galion-one. `ports::Block` is std-only (`std::sync::OnceLock`).
