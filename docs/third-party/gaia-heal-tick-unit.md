<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: gaia-heal-tick-unit (wave-4 slot 188)

No new crates and no copied code. The tests use only std, `serde_json` (already a dependency of `gaia`) and `hades-gate` (in-tree).
The tests run these external tools from the box at test time. They are not linked or shipped:
- `systemd-analyze` (systemd 257, LGPL-2.1-or-later; we run it as a program, so there is no linking or copying) for `verify` and `security --offline`.
- `cp`, `kill` (GNU coreutils / procps) in the test harness only.
