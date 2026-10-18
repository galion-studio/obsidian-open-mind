---
name: deploy-updates
description: Use when asked to deploy, update, or roll back a component (Galleon, Galleon1, Obsidian, OpenMind) on Slavik's VPS. Always dry-run first; applying needs explicit approval.
---

# Deploy updates

## Purpose
Safe, repeatable deploys with pre/post health checks, backups, and a written rollback.

## Prerequisites
- `stack-map.md` has, for the component: dir, how it runs (git / compose / systemd), unit/container names, and the deploy procedure confirmed by Slavik. If "How to deploy" is TBD → stop and ask.
- Explicit approval from Mate for this deploy (component, ref/version, time window). Coordinate with Slavik for production.
- Health is not already FAIL (or the deploy is the fix and that's understood).

## Procedure
1. Dry run (read-only — shows branch/SHA, dirty files, remote ref, compose state, size, plan):
   `~/workflows/vps/bin/deploy.sh --component <name> --dir <dir> --method git|compose|git+compose [--ref <tag/branch>] [--build] [--unit <systemd-unit>]`
2. Review: dirty working tree on the VPS (Slavik's hot-fixes?) → stop and ask; the script refuses unless `--allow-dirty`. Check what changes between current SHA and target (`git log <cur>..<target>` from the repo origin if accessible).
3. Apply after approval: same command + `--apply`, type `yes`. The script:
   pre health-check → backup to `~/vps-backups/<component>-<ts>/` on the VPS (git HEAD + branch + uncommitted diff, compose images/config, tarball of dir excluding .git/node_modules/venv; skip tarball with `--no-tarball` if huge) → `git fetch` + checkout/`pull --ff-only` and/or `docker compose pull && up -d [--build]` → optional `systemctl restart <unit>` → post health-check.
   Record + rollback steps: `data/changes/<ts>-deploy-<component>.md` (log in `.md.log`).
4. Verify beyond health: hit the component's URL/endpoint, check `collect-logs.sh --since "10 minutes ago" --unit/--container ...` for new errors.
5. If deploy failed or health got worse (script exits non-zero/3): follow the Rollback section in the record (git checkout old SHA, compose up with old images, restore tarball, restart, verify). Rollback is itself a change → confirm with Mate unless pre-agreed for failed deploys.
6. Update `stack-map.md`: new SHA/version, change history row.

## Safety
- Never deploy without a dry run and approval; never `git reset --hard`/`push --force`, never edit code in place on the VPS.
- Don't run migrations or DB-changing steps unless the confirmed procedure says so; back up the DB first (procedure to be defined per component in stack map).
- Remote backups consume disk: check `df -h` first; note backup path for later cleanup (cleanup also needs approval).

## Report
Component, from→to (SHA/tag/image), backup path, steps run, pre/post health, verification, rollback command summary, change record path.
