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
- Only `gaia-core` and `gaia` are **core** and own nothing (integration-2 adds `gaia-live` as core, pending Slavik; see below).
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

## Crate table (53 workspace crates)

| Crate | Kind | System | Function | Why |
|---|---|---|---|---|
| `cgroup-detect` | system | quotas | POSEIDON | read-only probe of per-run cgroup v2 limits; quotas are POSEIDON compute resources (was HEPHAESTUS) |
| `console-static` | system | console | HEPHAESTUS | pending Slavik: Rust console static server (console-route-31, main-staging 2026-10-21) |
| `console-ui` | system | console | HEPHAESTUS | pending Slavik: Rust console rooms replacing the old SPA (console-port-1..5, main-staging 2026-10-21) |
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
| `gaia-live` | core | (none) | GAIA | pending Slavik: Gaia's read-only live sensing (status --live) and the approval-gated heal path for box TEST services; owns no system (gaia-land note); alternatives guardian-watchdog/MINERVA or healer/DEMETER |
| `gaia-minerva` | function | (none) | MINERVA | MINERVA function crate |
| `gaia-poseidon` | function | (none) | POSEIDON | POSEIDON function crate |
| `galion-boot` | system | boot-units | DEMETER | boot/keep replacement and systemd unit renderer |
| `galion-chatterbox` | system | speech-io | AETHER | pending Slavik: Chatterbox TTS (T3, S3Gen, HiFT, voice cloning) on candle, CPU only; the mouth behind galion-voice-serve, speech like galion-voice-cpu/openmind-speech; came in with voice-rust via candle-zip-fix |
| `galion-common` | system | zagreb-clock | AETHER | pending Slavik: shared std-only helpers (SHA-256, hex, base64, JSON, Zagreb time, small HTTP/1.1 server/client); nearest system is the Zagreb clock it carries; alternative: core |
| `galion-desk` | system | galion-desk | HEPHAESTUS | operator desk endpoint (:8646) |
| `galion-one` | system | galion-one | ELEUTHIA | Galion One, the persona the stack embodies (:8091, :8002) |
| `galion-origin` | system | origin | AETHER | origin.py replacement: front door, desk notes, 302 builder (folded openmind-origin) |
| `galion-realtest` | system | repo-check | MINERVA | pending Slavik: platform-wide real-life test harness (self-evaluation); alternative cargo-build/HEPHAESTUS |
| `galion-tar` | system | survival | DEMETER | pending Slavik: shared std-only tar crate (GNU tree writer, ustar writer/reader, streaming reader) used by galion-backup and galion-deploy (shared-tar-crate, main-staging via slot 242) |
| `galion-voice-core` | system | voice-relay | AETHER | full-duplex framing, VAD, jitter buffer, barge-in |
| `galion-voice-cpu` | system | speech-io | AETHER | on-device CPU STT -> text model -> TTS pipeline (Whisper-tiny via candle); speech on CPU like openmind-speech |
| `galleon-arena` | system | ecosystem-census | ARTEMIS | paper arena that lists and scores the gladiator population; no value moves (was ARES) |
| `guardian-tick` | system | guardian-tick | DEMETER | one report-only health pass |
| `hades-gate` | system | teardown | HADES | approval gate and journal for destructive plans; dry-run, never executes |
| `hermes-gateway` | system | messaging | AETHER | Hermes API server binary: the chat/messaging channel |
| `hermes-rs` | system | hermes-agents | ARTEMIS | Rust Hermes: agent loop, delegate_task caps, sessions, spend caps |
| `hermes-tools` | system | terminal | HEPHAESTUS | pending Slavik: sandboxed coding tools for hermes-rs, run through openmind-sandbox; alternative hermes-agents/ARTEMIS |
| `link-watch` | system | link-watch | APOLLO | laptop link intake |
| `nerve-client` | system | nerve | AETHER | fire-and-forget signal push to galion-nerve over loopback; a nerve signal line |
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
| `tunnel-lint` | system | nerve | AETHER | pending Slavik: read-only cloudflared ingress lint (tunnel-audit d35ef73), used by the nerve ingress-lint sensor |
| `openmind-persona` | system | persona-plex | ELEUTHIA | persona TOML documents |
| `openmind-provider` | system | vram-pool | POSEIDON | compute provider registry, approval state, GPU/LAN/CPU fallback and route (was HEPHAESTUS) |
| `openmind-queue` | system | queues | POSEIDON | pending Slavik (S2 A/B): durable job queue, quotas, spend caps (numbers only), VRAM routing; the crate declares POSEIDON (option A) |
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

