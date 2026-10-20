<!-- SPDX-License-Identifier: MIT -->
# BACKLOG: Obsidian Open Mind, made by Galion (ranked)
Written 2026-10-18T21:30+02:00 (Europe/Zagreb) by Grok Bot. Read-only inputs: TEACHER/QUEUE.md (B01-B50), docs/rust-rewrite-plan.md (B51-B66 + owner actions O1-O5),
TEACHER/STAGED.md, WORKSTREAMS.md, session-handoff/SESSION-SUMMARY.md, `git log rust-v1..<branch>` on the box at 21:25, and the 21:27 test sweep (TEST-MATRIX.md).
Done and excluded: B01 B02 B10 B11 B17 B18 B19 B26 B27. Duplicates merged: T3b/T5 -> B51, T6 -> B61, "Galion One systemd unit" -> B21, "OMNODE-2 sandbox v1" -> OMNODE-2.
Real-money access (Revolut, Hermes paying) is not approved and is not on this list. No secrets in this file.

## Ranking rule
Weights, highest first: (a) Gaia as the main architecture (GAIA core plus nine functions owning every system) > (b) UNIVERSAL-CPU.md > (c) Rust rewrite via workstreams > (d) Oct 31 launch, private repo `galion-studio/obsidian-open-mind` public only after checks pass > (e) security.
Tie-break: unblocks the most other items, then smallest effort. Items whose code is already on a branch rank under the merge item that lands them.
Status words: not started | started | partial | done on branch (not in rust-v1) | staged (built, awaiting approval) | owner (decision only Slavik can make).

