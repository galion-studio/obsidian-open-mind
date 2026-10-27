# Deploy wave Mate t273u / t274u — 2026-10-27 Europe/Zagreb (+01:00)

Authorized by Mate t273u (pt273u) + t274u Hermes notify. Executor sand-subagent-1676ffa5.
No VPS IP in this file. Secrets never printed. No Cloudflare DNS/HADES edits.

## Results

| Item | Result | SHA / binary | Health | Rollback |
|---|---|---|---|---|
| Approvals recorded | DONE | swarm/t273u-rulings 9abaf1d | n/a | n/a |
| Nerve | **DEPLOYED** | tip `9d1a4f3` (`swarm/nerve-deploy-t273u` = ae3eee0+9479050+1d33c59 hoff-23); `/opt/galion/rust/bin/openmind-nerve-9d1a4f3`; drop-in `galion-nerve.service.d/90-deploy-t273u.conf` | GET /health /state /incidents /stats 200; event mode | `rm …/90-deploy-t273u.conf; daemon-reload; restart galion-nerve` → bb287cc |
| Cortex | **DEPLOYED** | merge-3 `73583ab` **with `--features llm-advisor`**; `/opt/galion/rust/bin/gaia-cortex-73583ab-llm`; drop-in `gaia-cortex.service.d/90-deploy-t273u.conf`; live actions kept | /health ok live+llm; backfill 3; caps per_unit_hour=3 | `rm …/90-deploy-t273u.conf; daemon-reload; restart gaia-cortex` → bd6fbe6 via 87-live-fixes.conf |
| Hermes gateway | **SKIPPED (already-current for prod)** | live `hermes-gateway-aa77864`; RSS ~1.8 MiB; 7117d54 is MockProvider soak on integration-5 (not ancestor; no prod --mock) | /health 200 | n/a |
| Hermes notify | **BLOCKED** | no TELEGRAM_* / webhook on VPS hermes `.env`; channel_directory empty | n/a | needs Mate bot token + chat id via secret form |
| Console Rust V2 | **BLOCKED** | no Rust console serve binary; `console-rust-serve` still audit tip; live Node console-dist/srvx :8080 left untouched | root 302 | n/a — do not cut over |
| Backup timer | **DEPLOYED** | `galion-backup-98466ab run`; drop-in `galion-backup.service.d/90-deploy-t273u.conf`; sandbox + one live run OK; offsite encrypt OK | oneshot success; timer active | `rm …/90-deploy-t273u.conf; daemon-reload` → `.sh` |
| HADES/H1–C5 | **BLOCKED** | admin/custody keys missing | n/a | Mate must provide — do not invent |
| Cloudflare / hoff-23 CF rule | **NOT DONE** (by design) | nerve flood code deployed instead | n/a | RULES forbid CF DNS/HADES edits |

## Cortex llm-advisor note
First install of release artifact `gaia-cortex-73583ab` (no llm-advisor) failed exit 2; immediately rolled back to bd6fbe6; rebuilt with `--features llm-advisor`; redeployed as `gaia-cortex-73583ab-llm`.

## Console FP watch
Cortex live actions remain ON. No console.service stop/restart performed this wave. If a future console cutover runs: mask/dry-run `85-live-actions.conf` first, cut over, restore live.
