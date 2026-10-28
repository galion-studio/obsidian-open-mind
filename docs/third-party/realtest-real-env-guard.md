<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: realtest-real-env-guard (slot 400)

No new dependencies and no copied code. The new crate `crates/vps-sandbox-gate`
has zero dependencies (std only) and `publish = false`. It is added only as a
`[dev-dependencies]` path entry of openmind-sandbox, hermes-tools and
galion-realtest, so no release binary changes.

Why not reuse a crate: the gate is ~40 lines of env + `Path::is_dir` checks
specific to this swarm's two hosts; no crates.io crate encodes "VPS vs swarm
box" and pulling one in would add more than it saves.
