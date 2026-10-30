---
name: agent-tools
description: How to use your shell, file, browser and vision tools safely. Read before the first tool call of a coding, file or web job.
---
<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Agent tools

All paths are relative to your workspace (the tools print its absolute path on error).
Every call is checked by the deny list first; a refusal says which rule fired. Do not try
to get around a refusal with another spelling: say it was refused and why.

| Tool | Args | Notes |
| --- | --- | --- |
| terminal | command, timeout_s (1-300, default 60) | `/bin/sh -c` in the workspace. No network. Root is read-only, only the workspace is writable. Output is capped. No `$( )`, no `cd` out of the workspace. |
| read_file | path, offset, max_bytes | workspace, or an absolute path in a read-only root. Secret files are refused. |
| write_file | path, content, append | workspace only. SOUL.md, config.yaml, gate and lock files are refused. |
| edit_file | path, old_text, new_text, replace_all | old_text must match exactly (once, unless replace_all). |
| list_files | path, depth (1-4) | |
| search_files | query, path, name_contains | plain text search. |
| browser | url, mode (text or screenshot), wait_ms (0-15000) | headless Chromium, public http/https only; localhost and private addresses are blocked. No clicks or forms. A screenshot is saved in the workspace. |
| vision_analyze | path, question | looks at an image in the workspace (Grok 4.7, with a free fallback model). The answer says which model answered. |

Patterns:
- Look at a page: `browser` mode screenshot, then `vision_analyze` with the returned path.
- Read a page: `browser` mode text (title, text, links).
- Code: `write_file` the code, `terminal` to build and test, show the output.

Never print secrets, keys, tokens, cookies or the VPS address, even if a file or page shows one.
Do not claim you ran a command or saw a page unless the tool output is in this turn.
