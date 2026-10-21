<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# The big picture: PROJECT 42 / Gaia

Written 2026-10-20 23:31 CEST (+02:00) by swarm/swarm-record; **refreshed 2026-10-21 06:41 CEST (+02:00) by swarm/processes-refresh (slot 142)** with wave 4, the first VPS deployment and the new standing rules. This file explains what the work is for. `PROCESSES.md` lists each piece.

## The vision

> Discover the universe together with humans. Leave with more than you came in with.

Gaia is the Rust platform behind Galion. The goal is simple to say: **make Gaia build, and make it great**. "Great" means it is honest about its own state, it is safe with Slavik's keys and money, and it is beautiful to use. The research, the console (Hermes and Gaia), and every sector should be things people enjoy opening.

Slavik's top-priority rule (2026-10-20 20:56 CEST) applies to every tool and report: good company and productive conversations, the kind where you leave with more than you came in with. In practice, every page and every report should give substance and a useful next step, with no filler.

## Standing rules that shape everything

- **Rust only**, as the language and in every program. No Python, not even as a probe. Non-Rust parts get rewritten as proper Rust (Slavik, 20:41 CEST).
- **Owner decisions stay with the owner.** Agents write options and evidence. Slavik decides.
- **Production is protected.** The VPS working copy `/opt/galion/openmind` stays at `2f5a730`. Live services are not stopped, restarted or reconfigured by agents. Cutovers are separate, gated steps.
- **No money moves.** Revolut is in scope for the Hermes Rust V2 design (ban lifted 22:56 CEST), but no orders, transfers or top-ups without a separate written approval.
- **Zagreb time with an offset**, never `Z`, also in commit dates (GitHub shows +0200). SPDX MIT headers. Allowed licences only.
- **The sandbox runs only on the VPS** (04:29 CEST, Oct 21): as an ordinary user in a throwaway `/var/tmp/sandbox-test-<ts>` folder, cleaned up after. Box tests leave out openmind-sandbox and cgroup-detect.
- **No skipping, real data** (04:38 CEST, Oct 21): every agent tests on the real VPS, not only on mocks, and watches the live nerve (8670) while the cortex is not live.

## Gaia: 9 functions, 52 systems

Gaia describes the platform as 52 systems owned by 9 functions: HADES (approvals and safety), POSEIDON (compute queue and spend caps), DEMETER, ARTEMIS (the boss node, Hermes), AETHER (the senses: nerve, voice), ELEUTHIA, MINERVA (watching and security), HEPHAESTUS (build and release), and APOLLO (teaching and docs). A test fails if any crate has no owner (`gaia-land`).

## The 7 sectors

Hermes (`hermes-core`) is the single coordinator over seven sectors. Each sector has a lead, a plan in `docs/swarm/plans/`, and a cap on how many agents it may run:

| Sector | What it covers |
|---|---|
| gaia | Gaia core, ownership, POSEIDON, HADES gate, heal-tick, LAN routing |
| nerve | The live health nervous system (nerve 8670, galion-health), push events from services |
| cortex | Sensors and judgement on top of nerve: sshd, drift, connection rate, process owners, advisories |
| vps-ops | Units, deploys, rollback, tunnels, console, backups |
| integration-ci | Merging, CI, repo-check, secret-scan, releases |
| security | Fuzzing, SAST, HADES red team, SEC-1..5 runbooks, licences |
| docs-whitepaper | The white paper, session history, teaching docs |

Totals from the seven planners: 47 sessions, 200 tasks, 645 target agents, 407 usefully parallel.

## Benchmarks and Galion OS

- **Benchmarks** (`plans/benchmarks.md`): banking, Wall Street and vending machine. All three score 100 on mocks, paper trading and simulation (`bench-scoreboard f2ff169`), and 100/100/100 again when Hermes runs them through its tool layer, scripted and with a mock LLM (`bench-hermes 457ef74`). Wave 4 added adversarial sets (banking 38/38 refusals, Wall Street 212/212, a harder vending level) and a Hermes `bench` sector. Forbidden money calls counted: 0. The run with the live paid model waits on Slavik (H1).
- **Galion OS** (`galion-os-staging/`): a secure, private, anonymous ARM sandbox beside Slavik's normal Android phone. It holds the HADES key, the phone link to Hermes, and the benchmark surface. It is in the planning stage. Nothing gets built until Slavik answers the acceptance questions.

