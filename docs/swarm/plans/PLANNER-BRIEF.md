<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/plans/PLANNER-BRIEF.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# Sector planner brief (2026-10-20 20:38 CEST +02:00)
You are a PLANNING-ONLY agent for PROJECT 42 / Gaia (Rust platform, repo galion-studio/obsidian-open-mind, main 13a296c).
Hard rules: no code changes, no commits, no pushes, no builds, no VPS writes or deploys (read-only ssh/GET allowed only if needed:
`ssh -i ~/.ssh/id_ed25519_vps -o IdentitiesOnly=yes root@<vps>`), never print secrets, no Python
(prefix every shell command with `export PATH=/workspace/swarm/bin:$PATH;`).

Read first: /workspace/swarm/RULES.md, /workspace/swarm/WAVE2.md, /workspace/swarm/CALL-RECORD.md, /workspace/swarm/CALL-RECAP.md,
/workspace/review-20261020/*.md, and docs in /workspace/swarm/wt/integration-1 (head 8fd26f6, all 20 wave-1 branches merged:
docs/STATUS.md, docs/main.md, docs/launch/*, docs/real-tests/*, docs/integration/integration-1.md). Launch deadline Oct 31.

Owner context: Slavik set the agent ceiling to 250 with auto-accept and wants everything done at once, with groups of ~50 agents per task.
Observed reality: tonight a burst of 15 lost 11 to the request limit; 16 concurrent agents have survived since 20:34. So plan BOTH
(a) the target plan (sessions, tasks, ~50-agent groups where a task can genuinely use them) and (b) a realistic schedule sized to 16
concurrent agents. For each task say honestly how many agents can usefully work it in parallel (most code tasks can't use 50; say so),
what splits it into parallel units (per crate, per service, per fuzz target, per test scenario...), and dependencies/merge order.

Output: write /workspace/swarm/plans/<sector>.md with: sector scope; current state with branch SHAs; numbered sessions (S1, S2...),
each with goal, tasks, per-task agent count (target and useful max), parallel units, inputs/dependencies, success criteria with real-life
tests, items needing Slavik's approval; a totals table (sessions, tasks, target agents, useful agents). Keep it factual; cite files.
Report back: file path, session count, task count, total target agents, total useful agents, top 5 risks.
