---
name: service-health-check
description: Use to check whether Slavik's VPS and its components (Galleon, Galleon1, Obsidian, OpenMind) are healthy right now — on request, before/after a deploy, or as part of triage.
---

# Service health check

## Purpose
A fast, read-only, repeatable health verdict: OK / WARN / FAIL per check.

## Prerequisites
- `vps-ssh-access` works. Ideally `stack-mapping` done so `services.list` has component-specific checks (otherwise only generic host checks run).

## Procedure
1. Run: `~/workflows/vps/bin/health-check.sh` (full) or `--quiet` (only WARN/FAIL). Thresholds: `--disk-warn 85 --disk-fail 95`.
   Exit code: 0 OK, 1 WARN, 2 FAIL, 255 SSH error. Result saved to `data/health/health-<ts>.txt`.
2. Generic checks: uptime/reboot-required, load vs CPUs, memory/swap, disk & inodes, failed systemd units, all docker containers (unhealthy/restarting = FAIL, exited = WARN), journal err+ volume last hour, Let's Encrypt cert expiry <14d.
3. Component checks from `services.list`: systemd active, container running/healthy, port listening, HTTP 2xx/3xx from the VPS itself, path exists.
4. For each FAIL/WARN, take one confirming read-only look, e.g.
   `vps-ssh.sh -- 'systemctl status <unit> --no-pager -l | head -40'`, `vps-ssh.sh -- 'docker logs --tail 50 <ctr>'`, `vps-ssh.sh -- 'df -h; du -xsh /var/log /var/lib/docker 2>/dev/null'`.
5. Compare with the previous `data/health/health-*.txt` to say whether it is new or persistent.
6. If anything FAILs → hand over to `logs-and-error-fixing` (workflow `incident-triage`).

## Safety
Read-only. Do not restart anything as part of a health check.

## Report
One-line verdict (OK/WARN/FAIL counts), list of WARN/FAIL lines with one-line interpretation, new vs persistent, file path, suggested next step.
