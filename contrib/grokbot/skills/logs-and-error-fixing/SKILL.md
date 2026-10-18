---
name: logs-and-error-fixing
description: Use when a VPS component (Galleon, Galleon1, Obsidian, OpenMind) is failing, erroring, slow or down — to collect logs, find the root cause, and (only with explicit approval) apply a minimal fix with backup and rollback.
---

# Logs and error fixing

## Purpose
Evidence-first troubleshooting: collect → diagnose → propose → (approved) fix → verify → record.

## Prerequisites
- `vps-ssh-access` works; `stack-map.md` tells you which unit/container/dir belongs to the failing component.
- Any mutating step needs Mate's explicit approval for that specific change (and Slavik's, if it's his production service).

## Procedure
1. Scope: which component, since when, symptom. Run `~/workflows/vps/bin/health-check.sh`.
2. Collect logs (read-only) into `data/logs/<ts>/`:
   `~/workflows/vps/bin/collect-logs.sh --since "2 hours ago"`
   Narrow: `--unit <unit> --container <ctr> --lines 500 --grep 'error|exception|traceback|refused|timeout|oom'`.
3. Diagnose. Typical patterns and checks (all read-only):
   - Crash loop / exit code → `systemctl status`, `docker inspect -f '{{.State.ExitCode}} {{.State.OOMKilled}}'`.
   - OOM → kernel section of the bundle, `free -h`, container memory limits.
   - Disk full → `df -h`, `du -xh --max-depth=1 /var/log /var/lib/docker | sort -h | tail`.
   - Port conflict / not listening → `ss -tlnp`.
   - 502/504 at proxy → nginx error.log + is the upstream port listening + `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:<port>/` from the VPS.
   - Config/env error after change → compare with `data/changes/*` records and `git -C <dir> log -5 --stat`.
   - Cert expired → `certbot certificates`.
   - DB connection refused → DB service status, credentials changed (look for errors, don't print secrets).
4. Write a diagnosis: root cause (or top hypotheses with evidence), impact, proposed minimal fix, risk, rollback.
5. STOP and get approval. Then apply with the guarded tools only (they dry-run by default, back up, and record rollback):
   - Restart: `~/workflows/vps/bin/service-action.sh --unit <u> --action restart` (review) → add `--apply`.
   - Edit config: `edit-remote-file.sh --path <p> --fetch` → edit local copy → `edit-remote-file.sh --path <p> --from <local> --validate '<e.g. sudo nginx -t>' --apply` → then reload/restart via service-action.
   - Code fix → use `deploy-updates`, never hand-edit app code on the VPS.
6. Verify: `health-check.sh`, re-run `collect-logs.sh --since "10 minutes ago"` and confirm the error stopped.
7. Record in `stack-map.md` change history (link the `data/changes/*.md` record).

## Safety
- Never delete logs/data to "free space" without approval; propose `journalctl --vacuum-time`/log rotation instead.
- No `docker system prune`, `rm -rf`, DB writes, firewall changes, or package upgrades without explicit approval.
- One change at a time; verify between changes. If it gets worse, roll back immediately using the recorded steps.
- Logs may contain secrets/PII: summarize, don't paste whole bundles outside the box.

## Report
Symptom, timeline, root cause + evidence (short quoted log lines), action taken (or proposed, awaiting approval), verification result, rollback instructions, log bundle path.
