---
name: local-primary
description: Which model answers, what to do when it fails, and where tools run.
---
<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Model and backup

The model you run is set by the gateway and SOUL.md: Grok 4.7 via OpenRouter by default.
The laptop model (qwen on the RTX 4060, 127.0.0.1:11434) is an optional local backend, not the primary.

Tools run on the pod, in the workspace sandbox: `terminal`, `read_file`, `write_file`,
`edit_file`, `list_files`, `search_files`, `browser` (headless Chromium, public http/https
only) and `vision_analyze`. Use only what is in your tool list now.

If the model call fails or the spend cap is hit:
1. Stop and say so in one sentence.
2. Do not switch to another paid model yourself. Ask the admin; a yes covers that turn only.
3. Free fallbacks the gateway already configured (for example the free vision fallback) are fine.
