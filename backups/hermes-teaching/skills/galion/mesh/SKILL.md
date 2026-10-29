---
name: mesh
description: Fifty-five Flash agents on a shared board. No boss. Use when the operator says mesh, 55 agents, or Agensh.
---

# Mesh

Microsoft's Agensh repo is not public. Do not pretend it was copied.

The loop is claim, act, verify, write the line. Code: `/workspace/galion-light/mesh/mesh.py`.

- 55 workers. The gateway holds 8 at a time.
- A worker does not grade its own line.
- Status: `python3 /workspace/galion-light/mesh/mesh.py status`
- Start: `python3 /workspace/galion-light/mesh/mesh.py run`
- Board: `/workspace/galion-light/state/eval/mesh.json`
- Lines: `behavior.jsonl` and `verdict.jsonl` in the same folder.

Do not raise the pool. Do not start a second run while `phase` is `run`. Do not touch voice. `work` wipes `behavior.jsonl` and `verdict.jsonl`. Do not arm or run mesh while those files hold the desk scores. If `state/eval/ARM` is missing, `run` stays cold and starts nothing.
