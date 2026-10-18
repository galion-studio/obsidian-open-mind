# Workflow: deploy-release
**Trigger:** Mate asks to deploy/update a component (name + version/ref), or a fix from incident-triage needs shipping.
**Skills:** service-health-check → deploy-updates → logs-and-error-fixing (verify) → (on failure) rollback
**Mutating:** yes, after Gate

## Steps
1. **Preconditions** — component's "How to deploy", dir, method, unit/container are filled in `stack-map.md` (not TBD). Else stop and ask.
2. **Pre-health** — `~/workflows/vps/bin/health-check.sh` (note exit code).
3. **Dry run** — `~/workflows/vps/bin/deploy.sh --component <c> --dir <dir> --method <git|compose|git+compose> [--ref <r>] [--build] [--unit <u>]`
4. **Gate** — show Mate the dry-run (current SHA → target, dirty files, plan, backup location); get explicit go (and Slavik's for production timing).
5. **Apply** — same command + `--apply` (type `yes`). Backups + rollback record go to `data/changes/<ts>-deploy-<c>.md`.
6. **Verify** — post-health is printed by the script; also `collect-logs.sh --since "10 minutes ago" --unit/--container ...` and hit the component URL from the VPS (`vps-ssh.sh -- 'curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:<port>/'`).
7. **On failure / regression** (script exit ≠ 0, or exit 3 = health worse) — report to Mate and roll back per the record's "Rollback" section (approval unless pre-agreed), then verify again.
8. **Record** — update component SHA/version + change history in `stack-map.md`.

## Report
Component, from→to, backup path, result, pre/post health, verification evidence, rollback summary, record path.
