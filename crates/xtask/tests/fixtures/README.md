<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# xtask host fixtures

Only the files `cargo xtask` reads. No secrets.

- `vps-20261028`: live VPS, read-only ssh `cat` 2026-10-28 01:15 CET (+01:00), kernel 6.8.12-43-pve (Proxmox LXC). Uid set to 65534 (the sandbox test user).
- `box-20261028`: the swarm box 2026-10-28 01:2x CET (+01:00), uid 1000; RULES.md reduced to the marker line.
- `gh-ubuntu-2404-default` / `-sysctl`: synthetic GitHub `ubuntu-24.04` runner (Ubuntu 24.04 defaults, AppArmor userns restriction on / off after the self-eval sysctl step). Not a capture.
