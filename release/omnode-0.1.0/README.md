<!-- SPDX-License-Identifier: MIT -->
# omnode 0.1.0 release binaries

Obsidian Open Mind, made by Galion. Phone/LAN-side node. Built 2026-10-18 21:06 CEST (+02:00).

| File | Target | Notes |
|---|---|---|
| `omnode-0.1.0-linux-x86_64.tar.gz` | x86_64-unknown-linux-musl | static |
| `omnode-0.1.0-linux-aarch64.tar.gz` | aarch64-unknown-linux-musl | static; for Android/Termux and Pi |

Verify: `sha256sum -c omnode-0.1.0-linux-<arch>.tar.gz.sha256`

Install on a **local** device (never the VPS): see `edge/openmind-node/install/linux.sh` and `install/termux.sh`. Set `OMNODE_RELEASE_URL` to this directory's URL, or copy the matching tarball onto the device.

These binaries are not a published GitHub release yet; they live on the VPS under `/opt/galion/openmind/release/omnode-0.1.0/` for Slavik to pull.
