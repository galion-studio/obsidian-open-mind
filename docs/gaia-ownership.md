<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Gaia ownership registry

- Written: 2026-10-20 19:00 CEST (+02:00), branch `swarm/gaia-land` (swarm slot 01).
- Code: `crates/gaia-core/src/ownership.rs` (`gaia_core::ownership::{CRATES, UNITS, crate_owner, unit_owner, crates_for}`).
- CLI: `gaia ownership` prints the whole registry as JSON.
- Guard: `crates/gaia-core/tests/workspace_ownership.rs` reads every `crates/*/Cargo.toml` and **fails if any workspace crate is unmapped**, or if the registry names a crate that is gone.
- Design: `gaia/docs/gaia-design.md` (9 functions, 52 systems), merged from `ws/gaia` 04778e6.

## Decision (supersedes the GAIA-4 list of 2026-10-19 18:48 CEST)

The previous version of this file (DESK-ID openmind.GAIA-4, crate `gaia-registry`) used a different set of nine:
GAIA, HADES, HEPHAESTUS, DEMETER, ARTEMIS, APOLLO, MINERVA, ELEUTHIA, ARES. It conflicted with the design in three ways:

1. It counted **GAIA itself** as one of the nine. In the design, GAIA core owns *only* orchestration and the master override, and owns **no system** (gaia-design §4.1).
2. It had an **ARES** function (the paper arena) that the design does not have.
3. It had **no POSEIDON and no AETHER**, so compute resources and communications had nowhere to live and were spread over HEPHAESTUS, APOLLO, ELEUTHIA and GAIA.

**Decision: the 52-system design wins.** `docs/SOURCE-OF-TRUTH.md` already picks ws/gaia over the VPS gaia-core + gaia-registry ("full orchestrator (52 systems, approval-token HADES path)"), and `gaia-registry` is not in main 13a296c any more. So:

- The nine are HADES, MINERVA, APOLLO, HEPHAESTUS, ARTEMIS, POSEIDON, AETHER, DEMETER, ELEUTHIA (`gaia_core::FunctionId`). GAIA and ARES are not functions.
- A crate maps to a **system**, and its function is that system's owner. A crate never maps straight to a function, except the nine function crates themselves.
- Only `gaia-core` and `gaia` are **core** and own nothing.
- Every one of the **35 crates of main 13a296c** maps to one of the 52 systems (`non_gaia_crates_map_to_a_system` checks the count).

Moves against the old list (the "was" notes in the table):

| Crate | Old | New (system / function) | Why |
|---|---|---|---|
| `zagreb-time` | GAIA | zagreb-clock / AETHER | GAIA core owns no system; the clock is a design AETHER system. |
| `galleon-arena` | ARES | ecosystem-census / ARTEMIS | ARES is not a function; the arena lists and scores the gladiator population. |
| `cgroup-detect` | HEPHAESTUS | quotas / POSEIDON | Per-run limits are compute resources. |
| `openmind-exo`, `openmind-llama`, `openmind-provider`, `openmind-tulu` | HEPHAESTUS / APOLLO | vram-pool / POSEIDON | External compute pool and the GPU/LAN/CPU route. |
| `openmind-discovery` | APOLLO | lan-discovery / AETHER | Network identity and discovery are comms. |
| `openmind-nerve` | APOLLO | nerve / AETHER | The nerve is a design AETHER system. |
| `openmind-speech`, `persona-plex-relay` | ELEUTHIA | speech-io, voice-relay / AETHER | Speech and relay are channels; ELEUTHIA keeps the personas. |
| `openmind-mcp-stack` | MINERVA | stack-schema / APOLLO | It serves knowledge about the stack; it audits nothing. |
| `one-defence` | HEPHAESTUS | guardian-watchdog / MINERVA | Health/defence watch is security. |
| `one-set-model`, `one-talk` | HEPHAESTUS | galion-one / ELEUTHIA | Parts of Galion One. |

