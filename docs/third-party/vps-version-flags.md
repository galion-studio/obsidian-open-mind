<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: vps-version-flags (slot 194)

No new third-party dependency and no copied code.

* 25 crates gain the in-workspace path dependency `galion-common` (MIT, std only, this repo).
* galion-wallstreet does not: its paper-only test pins its dependency list, so it carries a local copy of the flags (same output).
* Tools used only on the box for the real test: `strace` and `ss` (iproute2), both system packages, not linked or shipped.
* aarch64/armv7 builds of bundled SQLite used `zig cc` 0.15.2 (MIT) through a throwaway per-shell wrapper in /tmp (strips cc-rs's `--target=<rust triple>`), as earlier slots did. Nothing committed.
