<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# The big picture: PROJECT 42 / Gaia

Written 2026-10-20 23:45 CEST (+02:00) by swarm/swarm-record. This file explains what the work is for. `PROCESSES.md` lists each piece.

## The vision

> Discover the universe together with humans. Leave with more than you came in with.

Gaia is the Rust platform behind Galion. The goal is simple to say: **make Gaia build, and make it great**. "Great" means it is honest about its own state, it is safe with Slavik's keys and money, and it is beautiful to use. The research, the console (Hermes and Gaia), and every sector should be things people enjoy opening.

Slavik's top-priority rule (2026-10-20 20:56 CEST) applies to every tool and report: good company and productive conversations, the kind where you leave with more than you came in with. In practice, every page and every report should give substance and a useful next step, with no filler.

## Standing rules that shape everything

- **Rust only**, as the language and in every program. No Python, not even as a probe. Non-Rust parts get rewritten as proper Rust (Slavik, 20:41 CEST).
- **Owner decisions stay with the owner.** Agents write options and evidence. Slavik decides.
- **Production is protected.** The VPS working copy `/opt/galion/openmind` stays at `2f5a730`. Live services are not stopped, restarted or reconfigured by agents. Cutovers are separate, gated steps.
- **No money moves.** Revolut is in scope for the Hermes Rust V2 design (ban lifted 22:56 CEST), but no orders, transfers or top-ups without a separate written approval.
- **Zagreb time with an offset**, never `Z`. SPDX MIT headers. Allowed licences only.

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

- **Benchmarks** (`plans/benchmarks.md`): banking, Wall Street and vending machine. All three score 100 on mocks, paper trading and simulation (`bench-scoreboard f2ff169`). The real test is Hermes running them (`bench-hermes`, in progress). Forbidden money calls counted: 0.
- **Galion OS** (`galion-os-staging/`): a secure, private, anonymous ARM sandbox beside Slavik's normal Android phone. It holds the HADES key, the phone link to Hermes, and the benchmark surface. It is in the planning stage. Nothing gets built until Slavik answers the acceptance questions.

## How the swarm works

1. Slavik sets direction, by voice or typed, and Mate relays it.
2. Planners split each sector into sessions and tasks. Coding agents each take one task on their own `swarm/<slug>` branch from main `13a296c`.
3. Each agent proves its work with **real-life tests**: the release binary on its own box port range, hit with real requests, and read-only comparison against the live VPS where that helps.
4. Branches go to GitHub only through `push-branch.sh`. The token stays on the VPS, only `swarm/*` branches are allowed, and nothing is force-pushed.
5. Integration branches (`integration-1`, then `integration-2`) merge the work and run the whole workspace (999 tests, 0 failing on integration-2).
6. Slavik merges to main and approves cutovers. Each cutover is one service at a time with rollback, rehearsed first (`deploy-rust`: 2-4 ms swaps).
7. The measured ceiling is about 18-20 agents at once on one 8-vCPU box. Stopped agents are restarted and tracked in a retry ledger.
8. Everything important is kept in three places: GitHub (`swarm/*` branches, including this record), the VPS archive (`/opt/galion/archive/swarm-<timestamp>/`, with a git bundle and a sha256 manifest), and the box (working copy only).

## Merged vs waiting on Slavik (23:45 CEST)

- **Merged into main today:** nothing. Main is `13a296c`.
- **Ready and waiting for Slavik's merge:** `swarm/integration-2` (`df42e0f`), which includes integration-1, all 20 wave-1 branches and the Gaia branches. The other wave-2 and wave-3 branches are on GitHub, each listed in `PROCESSES.md`.
- **Waiting on Slavik's decisions:** HADES admin key, strict-handling packet, merge order and how main gets pushed, cutovers, Rust-only scope rulings, HADES plan format, voice clips, benchmark live-read access, Galion OS acceptance, SEC-1..5. The rolled-up list is in `PROCESSES.md`.
- **Done by Slavik today:** $50 OpenRouter credit (reported by Mate, not verified by an agent). Revolut ban lifted for the redesign. Publishing and new repos allowed (t124/t125).
