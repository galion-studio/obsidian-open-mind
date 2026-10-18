# __PROJECT__ VPS — Stack Map (template)

> Observed facts only; guesses are marked UNVERIFIED, unknowns TBD. Never copy secrets here (paths/names only).
> Fill this in with `bin/map-stack.sh` output (see `skills/stack-mapping/SKILL.md`). All times Europe/Zagreb with offset, e.g. `2026-10-18 18:52 CEST (+02:00)`.

- **Host / IP:** __FILL_ME_HOSTNAME__ / __FILL_ME_HOST__      - **SSH user:** __FILL_ME_USER__ (key auth)      - **SSH alias:** my-vps
- **OS / kernel:** TBD   - **Virtualisation:** TBD (`systemd-detect-virt`)
- **CPU / RAM / disk:** TBD
- **sudo / Docker / git / reverse proxy / firewall:** TBD
- **Owner / contact:** TBD
- **Last verified:** TBD (Europe/Zagreb, with offset) — map file: `data/maps/map-YYYYMMDD-HHMMSS.txt`

## Components
<!-- one table per component; copy this block -->
### __COMPONENT_NAME__
| Field | Value |
|---|---|
| What it is | TBD |
| How it runs | systemd unit / docker container / manual process — TBD |
| Install / code dir | TBD |
| Repo | TBD (branch, last commit, remote) |
| Config files | TBD (secret files: names only) |
| Listening | TBD (bind address:port) |
| Public URL | TBD |
| Depends on | TBD |
| Logs | TBD (`journalctl -u <unit>` / file path) |
| Health | TBD (URL or command) |
| Notes | TBD |

## Shared infrastructure
- **Reverse proxy / TLS:** TBD
- **Databases:** TBD
- **Firewall:** TBD
- **Scheduled jobs:** TBD
- **Secrets locations (names only):** TBD
- **Backups:** TBD

## Port map
| Port | Bound to | Process | Component |
|---|---|---|---|
| 22 | * | sshd | SSH |

## Request flow
```
client → ingress → service → backend
```

## Health (YYYY-MM-DD HH:MM CEST/CET (+02:00/+01:00))
- TBD

## Health checks configured
See `services.list`.

## Open questions
- [ ] TBD

## Change history
| Date (Europe/Zagreb, with offset) | Change | Record file |
|---|---|---|
