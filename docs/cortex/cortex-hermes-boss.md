<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Cortex -> Hermes boss handoff (swarm/cortex-hermes-boss)

Mate, 2026-10-21 22:50 +02:00 (t217): the cortex acts for real; incidents its
fixed rules cannot settle go to the Hermes boss agent, never to DeepSeek for
coding. Hermes notifies Mate first and changes nothing until he approves.
Mate t218 (22:52 +02:00): Hermes total ceiling $20.

```text
nerve event -> cortex rules -> restart (live, via nerve POST /actions)
                            \-> escalation (R0, R2, R3-cap, R4, R7/R8/R99 high, R10, R11 critical,
                                refused/failed restart)
                                 -> Handoff::offer: once per incident, once per subject+rule per hour,
                                    max 6/h, none while BUDGET-STOP exists
                                 -> /var/lib/gaia-cortex/hermes-inbox/hoff-<t>-<n>-<incident>.json (atomic)
                                 -> gaia-hermes-handoff.path (inotify PathExistsGlob, no timer)
                                 -> gaia-hermes-handoff.service: gaia-cortex hermes-dispatch (oneshot)
                                      claim file (rename into <inbox>/processing, same mount) -> $20 check (sum of hermes-rs rs-spend.json sessions
                                      + $1 reserve) -> ONE call: Hermes /v1/chat/completions, model boss
                                      (claude-opus-5.5), system = cortex-escalation SKILL.md, no retry
                                      -> /var/lib/gaia-hermes-handoff/proposals/<id>.md + proposals.jsonl
                                      -> Telegram to Mate if TELEGRAM_BOT_TOKEN + TELEGRAM_CHAT_ID are set
```

- The cortex never calls Hermes or OpenRouter itself; its own model calls stay
  DeepSeek on River (triage). The dispatcher is the only caller, one call per
  incident, no retries, no loop.
- Hermes on the VPS has no coding tools (`HERMES_RS_CODING_WORKTREE` unset) and
  a read-only `HERMES_HOME`: it cannot change code even if asked.
- At the ceiling the dispatcher writes `BUDGET-STOP` into the inbox; the cortex
  then counts `handoffs_budget_stopped` and writes nothing. Re-enable = Mate
  raises `HANDOFF_BUDGET_USD` (hard max 20 in code) and removes the file.
- Stats: `GET /stats` -> `handoffs*`, `hermes_handoff{enabled, sent_last_hour,
  incidents_handed, budget_stopped}`; `GET /health` -> `hermes_handoff`.

Deploy files: `deploy/gaia-cortex/` (`86-hermes-boss.conf`,
`gaia-hermes-handoff.{path,service}`, `cortex-escalation.SKILL.md` ->
`/opt/galion/hermes/skills/galion/cortex-escalation/SKILL.md`).

## Real test record (VPS, 2026-10-21 +02:00)

- 23:05:52 refusal path, temp inbox, `HANDOFF_BUDGET_USD=1`: `refused-budget`
  ("Hermes spend $1.1415 + reserve $1.00 > ceiling $1.00"), `BUDGET-STOP`
  written, zero calls.
- 23:06 first deploy (e57b946): claim used `rename` into the state dir, which
  is a separate systemd bind mount -> EXDEV, `claim-failed`; the `.path` unit
  re-fired 24 times in seconds, hit `TriggerLimitBurst=24` and failed. Zero
  paid calls (claim happens before any call). The cortex saw the failed unit
  and handed it off (R4-unknown-unit): the chain worked end to end on a real
  failure. Fixed: claimed files stay under `<inbox>/{processing,done,refused,failed}`.