Old rows for crates no longer in main (`gaia-registry`, `hades-journal`, `galleon-voice`, `openmind-boot`, `openmind-fallback`, `openmind-origin`, `openmind-route`, `openmind-units`) are dropped; their code was folded into `hades-gate`, `openmind-provider`, `galion-origin` and `galion-boot` (see SOURCE-OF-TRUTH's winner table).

## Crate table (46 workspace crates)

| Crate | Kind | System | Function | Why |
|---|---|---|---|---|
| `bench-banking` | system | ecosystem-census | ARTEMIS | B1 banking benchmark: Revolut-shaped mock bank, drafts only, scores the agent; no money moves (bench sector) |
| `bench-scoreboard` | system | ecosystem-census | ARTEMIS | renders the three benchmark percentages into SCOREBOARD.md (bench sector) |
| `cgroup-detect` | system | quotas | POSEIDON | read-only probe of per-run cgroup v2 limits; quotas are POSEIDON compute resources (was HEPHAESTUS) |
| `desk-hermes` | system | hermes-agents | ARTEMIS | Hermes mind snap for the desk: the agent population view |
| `gaia` | core | (none) | GAIA | the gaia CLI: wires GAIA core to the nine functions; dry-run only |
| `gaia-aether` | function | (none) | AETHER | AETHER function crate |
| `gaia-apollo` | function | (none) | APOLLO | APOLLO function crate |
| `gaia-artemis` | function | (none) | ARTEMIS | ARTEMIS function crate |
| `gaia-core` | core | (none) | GAIA | GAIA core: orchestrator, ownership table, bus, Alpha registry, lineage |
| `gaia-demeter` | function | (none) | DEMETER | DEMETER function crate |
| `gaia-eleuthia` | function | (none) | ELEUTHIA | ELEUTHIA function crate |
| `gaia-hades` | function | (none) | HADES | HADES function crate (plans only, approval-gated) |
| `gaia-hephaestus` | function | (none) | HEPHAESTUS | HEPHAESTUS function crate |
| `gaia-minerva` | function | (none) | MINERVA | MINERVA function crate |
| `gaia-poseidon` | function | (none) | POSEIDON | POSEIDON function crate |
| `galion-bench` | system | ecosystem-census | ARTEMIS | shared benchmark harness: scorecards, axes, Zagreb stamps (bench sector) |
| `galion-boot` | system | boot-units | DEMETER | boot/keep replacement and systemd unit renderer |
| `galion-desk` | system | galion-desk | HEPHAESTUS | operator desk endpoint (:8646) |
| `galion-one` | system | galion-one | ELEUTHIA | Galion One, the persona the stack embodies (:8091, :8002) |
| `galion-origin` | system | origin | AETHER | origin.py replacement: front door, desk notes, 302 builder (folded openmind-origin) |
| `galion-vending` | system | ecosystem-census | ARTEMIS | B3 vending-machine simulation, seeded and deterministic; simulated cents only (bench sector) |
| `galion-voice-core` | system | voice-relay | AETHER | full-duplex framing, VAD, jitter buffer, barge-in |
| `galion-wallstreet` | system | ecosystem-census | ARTEMIS | B2 Wall Street benchmark: paper broker, risk limits, recorded fixtures; no brokerage (bench sector) |
| `galleon-arena` | system | ecosystem-census | ARTEMIS | paper arena that lists and scores the gladiator population; no value moves (was ARES) |
| `guardian-tick` | system | guardian-tick | DEMETER | one report-only health pass |
| `hades-gate` | system | teardown | HADES | approval gate and journal for destructive plans; dry-run, never executes |
| `hermes-bench` | system | hermes-agents | ARTEMIS | the three benchmark tool sets as hermes-rs tools and the hermes-bench runner; Hermes's bench sector runs it (bench sector) |
| `hermes-core` | system | boss-node | ARTEMIS | Hermes coordinator above the swarm sectors: registry, governor, retry loop, HADES-gated dispatch, ledger (swarm/hermes-core) |
| `hermes-gateway` | system | messaging | AETHER | Hermes API server binary: the chat/messaging channel |
| `hermes-rs` | system | hermes-agents | ARTEMIS | Rust Hermes: agent loop, delegate_task caps, sessions, spend caps |
| `link-watch` | system | link-watch | APOLLO | laptop link intake |
| `one-defence` | system | guardian-watchdog | MINERVA | read-only health_checks rows and a defence note; security watch (was HEPHAESTUS) |
| `one-set-model` | system | galion-one | ELEUTHIA | Galion One plan/action model lane, validated, writes nothing (was HEPHAESTUS) |
| `one-talk` | system | galion-one | ELEUTHIA | Galion One hear/reply request shapes (was HEPHAESTUS) |
| `openmind-config` | system | config-health | DEMETER | config health: TOML, then env, loopback defaults |
| `openmind-discovery` | system | lan-discovery | AETHER | LAN peer seeds and scan path; network identity is AETHER (was APOLLO) |
| `openmind-exo` | system | vram-pool | POSEIDON | exo compute adapter (fixtures); external compute is POSEIDON (was HEPHAESTUS) |
| `openmind-llama` | system | vram-pool | POSEIDON | llama.cpp compute adapter (fixtures) (was HEPHAESTUS) |
| `openmind-mcp-stack` | system | stack-schema | APOLLO | stdio MCP stack_status: serves knowledge about the stack (was MINERVA) |
| `openmind-memory` | system | memory-gateway | APOLLO | memory store and gateway (:8420) |
| `openmind-nerve` | system | nerve | AETHER | nerve event archive: the signal lines are AETHER (was APOLLO) |
| `openmind-persona` | system | persona-plex | ELEUTHIA | persona TOML documents |
| `openmind-provider` | system | vram-pool | POSEIDON | compute provider registry, approval state, GPU/LAN/CPU fallback and route (was HEPHAESTUS) |
| `openmind-sandbox` | system | terminal | HEPHAESTUS | runs one command under rlimits, cleared env, workdir, timeout |
| `openmind-schema` | system | stack-schema | APOLLO | the stack contract |
| `openmind-speech` | system | speech-io | AETHER | whisper.cpp / piper output parsers on CPU (was ELEUTHIA) |
| `openmind-survival` | system | survival | DEMETER | read-only probes and suggestion-only decisions |
| `openmind-tulu` | system | vram-pool | POSEIDON | DR Tulu llama-server row (fixture); a compute provider (was APOLLO) |
| `persona-plex-relay` | system | voice-relay | AETHER | voice relay backend adapter for Persona Plex (was ELEUTHIA) |
| `repo-check` | system | repo-check | MINERVA | self-eval: SPDX, licence, secrets, no-Python, Zagreb time |
| `room` | system | room | ARTEMIS | read-only page of the operator files agents share |
| `tuner` | system | tuner | DEMETER | ukulele tuner static server (:8650) |
| `zagreb-time` | system | zagreb-clock | AETHER | Europe/Zagreb RFC 3339 with explicit offset (was GAIA) |

Systems with no crate yet stay design-only (for example vision, queues, kill-switch). `gaia ownership` reports `systems_with_a_crate`.

## Live units

`gaia_core::ownership::UNITS` maps the stack's systemd units (seen read-only on the VPS, 2026-10-20 18:52 CEST) to systems: cloudflared and tuner-quick to tunnels, console to console, galion-backup to survival, galion-desk, galion-health to guardian-watchdog, galion-nerve to nerve, galion-one and tanga-proto to galion-one, hermes-rs (and the legacy hermes) to hermes-agents, link-watch-rs (and legacy link-watch) to link-watch, openmind-memory (and legacy memory-gateway) to memory-gateway, tuner. `gaia view` uses this to show Gaia's view of a captured host; unknown units and listeners are reported as unmapped, never guessed.

## Overlap with other swarm branches

- `swarm/integration-1` (8fd26f6) also carries ws/gaia, plus `crates/gaia-live` and live-heal additions in `crates/gaia/src/{lib,main}.rs`, and still has the old GAIA-4 version of this file. This branch stays on main 13a296c. When the two meet: take this file and `gaia_core::ownership`; add one row for `gaia-live` (kind core: Gaia's read-only sensing / approval-gated test-service path, owns no system), otherwise `every_workspace_crate_is_mapped` fails, which is the point of the test. Expect textual conflicts where both branches append commands to `crates/gaia/src/lib.rs` and the USAGE/match in `main.rs`; the commands are disjoint (`plan`, `dry-run`, `ownership`, `view` here).
- `docs/crate-index.md` and `docs/STATUS.md` (GAIA-1, GAIA-2 rows) still describe the VPS gaia-core/gaia-registry. Left untouched here (status lives in one table that the owner updates on merge).

## Benchmark crates (swarm/hermes-core-bench-sector, 2026-10-21 05:05 CEST +02:00)

The six crates of swarm/bench-hermes 457ef74 (`bench-banking`, `bench-scoreboard`, `galion-bench`, `galion-vending`, `galion-wallstreet`, `hermes-bench`) were unmapped, so the guard test failed once bench-hermes was merged onto gaia-land. They are mapped by the precedent of `galleon-arena` (a paper arena that scores the population): the five benchmark crates go to `ecosystem-census` / ARTEMIS, and `hermes-bench`, which drives hermes-rs agents, goes to `hermes-agents` / ARTEMIS. The live nerve on the VPS already reports hermes-rs under ARTEMIS / hermes-agents (read-only GET of 127.0.0.1:8670/state, 2026-10-21 05:02 CEST). `hermes-core` (boss-node), which was in the code but not in this table, is listed too. Slavik may move them. It is one line each in `ownership.rs`.