## Ranked items
| Rank | ID | Title | GAIA function | Branch | Status | Blocked on owner | Why |
|---|---|---|---|---|---|---|---|
| 1 | GAIA-1 | GAIA core orchestrator crate + `gaia` CLI (status, tick, `simulate-faro --full-power`, master override) | GAIA core | ws/gaia | started: README committed (2bc8d48); `crates/gaia-core` exists only as untracked files in the agent worktree | no | (a) the main architecture; every other item needs an owner to report to |
| 2 | GAIA-2 | Nine function modules + ownership registry routing every crate, service, unit and staged item to one function (table in this file is the seed) | GAIA core (all nine) | ws/gaia | not started | no | (a) without it "owns every system" is only a doc |
| 3 | UCPU-1 | Universal-CPU gate for every binary: x86_64 + aarch64 musl build, aarch64-android + armv7 check, size/RSS per target; generalise ws/memory `cross.sh` | HEPHAESTUS | all ws/* (ws/memory 57083d3 has cross.sh) | partial | no | (b) binding rule; must pass before any merge |
| 4 | HADES-1 | Approval-token gate for destructive paths: healer heal-apply, switch/rollback `--apply`, kill switch, rotation plans; dry-run default | HADES | ws/survival (94d3260 dry-run default) + ws/gaia | partial | no | (a)(e) HADES is defined as approval-gated; healer already has apply paths |
| 5 | WS-SURV | Merge ws/survival into rust-v1 (galion-guardian, galion-healer, galion-boot, galion-origin; lands B20 B23 B24 B25 B54 B55 B56) after Cargo.lock fix | DEMETER (HADES for heal-apply) | ws/survival | done on branch: 7 commits, fmt/clippy/41 tests green | no | (a)(c) survival is the stack's self-repair; largest finished stream |
| 6 | WS-HERMES | Merge ws/hermes: hermes-rs (OpenAI API + SSE, agent loop, subagent caps, state.db-compatible store, cron) + openmind-persona M1 | ARTEMIS (ELEUTHIA for persona) | ws/hermes | done on branch: 6 commits, 50 tests green | no (any live switch is) | (a)(c) Hermes is the 300 MB Python hot spot |
| 7 | WS-MEM | Merge ws/memory: openmind-memory v0 (B58, B59, B60 prep), then shadow run + parity report | APOLLO | ws/memory | done on branch: 5 commits, 23 tests green; shadow run not done | switch only | (a)(c) memory-gateway is 615 MB RSS, the biggest saving |
| 8 | WS-ODT | Merge ws/one-desk-tuner: galion-one with no Python (B12-B15, B11 backoff), galion-desk config (B05), B06 part, tuner crate (B53), staged units | HEPHAESTUS (tuner: DEMETER) | ws/one-desk-tuner | done on branch: 7 commits, 35 tests green; Cargo.lock drift | no | (c) phase 0 of the rewrite plan |
| 9 | LAUNCH-1 | First push to private `galion-studio/obsidian-open-mind`: rust-v1 -> main, all branches, secret scan, green Actions; public only after checks pass | MINERVA (checks), HEPHAESTUS (release) | rust-v1 | not started | yes (push, later visibility) | (d) Oct 31 needs CI history before launch week |
| 10 | SEC-1 | Secret rotations + history purge (O1: tracked .env files and key patterns in the galion-src clones); HADES writes the rotation plan, owner rotates | HADES (plan), MINERVA (scan) | none | owner | yes | (e)(d) hard gate before anything goes public |
| 11 | SEC-2 | Lock the session log that contains the token (permissions or move; owner decides) | MINERVA | none | owner | yes | (e) live credential exposure |
| 12 | WS-VOICE | Merge ws/voice M2: galion-voice-cpu (Whisper-tiny via candle, opt-in feature), Opus/Ogg codec, half-duplex pre-roll | AETHER | ws/voice | done on branch: 3 commits, 38 tests green (whisper feature not built) | no | (a)(b)(c) on-device CPU voice; check candle against UNIVERSAL-CPU |
| 13 | SEC-3 | Hermes file permissions (O3: config.yaml and SOUL.md 666, vault/ 777) | MINERVA | none | owner | yes | (e) world-writable agent config |
| 14 | B43 | Self-check script: C1-C3, C6-C8, SPDX, no-python, secret scan (file:line only), size budget | MINERVA | rust-v1 (repo-check 7b015b1 covers the lints) | partial | no | (d)(e) the "checks pass" gate for going public |
| 15 | LAUNCH-2 | Oct 31 launch checklist (RELEASE-CHECKLIST.md) run end to end, with go/no-go | GAIA core | rust-v1 | partial (checklist exists) | no | (d) |
| 16 | B21 | Restart=always drop-ins + guardian timer + Galion One systemd unit (One runs under nohup, dies on reboot) | DEMETER | ws/one-desk-tuner 9a6c6e1, ws/survival (staged units) | staged | yes | (c) survival gap |
| 17 | B51 | Switch live one, galion-desk, link-watch to the dist builds (T3b, T5) | HEPHAESTUS | ws/one-desk-tuner (docs) | staged | yes | (c) live binaries are not the repo builds |
| 18 | OMNODE-2 | Commit openmind-sandbox v1 and move omnode onto it | MINERVA (policy), HEPHAESTUS (runner) | staged/sandbox-v1 c2579ce | staged; VPS omnode still on landlock 0.4 | yes | (a)(b)(e) |
| 19 | PHONE-1 | Phone node: enrollment endpoint, tunnel, HTTPS host for the phone PWA | AETHER | none (docs/hermes-enrollment.md, design) | not started | yes (HTTPS host) | (a)(d) |
| 20 | B42 | SPDX/REUSE headers everywhere, REUSE.toml | MINERVA | rust-v1 (69621a3, 22 files) | partial | no | (d) |
| 21 | B16 | No-Python gate | MINERVA | rust-v1 (repo-check python lint) | partial | no | (c) |
| 22 | B08 | Size budget script + low-end profile doc | HEPHAESTUS | none | not started | no | (b) |
| 23 | B44 | SBOM + license allowlist + CI step | MINERVA | none | not started | no | (d)(e) |
| 24 | B45 | SECURITY.md review + OpenSSF status | MINERVA | rust-v1 (SECURITY.md) | partial | no | (d)(e) |
| 25 | FE-1 | Console: 10 lint errors, 7 type errors, 16 template-drift test failures, hard-coded VPS IP in the bundle | HEPHAESTUS | console-ide (not in the repo) | found by sweep | no | (d)(e) |
| 26 | B61 | Console prod switch + /health route (T6, 505 MB -> 57 MB) | HEPHAESTUS | none | staged | yes | (c) |
| 27 | B33 | NerveEvent schema in openmind-schema | AETHER | ws/survival (nerve lines 205bd81) | partial | no | (a) the nerve |
| 28 | B40 | openmind-nerve JSONL writer | AETHER | ws/survival (partial) | partial | no | (a) |
| 29 | ART-1 | Agent spawning and retirement under ARTEMIS (delegate_task caps, boss node, room) | ARTEMIS | ws/hermes a880772 | partial | no | (a) |
| 30 | ELEU-1 | Gaia seeds: seed format -> new agents/projects, lineage, selection by checks | ELEUTHIA | ws/gaia | not started | no | (a) |
| 31 | B34 | ComputeProvider trait + registry with approval state | POSEIDON | none | not started | no | (a)(b) |
| 32 | B38 | Provider fallback GPU -> LAN -> CPU | POSEIDON | none | not started | no | (a)(b) |
| 33 | POS-1 | Raise the Hermes spend cap | POSEIDON | none | owner | yes | budget |
| 34 | POS-2 | RunPod top-up / GPU | POSEIDON | none | owner | yes | budget; agents never hold payment access |
| 35 | HYG-1 | Commit regenerated Cargo.lock on ws/survival and ws/one-desk-tuner (`--locked` fails today) | HEPHAESTUS | ws/survival, ws/one-desk-tuner | found by sweep | no | blocks clean merge |
| 36 | HYG-2 | Rule for all agents: one CARGO_TARGET_DIR per worktree (shared dir served stale rlibs) | HEPHAESTUS | none | found by sweep | no | false failures/passes |
| 37 | HYG-3 | Reconcile STAGED.md OMNODE-2 text with the VPS omnode tree | APOLLO | none | found by sweep | no | docs drift |
| 38 | B60 | memory shadow run + parity, switch staged | APOLLO | ws/memory 7af1c73 (prep) | partial | switch: yes | (c) |
| 39 | B20 | Healer probe list as data | DEMETER | ws/survival ec28d09 | done on branch | no | lands with WS-SURV |
| 40 | B23 | Healer decision engine | DEMETER | ws/survival 205bd81 | done on branch | no | lands with WS-SURV |
| 41 | B24 | Graceful degradation rules | DEMETER | ws/survival 205bd81 | done on branch | no | lands with WS-SURV |
| 42 | B25 | Guardian report -> nerve | DEMETER | ws/survival 7101702 | done on branch | no | lands with WS-SURV |
| 43 | B54 | Hardware probe (hardware.json) | DEMETER | ws/survival de1edf9 | done on branch | no | lands with WS-SURV |
| 44 | B55 | Unit renderer, no install | DEMETER | ws/survival de1edf9 | done on branch | no | lands with WS-SURV |
| 45 | B56 | origin.py replacement (galion-origin) | HEPHAESTUS | ws/survival 95183c5 | done on branch | no | lands with WS-SURV |
| 46 | B58 | memory-gateway API usage study | APOLLO | ws/memory 57083d3 | done on branch | no | lands with WS-MEM |
| 47 | B59 | openmind-memory v0 | APOLLO | ws/memory b1d24fd 1f6cb9b | done on branch | no | lands with WS-MEM |
| 48 | B12 | defence.py -> Rust | HEPHAESTUS | ws/one-desk-tuner 9519693 | done on branch | no | lands with WS-ODT |
| 49 | B13 | talk/warm helpers -> Rust | HEPHAESTUS | ws/one-desk-tuner 9519693 61a15f8 | done on branch | no | lands with WS-ODT |
| 50 | B14 | Retire the TTS worker spawn (8002 503 today) | HEPHAESTUS | ws/one-desk-tuner 9519693 | done on branch | no | lands with WS-ODT |
| 51 | B15 | set-model.py / trace.py | HEPHAESTUS | ws/one-desk-tuner | done on branch | no | lands with WS-ODT |
| 52 | B05 | galion-desk config-driven | HEPHAESTUS | ws/one-desk-tuner 635fe4a | done on branch | no | lands with WS-ODT |
| 53 | B06 | galion-one config-driven paths | HEPHAESTUS | ws/one-desk-tuner | partial | no | (b)(c) |
| 54 | B53 | tuner into the workspace | DEMETER | ws/one-desk-tuner 4c65d96 | done on branch | no | lands with WS-ODT |
| 55 | B52 | Dedupe old Rust sources (desk-rs, one/src, room, publish copy) | HEPHAESTUS | ws/one-desk-tuner (planned in its doc) | not started | no | (c) |
| 56 | B47 | Persona Plex v1 machine-readable | ELEUTHIA | ws/hermes dd6daaa (openmind-persona M1) | partial | no | (a) |
| 57 | B65 | persona-plex-relay tracked, SPDX, tests, size/RSS | ELEUTHIA | rust-v1 461acba e0b1565 | partial (tracked; size/RSS not recorded) | no | (c) |
| 58 | B62 | CPU STT/TTS adapter | AETHER | ws/voice 1fe6a17 | partial | no | (b)(c) |
| 59 | B46 | Voice-to-voice integration design | AETHER | rust-v1 (docs/ws-voice.md) | partial | no | (a) |
| 60 | WS-SPEECH | Create ws/speech (stt/tts/vision on CPU) | AETHER | none (queued in WORKSTREAMS.md) | not started | no | (c) |
| 61 | B04 | Portability inventory | DEMETER | rust-v1 (repo-check hosts lint) | partial | no | (b) |
| 62 | B03 | link-watch reads openmind-config | APOLLO | none | not started | no | (b)(c) |
| 63 | B07 | openmind-schema provider rows from config | POSEIDON | none | not started | no | (b) |
| 64 | B09 | RSS baseline for all binaries | POSEIDON | none | not started | no | (b) |
| 65 | B22 | Service survival audit | DEMETER | none | not started | no | (c) |
| 66 | B28 | seccomp/namespace layer | MINERVA | staged/sandbox-v1 (namespaces) | staged | yes (via OMNODE-2) | (e) |
| 67 | B29 | cgroup v2 detect | POSEIDON | none | not started | no | (b) |
| 68 | B30 | Sandbox CLI demo | HEPHAESTUS | none (omnode `sandbox run` exists) | partial | no | (e) |
| 69 | B31 | Sandbox policy presets | MINERVA | none | not started | no | (e) |
| 70 | B32 | LAN plug-in design | AETHER | none | not started | no | (a) |
| 71 | B35 | openmind-discovery stub | AETHER | omnode `discover` (rust-v1 7992528) | partial | no | (a) |
| 72 | B36 | llama.cpp adapter | POSEIDON | none | not started | no | (b) |
| 73 | B37 | Exo adapter | POSEIDON | none | not started | no | |
| 74 | B39 | Timestamp audit | APOLLO | rust-v1 (repo-check utc lint) | partial | no | |
| 75 | B41 | Cortex design | MINERVA | none | not started | no | (e) |
| 76 | B57 | MCP stack-read in Rust | APOLLO | none | not started | no | (c) |
| 77 | B63 | Import tulu as a llama-server provider | POSEIDON | none | not started | no | (c) |
| 78 | B64 | Archive/retire galion-light experiments (deletion only after approval) | HADES | none | not started | yes (deletion) | (c) |
| 79 | B66 | Legacy 502 hosts decision table, staged ingress cleanup | HADES | none | not started | yes (ingress) | (d) |
| 80 | STG-B01 | link-watch unit needs `LINK_WATCH_FRANCE_URL` line | APOLLO | rust-v1 (deploy) | staged | yes | switch prerequisite |
| 81 | STG-B11 | galion-one unit needs the OpenRouter key from the secret path (backoff done on ws/one-desk-tuner) | HEPHAESTUS | ws/one-desk-tuner | staged | yes | switch prerequisite |
| 82 | STG-OMBIN | Host omnode binaries (GitHub release) | HEPHAESTUS | VPS edge/dist (untracked) | staged | yes | (d) |
| 83 | STG-OMCLEAN | Merge leftovers: remove .bak files, optionally move omnode under crates/ | HEPHAESTUS | none | staged | yes (deletion) | hygiene |
| 84 | SEC-4 | O2: private key copy and secret folders inside the from-grok-build snapshot: keep or remove | HADES | none | owner | yes | (e) |
| 85 | SEC-5 | O4: tuner-quick public trycloudflare URL: keep or stop | AETHER | none | owner | yes | (e) |
| 86 | OPS-1 | O5: GitHub connector lacks private-repo scope | MINERVA | none | owner | yes | (d) remote checks of the private repo |
| 87 | B48 | Galleon Arena design | ARTEMIS | none | not started | no | last phase |
| 88 | B49 | galleon-arena crate | ARTEMIS | none | not started | no | last phase |
| 89 | B50 | Arena CLI | ARTEMIS | none | not started | no | last phase |

**Total: 89 items** (41 open B01-B50, 16 B51-B66, 32 from STAGED, workstreams, owner approvals, O1-O5 and the sweep).

## Blocked on owner (23)
LAUNCH-1, SEC-1, SEC-2, SEC-3, SEC-4, SEC-5, OPS-1, B21, B51, OMNODE-2, B28 (through OMNODE-2), PHONE-1, B61, POS-1, POS-2, WS-MEM and B60 (switch only), B64, B66, STG-B01, STG-B11, STG-OMBIN, STG-OMCLEAN.
The eight approvals named by Slavik map to: secret rotations = SEC-1; OMNODE-2 sandbox v1 = OMNODE-2; RunPod top-up/GPU = POS-2; Hermes cap raise = POS-1; Hermes file perms = SEC-3; HTTPS host for the phone PWA = PHONE-1; Galion One systemd unit = B21; locking the session log with the token = SEC-2.

## Rewrite in progress (top 10, from `git log rust-v1..<branch>` at 2026-10-18T21:25+02:00)
| Rank | ID | Branch with commits | Commits | State |
|---|---|---|---|---|
| 1 | GAIA-1 | ws/gaia | 1 (2bc8d48, concept doc only) | code started but **uncommitted** (`crates/gaia-core` untracked in /workspace/oom-ws/gaia) |
| 2 | GAIA-2 | none | 0 | not started |
| 3 | UCPU-1 | ws/memory (57083d3, 900d465: cross.sh, per-target sizes); UNIVERSAL-CPU.md added on every ws branch | 2 | partial: only ws/memory reports per-target results |
| 4 | HADES-1 | ws/survival (94d3260 healer dry-run default, allow-list) | 1 | partial: no approval token yet |
| 5 | WS-SURV | ws/survival | 7 (50da469..7101702) | code complete for B20 B23-B25 B54-B56; not merged |
| 6 | WS-HERMES | ws/hermes | 6 (8ef7394..2ca280b) | active; not merged |
| 7 | WS-MEM | ws/memory | 5 (b1d24fd..900d465) | v0 done; shadow run not started |
| 8 | WS-ODT | ws/one-desk-tuner | 7 (635fe4a..3fa0334) | code complete for its tickets; not merged |
| 9 | LAUNCH-1 | none (rust-v1 has the package overlay 6f3c9ba and green gate 69621a3) | 0 on a push branch | not started |
| 10 | SEC-1 | none | 0 | owner action |
ws/side has 0 commits ahead (merged into rust-v1 at f424e2f). ws/voice was 0 ahead at 21:20 and 3 ahead at 21:25 (ef3074a, 5d39e69, 1fe6a17).
