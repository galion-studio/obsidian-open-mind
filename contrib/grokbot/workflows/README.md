# Workflows — Slavik's VPS
Markdown workflow definitions. Each has: Trigger, Skills chained, Steps (exact commands), Gates (where approval is needed), Outputs, Report.
Skills live in `~/skills/`; scripts in `~/workflows/vps/bin/`; settings in `~/workflows/vps/config.env`.

| Workflow | When |
|---|---|
| first-connect-and-map.md | Host/user just provided; never connected before, or major change on VPS |
| daily-health-check.md | Daily (and weekly deep section) or "is everything OK?" |
| incident-triage.md | Something is down/erroring, or an alert fired |
| deploy-release.md | Asked to deploy/update/roll back a component |