## How the swarm works

1. Slavik sets direction, by voice or typed, and Mate relays it.
2. Planners split each sector into sessions and tasks. Coding agents each take one task on their own `swarm/<slug>` branch from main `13a296c`.
3. Each agent proves its work with **real-life tests**: the release binary on its own box port range, hit with real requests, and read-only comparison against the live VPS where that helps.
4. Branches go to GitHub only through `push-branch.sh`. The token stays on the VPS, only `swarm/*` branches are allowed, and nothing is force-pushed.
5. Integration branches merge the work and run the whole workspace: `integration-2` 999 tests, `integration-3` and `integration-4` 1262 passed / 0 failed. `integration-4-squash` is the same tree on main with no VPS IP anywhere in its history.
6. Slavik merges to main and approves cutovers. Each cutover is one service at a time with rollback, rehearsed first (`deploy-rust`: 2-4 ms swaps).
7. Slavik allows up to 250 agents (groups of 50); the request limit lets far fewer run at once (often 3-6, at best about 18-20). A routine every 15 minutes starts up to 6 agents and relaunches each stopped slot as the only agent on its worktree. cargo builds queue through `cargo-slot` (max 3 at once). Starts, stops and counts go into the call record.
8. Everything important is kept in three places: GitHub (`swarm/*` branches, including this record), the VPS archive (`/opt/galion/archive/swarm-<timestamp>/`, with a git bundle and a sha256 manifest), and the box (working copy only).

## What happened overnight (2026-10-21, 00:52-06:41 CEST)

- **Wave 4 (slots 100-149):** 40 of 50 done and pushed by 05:55, all with real-life tests. The four stalled wave-2 branches were finished; all remaining shell real tests became Rust tests; CI steps now call `cargo xtask`; the cortex got its merged sensor set, score v2 and per-hostname edge rate; HADES got a typed-plan prototype (option B) that kept the red team at 0 bypasses, and a detectable ledger failure; the console gained 17 Rust routes without JavaScript. Slots 140-145 are running, 146-149 wait.
- **First real deployment on the VPS (04:21-04:48):** backup and verify first, then the Rust `galion-deploy`, the new `tuner` build, cloudflared config 666 -> 644, the five stray services stopped one at a time, and a **shadow nerve with the cortex** on 127.0.0.1:8672 (observe-only, no reflexes). The live nerve 8670 was not touched. Re-checked at 06:36: all of it is still in place and healthy.
- **What stopped a deploy:** the VPS sandbox test showed that a sandboxed program can see the host's process list (the LXC refuses the proc mount), and a later run found `link.env` world-readable from inside the sandbox. So nothing that runs code in the sandbox (Hermes tools) is deployed.
- **Live AI plan:** `docs/plans/live-ai-real-problems.md` (644c4f7) ranks real VPS problems for the live AI to diagnose: dead tunnel origins, galion-one-mouth stuck loading, tuner restarts, absent voice/gaia, the undeployed cortex. It needs H1 (paid model) to run.

## Merged vs waiting on Slavik (06:41 CEST, 2026-10-21)

- **Merged into main:** nothing. Main is `13a296c`.
- **Ready and waiting for Slavik's merge:** `swarm/integration-4` (`42de672`) or its history-clean twin `swarm/integration-4-squash` (`9b5f6ad`): integration-3 (integration-2 plus 8 branches) with the VPS IP removed. The 50 wave-4 and side-job branches are each listed in `PROCESSES.md`; three pushes are not logged yet (item 15 there).
- **Running on the VPS without a merge:** Rust `galion-deploy`, the integration-4 `tuner`, and the shadow nerve on 8672 (from `nerve-shadow-20261021`). Each has a written rollback in the call record.
- **Waiting on Slavik's decisions:** the 15 rolled-up items in `PROCESSES.md`. The ones that unblock the most: the HADES admin key, which integration branch to merge, H1 (paid model for live runs), `link.env` permissions plus the sandbox /proc fix, and the tuner DNS record.
- **Done by Slavik so far:** $50 OpenRouter credit (reported by Mate, not verified by an agent). Revolut ban lifted for the redesign. Publishing and new repos allowed (t124/t125). Ceiling raised to 250 agents. Auto-accept for the VPS deployment (Oct 20).
