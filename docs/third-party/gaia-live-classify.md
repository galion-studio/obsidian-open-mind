<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: gaia-live-classify (wave-4 slot 224)

No new dependencies and no copied code. `gaia classify` uses only crates already in
the workspace (`serde_json`, MIT OR Apache-2.0, already a `gaia` / `gaia-live`
dependency) and the standard library. The real test drives the system `ssh`
client as an external program (not linked; it is not part of the build).