Systems with no crate yet stay design-only (for example vision, kill-switch). `gaia ownership` reports `systems_with_a_crate`.

## Live units

`gaia_core::ownership::UNITS` maps the stack's systemd units (seen read-only on the VPS, 2026-10-20 18:52 CEST) to systems: cloudflared and tuner-quick to tunnels, console to console, galion-backup to survival, galion-desk, galion-health to guardian-watchdog, galion-nerve to nerve, galion-one and tanga-proto to galion-one, hermes-rs (and the legacy hermes) to hermes-agents, link-watch-rs (and legacy link-watch) to link-watch, openmind-memory (and legacy memory-gateway) to memory-gateway, tuner. `gaia view` uses this to show Gaia's view of a captured host; unknown units and listeners are reported as unmapped, never guessed.

## Overlap with other swarm branches

- `swarm/integration-1` (8fd26f6) also carries ws/gaia, plus `crates/gaia-live` and live-heal additions in `crates/gaia/src/{lib,main}.rs`, and still has the old GAIA-4 version of this file. This branch stays on main 13a296c. When the two meet: take this file and `gaia_core::ownership`; add one row for `gaia-live` (kind core: Gaia's read-only sensing / approval-gated test-service path, owns no system), otherwise `every_workspace_crate_is_mapped` fails, which is the point of the test. Expect textual conflicts where both branches append commands to `crates/gaia/src/lib.rs` and the USAGE/match in `main.rs`; the commands are disjoint (`plan`, `dry-run`, `ownership`, `view` here).
- `docs/crate-index.md` and `docs/STATUS.md` (GAIA-1, GAIA-2 rows) still describe the VPS gaia-core/gaia-registry. Left untouched here (status lives in one table that the owner updates on merge).

## integration-2 rows (2026-10-20, pending Slavik where marked)

integration-1 brought 7 crates the registry did not know; `every_workspace_crate_is_mapped` failed on exactly these after the gaia-land merge, as intended. integration-2 adds one row each. Rows whose `why` starts with `pending Slavik` are judgement calls (`gaia ownership` shows `"pending_slavik": true`); they stay open until the S2 decision (plan S2.2).

| Crate | Placed at | Pending? | Alternatives |
|---|---|---|---|
| `gaia-live` | core (owns no system) | yes | guardian-watchdog / MINERVA (read-only health), healer / DEMETER (heal path). Core follows the gaia-land note above, but widens "only gaia and gaia-core are core". |
| `galion-chatterbox` | speech-io / AETHER | yes | voice-relay / AETHER (it serves the legacy mouth API through galion-voice-serve). Arrived with voice-rust inside candle-zip-fix (slot 154). |
| `galion-common` | zagreb-clock / AETHER | yes | core; it is shared plumbing (SHA-256, base64, JSON, Zagreb time, HTTP/1.1) with no natural system. |
| `galion-realtest` | repo-check / MINERVA | yes | cargo-build / HEPHAESTUS. |
| `galion-voice-cpu` | speech-io / AETHER | no | Same place as `openmind-speech` (STT/TTS on CPU). |
| `hermes-tools` | terminal / HEPHAESTUS | yes | hermes-agents / ARTEMIS. It runs through openmind-sandbox (terminal). |
| `nerve-client` | nerve / AETHER | no | Same system as `openmind-nerve`. |
| `openmind-queue` | queues / POSEIDON | yes (S2 A/B) | Under option B (VPS GAIA-4: no POSEIDON) it would move to HEPHAESTUS with the vram-pool crates. The crate itself declares POSEIDON. |

## integration-3 rows (2026-10-21, all pending Slavik)

integration-3 brings 10 more crates (rust-ports, deploy-rust, health-land and the bench-hermes family). `every_workspace_crate_is_mapped` failed on exactly these after the merges; one row each, every one a judgement call (`"pending_slavik": true`). Counts: 63 crates, 51 system crates, 29 systems with a crate.

| Crate | Placed at | Alternatives |
|---|---|---|
| `bench-banking` | ecosystem-census / ARTEMIS | a new `bench` system (wave-4 slot 129 adds a bench sector to hermes-core) |
| `bench-scoreboard` | ecosystem-census / ARTEMIS | teacher-docs / APOLLO, or a new bench system |
| `galion-backup` | survival / DEMETER | healer / DEMETER, release-artifacts / HEPHAESTUS. Same system as its `galion-backup` unit in `UNITS`. |
| `galion-bench` | ecosystem-census / ARTEMIS | a new bench system |
| `galion-deploy` | deploy-switch / HEPHAESTUS | rollback / HADES (it does roll back automatically, and unlike `deploy/switch.sh` it executes) |
| `galion-guard` | guardian-watchdog / MINERVA | tunnels / AETHER |
| `galion-health` | guardian-watchdog / MINERVA | guardian-tick / DEMETER, alerting / MINERVA. Same system as its `galion-health` unit in `UNITS`. |
| `galion-vending` | ecosystem-census / ARTEMIS | a new bench system |
| `galion-wallstreet` | ecosystem-census / ARTEMIS | a new bench system |
| `hermes-bench` | hermes-agents / ARTEMIS | ecosystem-census / ARTEMIS, a new bench system |

The six benchmark crates sit next to `galleon-arena` (which already scores the agent population) until Slavik decides whether benchmarks get their own system.

## candle-zip-rebase row (2026-10-21 07:45 CEST +02:00, pending Slavik)

swarm/candle-zip-rebase merges candle-zip-fix a4f49b5 onto integration-4 42de672. candle-zip-fix sits on voice-rust, so the merge also brings the `galion-chatterbox` crate, and `every_workspace_crate_is_mapped` failed on it. One row, a judgement call (`"pending_slavik": true`): speech-io / AETHER, next to `galion-voice-cpu` and `openmind-speech`; alternative voice-relay / AETHER. Counts: 64 crates, 52 system crates, 29 systems with a crate (speech-io already had one).

## shared-tar-mergefix row (2026-10-22 01:15 CEST +02:00, pending Slavik)

swarm/shared-tar-mergefix (wave-4 slot 242) merges swarm/main-staging into shared-tar-crate 820740e. shared-tar-crate adds the std-only `galion-tar` crate (galion-backup and galion-deploy now share it), and `every_workspace_crate_is_mapped` failed on it, which is why the main merge skipped 820740e. One row, a judgement call (`"pending_slavik": true`): survival / DEMETER, next to `galion-backup`, its main user; alternatives deploy-switch / HEPHAESTUS (galion-deploy) or core (shared plumbing like `galion-common`). Counts: 68 crates, 56 system crates; systems with a crate unchanged (survival already had one).

## release-repro-unify rows (2026-10-27 23:15 CET +01:00, pending Slavik)

swarm/release-repro-unify (wave-4 slot 325) adds two crates so solo `-p` release builds are byte-identical to the full-set build, and `every_workspace_crate_is_mapped` failed on them. Two rows, both judgement calls (`"pending_slavik": true`): `galion-feature-pin` (doc-only feature pins for the release bins) at release-artifacts / HEPHAESTUS, alternative cargo-build / HEPHAESTUS; `galion-jsonout` (the serde_json helpers split out of `galion-common`, so `galion-common` has no features) at zagreb-clock / AETHER next to `galion-common`, alternative core. Counts: two more crates and two more system crates; release-artifacts gains its first crate.
