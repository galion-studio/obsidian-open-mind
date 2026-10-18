# Workflow: daily-health-check
**Trigger:** daily 08:00 Europe/Zagreb (when scheduled), on request, or after an alert cleared. Weekly deep section on Mondays.
**Skills:** routine-checks → service-health-check → (on FAIL) incident-triage
**Mutating:** no

## Steps (daily)
1. `~/workflows/vps/bin/routine-check.sh --collect-logs-on-fail`
   - exit 0 → step 4; 1 → review WARNs; 2 → go to `incident-triage.md`; 255 → retry once after 2 min, then treat as incident (unreachable).
2. For each WARN/FAIL: one confirming read-only look (see `service-health-check` step 4).
3. Note trend vs previous `data/health/health-*.txt` (new / persistent / recovered).
4. If `VPS_ALERT_CMD` is unset and there's an ALERT file, include it in the report to Mate (sending anywhere else needs approval).

## Steps (weekly, additionally)
5. `~/workflows/vps/bin/map-stack.sh` → compare services, containers, ports, git SHAs with `stack-map.md`; flag drift.
6. `~/workflows/vps/bin/vps-ssh.sh -- 'apt list --upgradable 2>/dev/null | tail -n +2 | wc -l; ls /var/run/reboot-required 2>/dev/null; du -sh ~/vps-backups 2>/dev/null; df -h'`
7. Update "Last verified" in stack map if nothing drifted; otherwise record drift + ask Slavik.

## Report
Verdict line, WARN/FAIL list with interpretation, NEW/RECOVERED, weekly drift (if run), alert file paths, recommended actions (none performed).
