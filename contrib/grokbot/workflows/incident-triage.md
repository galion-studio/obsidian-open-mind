# Workflow: incident-triage
**Trigger:** alert/FAIL from health or routine check, or a report that Galleon/Galleon1/Obsidian/OpenMind is down/slow/erroring.
**Skills:** vps-ssh-access → service-health-check → logs-and-error-fixing → (if code fix) deploy-updates
**Mutating:** only after Gate B

## Steps
1. **Reachability** — `~/workflows/vps/bin/vps-ssh.sh --check`. Unreachable → report to Mate immediately (can't fix network/host from here; Slavik/provider console needed).
2. **Scope** — `~/workflows/vps/bin/health-check.sh`; identify component(s) from `stack-map.md`; note start time (Europe/Zagreb).
3. **Evidence** — `~/workflows/vps/bin/collect-logs.sh --since "<start - 15 min>" [--unit U] [--container C]`.
4. **Recent changes?** — `ls -t ~/workflows/vps/data/changes/ | head`; `vps-ssh.sh -- 'git -C <dir> log -3 --format="%h %ci %s"'`.
5. **Diagnose** — patterns in `~/skills/logs-and-error-fixing/SKILL.md` step 3. Write root cause/hypotheses + evidence.
6. **Gate A — report first.** Send Mate: impact, cause, proposed minimal fix, risk, rollback. If urgent and Mate pre-approved "restart on crash" for that component, that's the only exception.
7. **Gate B — approved fix**, guarded tools only:
   - restart: `service-action.sh --unit|--container <n> --action restart` → review → `--apply`
   - config: `edit-remote-file.sh --fetch` → edit → `--from ... --validate ... --apply` → reload via service-action
   - recent bad deploy: rollback per `data/changes/<ts>-deploy-*.md`
   - code bug: `deploy-release.md`
8. **Verify** — `health-check.sh`; `collect-logs.sh --since "10 minutes ago"`; error gone?
9. **Close** — change history row in stack map; add a health check to `services.list` if this failure wasn't caught automatically.

## Report
Timeline, impact, root cause + evidence, action (taken / proposed), verification, rollback steps, follow-ups, file paths.
