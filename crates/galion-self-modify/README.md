<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# galion-self-modify — Hermes self-change approval core

Slavik-approved design 2026-10-31. Formats: `docs/galion-self-modify-formats.md`.

| binary | runs as | does |
|---|---|---|
| `galion-sm` | openmind-hermes (stack-sign) / Slavik's device (owner-sign) | stack signature over the proposal; owner Ed25519 key-file approval; owner challenge + fingerprint helpers |
| `galion-sm-sign` | galion-sm-sign (NEW sandbox key) | re-hash artifact + logs, verify stack sig, sign test record and dry-run record. Never runs a process; refuses root |
| `galion-sm-deploy` | root (manual) | verify all 4 signatures + fingerprint + one-time nonce + expiry, then backup, install drop-in, ONE restart, health, auto-rollback, ntfy |

`deploy/` holds the account and unit definitions as FILES ONLY (sysusers.d, tmpfiles.d,
`galion-sm-test@`, `galion-sm-sign-test@`, `galion-sm-sign-dryrun@`, `galion-sm-deploy@`).
Nothing is installed on the server by this branch.
