<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party code: vps-preflight-scripts (slot 195)

No new third-party dependency and no copied code.

| Crate | Version | Licence | Source | Use | Why |
|---|---|---|---|---|---|
| serde_json | workspace (already in Cargo.lock) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | parse the nerve `/state` and JSON health bodies | already a workspace dependency; no new lock entries except the galion-flight package itself |
| galion-common | in-workspace | MIT | this repo | SHA-256, hex, HTTP client, Zagreb time | std-only shared helpers |

Runtime tools called (not linked): `ssh` (OpenSSH, box), `curl` (box for public URLs; VPS for
loopback GETs), and on the VPS `systemctl`, `readlink`, `sha256sum`, `stat`, `ss` (coreutils,
systemd, iproute2, all already present).
