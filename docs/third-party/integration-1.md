<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party: swarm/integration-1

No new dependencies. `Cargo.lock` is the exact union of the 20 merged
wave-1 branches' locks (checked per conflict, see
docs/integration/integration-1.md R6); `cargo deny check` (bans, licenses,
sources) passes for the workspace and edge/openmind-node.

Added licence text only: `LICENSES/Apache-2.0.txt`, the verbatim Apache
License 2.0 text (copied from serde 1.0.229's `LICENSE-APACHE`,
https://github.com/serde-rs/serde), required by REUSE for the candle whisper
files that swarm/voice-mouth vendors under `MIT OR Apache-2.0`.

Tools used on the box (not dependencies, not committed), from
/workspace/swarm/target/launch-gates/tools/bin: gitleaks 8.30.1 (MIT),
cargo-deny 0.20.2 (MIT OR Apache-2.0), cargo-cyclonedx 0.5.9 (Apache-2.0),
comply 0.1.1 (MIT OR Apache-2.0); zig 0.15.2 (MIT) for the ucpu gate.
