<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Staged Hermes skills (2026-10-29 CET +01:00)

New SKILL.md files for the live skills dir `/opt/galion/hermes/skills`. They are staged
here because the live dir is not written by the swarm. Paths mirror the live layout.

| Staged file | Live file | Why |
| --- | --- | --- |
| addy/planning-and-task-breakdown/SKILL.md | same path | live file is empty (0 B) |
| addy/spec-driven-development/SKILL.md | same path | live file is empty (0 B) |
| addy/test-driven-development/SKILL.md | same path | live file is empty (0 B) |
| local/hearth/SKILL.md | same path | stale: "you are not Grok", four-sentence rule, no file tool |
| local/grok-session/SKILL.md | same path | stale: port 11434 "cannot think", four-sentence rule |
| galion/local-primary/SKILL.md | same path | stale: laptop qwen primary; live default is Grok 4.7 via OpenRouter |
| tools/agent-tools/SKILL.md | new | how to use the restored tools (terminal, files, browser, vision) |

Install only with the agent-tools deploy (docs/hermes-agent-tools.md): back up the live
files first, then copy. Skills that call `/workspace/vision/venv/bin/python` or other
Python scripts (playwright-see, mesh, survival, ...) still need Python, which the
Rust-only stack does not run; `tools/agent-tools` gives the Rust replacement
(browser + vision_analyze) for the page-looking ones.
