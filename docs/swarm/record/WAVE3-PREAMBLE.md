<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/WAVE3-PREAMBLE.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# Wave 3 preamble (2026-10-20 22:50 CEST +02:00) — read after RULES.md
- You are one of ~15 agents on ONE 8-vCPU / 15 GB box. Memory is tight (~3.5 GB free at launch).
  Before every cargo build/test: run `free -m`; if "available" < 1500 MB, wait 60 s (AwaitShell, not sleep loops) and re-check.
  Build with `nice -n 10 cargo ... -j 2` (NOT -j 4) and your own CARGO_TARGET_DIR=/workspace/swarm/target/<slug>, CARGO_INCREMENTAL=0.
- Owner's direction (Slavik): Rust only, language and program; make it build; make it great. TOP PRIORITY RULE in RULES.md applies to your report.
- Planning phase: NOTHING is deployed, installed, restarted or written on the VPS. VPS = read-only GETs / status reads only.
- Owner decisions stay owner decisions: if your task hits one, write it up as a decision item with options and evidence; do not decide it yourself.
- Branch base: GitHub main 13a296ce unless your task names another base. Push only your own swarm/<slug> via push-branch.sh. Never main, never force, no PRs.
- If you are interrupted and resumed, continue from your worktree state; don't restart from scratch.
- Final report: branch + head SHA (pushed?), what was done, real-test evidence, deps + licences, open issues, owner decisions. Short and factual.
