<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Decision memo: who owns compute in Gaia (POSEIDON or HEPHAESTUS)

Written 2026-10-20 23:00 CEST (+02:00), slot 42 `decision-memos`. Read-only research; nothing decided here.
Sources: plans/gaia.md §2, §3 S2.1, §5; wt/gaia-land 9d3ec18 docs/gaia-ownership.md; wt/gaia-poseidon 498e3d2; CALL-RECAP.md §4.9; CALL-RECORD.md 20:52.

## The two options
| | Option A: design (ws/gaia 04778e6) | Option B: VPS GAIA-4 `gaia-registry` (2026-10-19 18:48) |
|---|---|---|
| Nine functions | HADES, MINERVA, APOLLO, HEPHAESTUS, ARTEMIS, POSEIDON, AETHER, DEMETER, ELEUTHIA | GAIA, HADES, HEPHAESTUS, DEMETER, ARTEMIS, APOLLO, MINERVA, ELEUTHIA, ARES (no POSEIDON, no AETHER) |
| Compute | POSEIDON: vram-pool, quotas, queues, budget-caps | HEPHAESTUS (fabrication + compute) |
| Backing | SOURCE-OF-TRUTH.md picks ws/gaia (PLAN-DIGEST.md:110) | `gaia-registry` is not on main 13a296c |
| Undisputed | `openmind-sandbox` = HEPHAESTUS terminal in both | same |

## Crates that move (old GAIA-4 owner -> option A owner); under B they stay/return as in column 2
| Crate | B (GAIA-4) | A (gaia-land registry) |
|---|---|---|
| cgroup-detect | HEPHAESTUS | quotas / POSEIDON |
| openmind-exo, openmind-llama, openmind-provider | HEPHAESTUS | vram-pool / POSEIDON |
| openmind-tulu | APOLLO | vram-pool / POSEIDON |
| openmind-queue (new, not in either list) | needs a HEPHAESTUS row | POSEIDON (`gaia::REGISTRY_ROW`) |
| zagreb-time | GAIA | zagreb-clock / AETHER |
| openmind-discovery, openmind-nerve | APOLLO | lan-discovery, nerve / AETHER |
| openmind-speech, persona-plex-relay | ELEUTHIA | speech-io, voice-relay / AETHER |
| galleon-arena | ARES | ecosystem-census / ARTEMIS |
| openmind-mcp-stack | MINERVA | stack-schema / APOLLO |
| one-defence | HEPHAESTUS | guardian-watchdog / MINERVA |
| one-set-model, one-talk | HEPHAESTUS | galion-one / ELEUTHIA |
Under B the VPS list also had openmind-route/fallback/origin under HEPHAESTUS; those crates no longer exist (folded into openmind-provider / galion-origin).
Under B, AETHER's 6 crates above have no AETHER to go to and must be spread over APOLLO/ELEUTHIA/GAIA again; `gaia-aether` and `gaia-poseidon` function crates would be orphaned.

## Code already written against option A
- swarm/gaia-poseidon 498e3d2: `crates/gaia/src/poseidon.rs` uses `FunctionId::COMPUTE` (= POSEIDON) for owner, lineage and `System::COMPUTE`; real test 60/60.
- swarm/gaia-land 9d3ec18: `gaia_core::ownership` (46 crates -> 52 systems / 9 functions), `docs/gaia-ownership.md`, guard test `workspace_ownership`.
- swarm/poseidon 7d0c478: openmind-queue `gaia::REGISTRY_ROW` -> POSEIDON.
- swarm/lan-routing: VRAM from approved LAN rows feeds the POSEIDON pool.
Cost of B: revert gaia-poseidon ownership, re-home 6 compute crates + 6 AETHER crates, rewrite the registry and its doc.

## The 4 judgement-call mappings in gaia-land (owner sign-off)
CALL-RECORD.md 20:52 says "Owner sign-off: 4 judgment-call mappings"; no file names them. These four are this memo's reading: the moves whose reason is an interpretation, not a design-named system.
1. galleon-arena: ARES -> ecosystem-census / ARTEMIS ("lists and scores the gladiator population").
2. openmind-mcp-stack: MINERVA -> stack-schema / APOLLO ("serves knowledge, audits nothing").
3. one-defence: HEPHAESTUS -> guardian-watchdog / MINERVA ("health/defence watch is security").
4. hermes-rs: hermes-agents / ARTEMIS in gaia-land vs POSEIDON budget-caps in PROGRESS.md:123 (plan S2.3).

## Questions for Slavik (answer yes/no)
1. Option A: POSEIDON owns compute (cgroup-detect, openmind-exo/llama/provider/tulu, openmind-queue)?
2. AETHER exists as a function and takes zagreb-time, discovery, nerve, speech, persona-plex-relay?
3. GAIA owns no system and ARES is not a function?
4. galleon-arena -> ARTEMIS ecosystem-census?
5. openmind-mcp-stack -> APOLLO stack-schema?
6. one-defence -> MINERVA guardian-watchdog?
7. hermes-rs -> ARTEMIS hermes-agents (not POSEIDON budget-caps)?
8. Count is 52 systems (not 51)?
9. Did I name the right 4 judgement calls? (If no, name them.)
