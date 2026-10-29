<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes skills + persona (2026-10-29)

Branch `swarm/hermes-skills-persona-20261029`, based on the live build 5c7bad5f
(`swarm/memory-integration-20261029`).

## What changes
- `crates/hermes-rs/src/skills.rs`: skill loader for `$HERMES_HOME/skills` plus config
  `skills.external_dirs` (symlinks followed, dot dirs skipped, a canonical dir is read
  once, first name wins). It adds the tools `skills_list` and `skill_view`, both read-only.
  `skill_view` refuses `..`, absolute paths and anything outside the skill folder.
  This follows the upstream hermes-agent skills design (MIT).
- System prompt order: SOUL, then `## Runtime` (the model and provider of this turn and
  the real tool list), then the `## Skills` index (one line per skill, at most 6000 chars),
  then channel, config prompt, client, persona.
- Skill flags: `[python]` means the script cannot run in hermes-rs (reference only).
  `[live-change]` means the skill deploys or restarts; SOUL `## Changes` overrides it.
  `[rust: x]` means a Rust tool replaces the script.
- Python ports (MIT reuse of the hermes-agent skill scripts):
  - `arxiv_search` (`crates/hermes-rs/src/arxiv.rs`) ports `research/arxiv/scripts/search_arxiv.py`.
    It is a public Atom API GET through the system curl, https only, 20 s timeout.
  - `citations` (`crates/hermes-rs/src/citations.rs`) ports
    `research/grounded-citations/scripts/sources.py`: add, quote (verbatim only), list,
    render, verify, reset. There is one ledger per session in `/var/lib/hermes-rs/citations/`.
- `SOUL.md` (this folder) fixes the conflicts listed in
  `/workspace/swarm/reports/skills-persona-20261029.md`. It adds `## Skills`, and it has
  no public IPs.
- Switches: `HERMES_RS_SKILLS=0` turns off the skill tools, the Runtime block and the
  index. `HERMES_RS_SKILL_TOOLS=0` turns off arxiv_search and citations.
  `HERMES_RS_SKILLS_DIRS=a:b` overrides the roots. `HERMES_RS_CITATIONS_DIR` sets the
  ledger folder.

## Not ported
- `web/blocked-page-recovery/scripts/recover_page.py`: hermes-rs has no web fetch tool,
  and SOUL keeps pages text-only. Porting it needs an owner decision on a fetch tool.
- `software-development/github/scripts/git-credential-token.py`: deliberately not ported.
  Hermes must not hold push credentials (gate design, step 4).
- `_hermes_home.py`: a helper only. It is replaced by the ledger dir setting.

## Rollback
- Prompt and skill tools only: add a drop-in with `Environment=HERMES_RS_SKILLS=0` and
  `HERMES_RS_SKILL_TOOLS=0`, then restart.
- Binary: remove `98-skills-persona.conf`, run `systemctl daemon-reload`, and restart
  hermes-rs. That goes back to `hermes-gateway-5c7bad5f`.
- SOUL: `cat /var/backups/skills-persona-20261029-<ts>/SOUL.md > /opt/galion/hermes/SOUL.md`.
  Using `cat` keeps the file mode. SOUL is read on every request, so no restart is needed.
