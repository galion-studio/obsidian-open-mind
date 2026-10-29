<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes teaching backup, 2026-10-29

Taken 2026-10-29 14:07 +01:00 (Europe/Zagreb) from the live Hermes home before the
sandbox-first teaching change: `SOUL.md`, `USER.md`, `control.json` and `skills/`.

Redacted before commit:
- Public IP addresses are now `<IP>`. Loopback `127.0.0.1` is kept.
- Email addresses are now `<email>`.
- `skills/.locks/` and `*.lock` files are left out (empty lock files).
- Python scripts (`*.py`) are left out, because this repository allows no Python files
  (repo-check C8). Their sha256 values are in `PYTHON-NOT-COPIED.sha256`. The full,
  unredacted copy stays only on the VPS, in the omfix backup folder under `/var/backups/`.

This folder is a backup only. Nothing here is loaded by Hermes.
