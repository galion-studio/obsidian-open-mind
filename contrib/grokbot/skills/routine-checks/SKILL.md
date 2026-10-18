---
name: routine-checks
description: Use to design or run recurring monitoring of Slavik's VPS (daily/hourly health, weekly deeper review) and to decide when and how to alert. Does not install cron jobs by itself.
---

# Routine checks

## Purpose
Catch problems early with a fixed cadence and clear alert rules, using read-only checks only.

## Prerequisites
- `stack-mapping` completed and `services.list` filled (otherwise only generic checks run).
- Alert channel agreed with Mate (set `VPS_ALERT_CMD` in `config.env`, e.g. a command that sends a message; until then alerts are files only).

## Schedule (recommended — not installed)
| Cadence | What | Command |
|---|---|---|
| Every 15–30 min (optional, once stable) | Liveness: components up, ports listening | `routine-check.sh` |
| Daily 08:00 Europe/Zagreb | Full health + log error volume + cert expiry | `routine-check.sh --collect-logs-on-fail` then workflow `daily-health-check` |
| Weekly (Mon 09:00 Europe/Zagreb) | Deeper review: re-run `map-stack.sh`, diff against stack map; disk growth trend; pending updates (`apt list --upgradable 2>/dev/null \| wc -l`), reboot-required, cert expiry <30d, backup age, old `~/vps-backups` size | workflow `daily-health-check` weekly section |
| Monthly | Review alert noise / thresholds, clean-up proposals (needs approval) | manual |

If later asked to schedule it, example (DO NOT install without approval):
`*/30 * * * * $HOME/workflows/vps/bin/routine-check.sh >> $HOME/workflows/vps/data/health/routine.log 2>&1`
or a systemd user timer. Cron runs in the box's TZ (Europe/Zagreb).

## Procedure (each run)
1. `~/workflows/vps/bin/routine-check.sh [--collect-logs-on-fail]`
2. It runs `health-check.sh --quiet`, diffs with the previous result (NEW / RECOVERED / CURRENT), writes `data/health/ALERT-<ts>.txt` when: any FAIL, any NEW warn/fail, or SSH unreachable (exit 255). Pipes the summary to `VPS_ALERT_CMD` if set.
3. Weekly: run `map-stack.sh`, compare service/port/container lists with `stack-map.md`; flag drift (new/removed services, changed ports, SHA changed without a change record).

## Alert rules
- **Page now:** SSH unreachable twice in a row; any component FAIL (svc/ctr/port/http); disk ≥95%; OOM kills.
- **Same-day notice:** disk ≥85%; cert <14d; container exited; failed units; error volume spike; reboot required.
- **Weekly digest:** recovered items, drift, pending updates, backup sizes.
- De-duplication: the script alerts on every run with a FAIL and on any NEW warn; WARNs that persist unchanged are not re-alerted. RECOVERED items are listed in the next summary.
- Alerts go to Mate (and Slavik if agreed). Sending any message is externally visible — only through the channel Mate approved.

## Safety
Read-only. Routine checks never restart or fix anything automatically; they hand off to `incident-triage`.

## Report
Timestamp (Europe/Zagreb), verdict, NEW/RECOVERED/CURRENT lists, alert file path, whether an alert was sent and where.
