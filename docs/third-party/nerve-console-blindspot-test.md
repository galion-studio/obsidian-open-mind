<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-console-blindspot-test (wave-4 slot 332)

No new dependencies and no copied code. The regression guard
`crates/openmind-nerve/tests/real_console_blindspot.rs` uses std, the existing
`serde_json` and the `openmind_nerve` library (`config::default_specs`,
`config::CONSOLE_DEEP`). `Cargo.lock` is unchanged. Hermes was not used.
