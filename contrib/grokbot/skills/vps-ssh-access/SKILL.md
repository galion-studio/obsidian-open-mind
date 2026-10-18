---
name: vps-ssh-access
description: Use when you need to connect to Slavik's VPS (alias slavik-vps), set or change its host/user, test connectivity, or troubleshoot SSH auth. Prerequisite for every other VPS skill.
---

# VPS SSH access

## Purpose
Reliable, non-interactive SSH to Slavik's VPS with the existing key `~/.ssh/id_ed25519_vps` (comment `grokbot-box`).

## Prerequisites
- `~/.ssh/id_ed25519_vps` + `.pub` exist (do **not** generate a new key; if missing, stop and ask Mate).
- Host/IP and username known (from Mate/Slavik). Until then `config.env` holds `__FILL_ME_HOST__` / `__FILL_ME_USER__` and every script refuses to run.
- Slavik has added the public key to `~<user>/.ssh/authorized_keys` on the VPS.

## Procedure
1. Set the target (updates `~/workflows/vps/config.env` **and** the `slavik-vps` block in `~/.ssh/config`, with backups):
   `~/workflows/vps/bin/set-target.sh <host-or-ip> <user> [port]`
2. Show effective config + the public key to send Slavik if not yet installed:
   `~/workflows/vps/bin/vps-ssh.sh --print-config`
3. Check what ssh will use: `ssh -G slavik-vps | grep -E '^(hostname|user|port|identityfile|identitiesonly) '`
4. First connection (records host key with `accept-new`; verify the fingerprint with Slavik if possible):
   `~/workflows/vps/bin/vps-ssh.sh --check`
   Expected: `connected: <user>@<hostname>`, OS line, and `sudo:` capability.
5. One-off read-only command: `~/workflows/vps/bin/vps-ssh.sh -- 'uptime'`; interactive: `vps-ssh.sh` or `ssh slavik-vps`.

## Troubleshooting
- `Permission denied (publickey)` → key not in authorized_keys, wrong user, or perms on VPS (`~/.ssh` 700, `authorized_keys` 600). Debug: `ssh -vvv slavik-vps true 2>&1 | grep -Ei 'offering|identity|denied|accepted'`.
- `Connection timed out/refused` → wrong host/port or firewall; `nc -zv <host> 22` (local check only).
- `REMOTE HOST IDENTIFICATION HAS CHANGED` → STOP. Do not delete the known_hosts entry without confirming with Mate/Slavik (possible reinstall or MITM).
- `sudo: needs password` → discovery still works in a reduced view; note it in the stack map. Never store a sudo password.

## Safety
- Never print or copy the private key; only the `.pub` may be shared.
- Don't change sshd config, authorized_keys, or firewall on the VPS.
- Don't add other keys or disable host key checking globally.

## Report
Host, user, hostname returned, OS, sudo capability, host key fingerprint (`ssh-keygen -lF <host>`), and any errors verbatim.
