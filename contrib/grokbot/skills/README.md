# VPS skills & workflows — Slavik's VPS (Galleon / Galleon1 / Obsidian / OpenMind)

## Fill in host + user (the only two required values)
```bash
~/workflows/vps/bin/set-target.sh <host-or-ip> <user>   # updates config.env AND ~/.ssh/config block (backs both up)
~/workflows/vps/bin/vps-ssh.sh --print-config           # shows public key to give Slavik for authorized_keys
~/workflows/vps/bin/vps-ssh.sh --check                  # first connection test
```
Or edit by hand: `VPS_HOST` / `VPS_USER` in `~/workflows/vps/config.env`, and `HostName` / `User` in the
`# >>> slavik-vps` block of `~/.ssh/config`. (Scripts only need config.env; the ssh block is for plain `ssh slavik-vps`.)
Key: `~/.ssh/id_ed25519_vps` (existing, comment grokbot-box). Never generate a new one.

## Layout
- `~/skills/<skill>/SKILL.md` — single capabilities (frontmatter `name`/`description` says when to use):
  `vps-ssh-access` → `stack-mapping` → `service-health-check` → `logs-and-error-fixing` → `deploy-updates`; `routine-checks`.
- `~/workflows/*.md` — end-to-end flows chaining skills: `first-connect-and-map`, `daily-health-check`, `incident-triage`, `deploy-release`.
- `~/workflows/vps/config.env` — connection + alert settings.
- `~/workflows/vps/stack-map.md` — facts about the stack (fill after first map). `services.list` — health checks to run.
- `~/workflows/vps/bin/` — helper scripts. `data/` — local outputs (maps, health, logs, change records).

## Script safety model
| Script | Mode |
|---|---|
| vps-ssh.sh, map-stack.sh, health-check.sh, collect-logs.sh, routine-check.sh | read-only |
| set-target.sh | edits local config only (with backups) |
| service-action.sh, edit-remote-file.sh, deploy.sh | MUTATING: dry-run by default; need `--apply` + typing `yes`; capture pre-state/backups; write rollback steps to `data/changes/` |

`VPS_ASSUME_YES=1` skips the typed confirmation (still needs `--apply`) — use only when Mate explicitly approved that exact action.
Nothing is scheduled (no cron); see `routine-checks`.
