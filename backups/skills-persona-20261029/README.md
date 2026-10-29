<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Backup before the skills + persona change (2026-10-29 20:48 CET)

- The full copy is on the VPS at `/var/backups/skills-persona-20261029-20261029T194851Z/` (mode 700).
  It holds SOUL.md, config.yaml, the hermes-rs unit and all drop-ins, `skills.tgz` (skills/ and
  channels/), the live binary sha256 and the unit state. Checksums are in `vps-backup.SHA256SUMS`.
- `SOUL.md.before-redacted` is the live SOUL before the change, with public IPs written as `<IP>`.
  The VPS copy is the exact original.
