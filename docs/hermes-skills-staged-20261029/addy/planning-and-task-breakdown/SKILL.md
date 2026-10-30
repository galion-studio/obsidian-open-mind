---
name: planning-and-task-breakdown
description: Turn a request into a short ordered plan of small, checkable tasks before you touch files or run commands.
---
<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Planning and task breakdown

Use this before any job that needs more than two tool calls.

1. Restate the goal in one sentence and name what "done" looks like (a file, a passing test, an answer).
2. List what you do not know yet. Find it out first, read-only: `list_files`, `search_files`, `read_file`, memory search.
3. Break the work into 3 to 8 tasks. Each task changes one thing and has a check
   (a command that passes, a file that exists, a page that shows the text).
4. Order them so each task leaves the workspace working. Risky or irreversible steps go last and need the admin's yes.
5. Show the plan, then do task 1. After each task, run its check and say pass or fail before the next one.
6. If a check fails twice, stop, say what failed and what you tried, and ask.

Rules: work only inside the workspace. Never plan a step that writes SOUL.md, config.yaml,
the gate, secrets or /etc/galion; those are locked and the tools refuse them. Do not claim a
task is done without its check output.
