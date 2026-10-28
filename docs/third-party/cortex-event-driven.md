<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: cortex-event-driven

No new third-party dependencies and no copied code.

| crate | used by | source |
|---|---|---|
| serde 1, serde_json 1 (MIT OR Apache-2.0) | gaia-cortex, nerve-cortex-contract | already in the workspace (`[workspace.dependencies]`) |
| openmind-nerve, galion-common, zagreb-time | gaia-cortex | workspace path crates (MIT) |

The advisor's HTTP transport (feature `llm-advisor`, off by default) shells
out to the system `curl` (as `galion-one` and `openmind-nerve` alerts do);
no TLS crate is linked.
