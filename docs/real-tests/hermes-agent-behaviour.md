<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes agent tools: zero-spend behaviour test (box only)

`scripts/real/hermes-agent-behaviour.sh` runs the real hermes-gateway with the live tool layout
(drop-ins 99j/99k: guard workspace = files root = tools workspace, files write on, 4096 MB) under a
temp folder, against the scripted mock model. No OpenRouter call, no key, cost $0.

Checks (29, or 30 with script mode): every live tool is offered; terminal runs in the files root;
read/write/edit/list/search; galion-files list/read; rm -rf /, curl, /etc/shadow and a ../ write are
refused; nothing escapes the root; timeout_s is honoured; a folder outside the workspace is hidden;
a workspace shell script is refused (strict) or runs (EXPECT_SCRIPT_MODE=1 with a script-mode build);
a script that calls curl is refused in both; no key reaches the mock; canary intact.

Runs on 2026-11-01 (CET):
- 12:59 base 4ce482de (= live 46877fd8 + test-only fix), strict: 29/29 PASS.
- 13:00 swarm/hermes-script-run-20261101 d493a9cb, HERMES_RS_GUARD_SCRIPT_MODE=1: 30/30 PASS
  (count.sh wrote 3 lines; bad.sh refused S7-network).
- 13:00 same build, switch off: 29/29 PASS (script refused S5-expand, as live today).
