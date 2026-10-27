<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/nerve-publish-recoveries (slot 246)

No new dependencies and no copied code. `Cargo.toml` and `Cargo.lock` are
unchanged from nerve-event-driven bb287cc. The unit fault book
(`crates/openmind-nerve/src/evt/recover.rs`, about 300 lines plus tests) uses
only `std` and the `serde_json` the crate already had. Nothing on crates.io
fits this job: the logic is a small state machine tied to the nerve's own
event contract.
