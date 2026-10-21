<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Swarm processes, 2026-10-20 (Europe/Zagreb, CEST +02:00)

Written 2026-10-20 23:31 CEST (+02:00) by swarm/swarm-record (wave 3, slot 39). Sources: `docs/swarm/record/CALL-RECORD.md` (snapshot of the live call record), `docs/swarm/record/WAVE2.md`, the agents' branch heads in `/workspace/swarm/wt/*`, and a read-only `git ls-remote` of GitHub at 23:27 CEST.

**State of `main`:** GitHub `main` is still `13a296c` (PR #8, one-source merge). Nothing from today's swarm is merged into main yet. Every branch below is on GitHub as `swarm/<slug>` and waits for Slavik to merge, through the integration branches (`swarm/integration-1`, then `swarm/integration-2`).

**How to read this file:** one row per agent slot or workstream. *What* is the deliverable. *Why* is the reason it was started. *Status* is the state at 23:31 CEST. *Owner decisions* are open questions only Slavik can answer. Agents did not decide these themselves.

## Why the swarm ran today

Slavik wants Gaia, the Rust platform behind Galion, to build cleanly, be honest about its state, and be ready for the Oct 31 launch gates. Everything has to be in Rust, with no Python. The swarm split that work into small branches, each one with real-life tests (the real binary on a box port, real HTTP requests). Each agent wrote up the decisions it couldn't make itself instead of guessing. The VPS stayed read-only. Live services were not touched.

## Wave 1 (about 18:40-20:10 CEST): 20 branches on main 13a296c

All 20 are on GitHub and merged together in `swarm/integration-1` (`8fd26f6`).

| Slug | Head | What it did | Why | Owner decisions |
|---|---|---|---|---|
| one-probe | b44cd4f | galion-one: ONE-PROBE ported to the portable crate, fail-closed pairing, `/api/v1/services` | One must refuse unpaired devices | merge |
| ucpu | 3e70ffb | Universal-CPU gate for every binary, SQLite trim, armv7 seccomp fix | Binaries must run on any x86_64/aarch64/armv7 CPU (no SIMD flags) | merge before edge-boot |
| room | 5bb27e1 | room: lib split, RoomError, tests, cutover plan | Make room cutover-ready | cutover approval |
| hades-approval | ccaab42 | hades-gate: Ed25519 single-use admin tokens replace the public "approved-test" token | Approvals must really come from Slavik | HADES admin key (SLAVIK-TODO 1) |
| docs-sync | 80e28f3 | main.md/goals.md, SoT, STATUS.md reconciled | One source of truth | merge |
| tuner-cutover | 5fd1611 | tuner `--version/--check`, golden fixtures, cutover plan | Safe tuner cutover | tuner cutover; merge before common |
| desk-linkwatch | a54c792 | galion-desk, desk-hermes, link-watch tests, desk cutover doc | Desk and link-watch had thin tests | desk cutover |
| survival-heal | 45a18aa | guardian/healer repair path on openmind-survival | Self-healing in Rust | VPS IP visible in survival-heal.md (public export) |
| gaia-live | 4fbbb7f | ws/gaia merged, gaia heal-tick, nerve serve | Bring Gaia core onto main's line | take nerve's main.rs over this one |
| sandbox | d194c19 | caps/net flake root cause, fd hardening, per-run cgroup, escape test | Sandbox must contain agent tools | merge |
| memory-real | 723b46c | openmind-memory real-life suite, pre-v3 migration fix | Memory is live on 8420 | merge |
| real-harness | 5b3c662 | galion-realtest: box stack on 19500-19551, 12 scenarios | One harness for real-life tests | merge |
| voice-mouth | 5d776c1 | honest :8002 `/health`, galion-voice-serve | TTS showed 503 with no reason | merge |
| lan-discovery | fe5f0bd | mDNS/DNS-SD find + announce, pending rows approve/deny/join | LAN compute only after approval | merge |
| poseidon | 7d0c478 | openmind-queue: durable jobs, quotas, hard spend caps, VRAM routing | POSEIDON owns compute queueing | POSEIDON vs HEPHAESTUS ownership |
| nerve | 5802d0e | openmind-nerve: cortex, incidents, score, DDoS, replay | Live nerve said PASS while it missed ~59% of tunnel failures | nerve 8670 cutover |
| edge-boot | 753676e | omnode boot path, fail-closed pairing, seccomp gate | Edge devices boot safely | merge after ucpu |
| launch-gates | 79ee761 | REUSE, SBOM, cargo-deny, public-export check (nothing published) | Oct 31 launch compliance | SEC-1..5 (key rotation, history purge) |
| hermes-tools | 8ccdfcc | sandboxed fs/git/cargo tools for hermes-rs | Hermes could not do coding tickets | unit-file proposal |
| common | 26d98b0 | galion-common: sha256/base64/json/time/http, 9 crates migrated | Remove duplicated helpers | merge after tuner-cutover |

## Wave 2 (20:10-22:35 CEST), including the full-capacity swarm test (start 20:34)

| Slug / slot | Head | What it did | Why | Status | Owner decisions |
|---|---|---|---|---|---|
| integration-1 (22) | 8fd26f6 | Merged all 20 wave-1 branches (8 conflicts resolved); 950 tests pass, realtest stack 76/77 | One tested merge candidate for main | on GitHub (never ran GitHub CI, no PR) | merge order |
| gaia-poseidon | 498e3d2 | gaia-core POSEIDON forwards to openmind-queue (read-only budget) | Gaia uses the queue, never spends | on GitHub; real 60/60, 639 tests | POSEIDON vs HEPHAESTUS |
| gaia-hades-tokens | f3f41a2 | heal-tick uses Ed25519 scoped tokens | No random test tokens in Gaia | on GitHub; 627 tests | merge |
| gaia-land (01) | 9d3ec18 | ws/gaia merged; 52-system design; ownership registry with a test for unmapped crates | Every crate and unit has a Gaia owner | on GitHub (one stall, restarted) | 4 judgement-call mappings |
| flake-fix (27) | b2e69b9 | ~15 shared-path/fixed-port root causes; 34 full runs, 0 failures | 11/12 parallel runs failed before | on GitHub | merge |
| sec-runbooks (26) | c5fc269 | SEC-1..5 runbooks (not executed); galion-purge; SEC-3 perms in Rust | Prepare key rotation and history purge | on GitHub | run SEC-1..5 |
| console-static (34) | 6bd3580 | Rust static front + loopback API proxy; ~2.3 MB vs ~505 MB Vite | Live 8080 is a Vite dev server that serves source files | on GitHub, staged unit not installed | UI asset source, cutover |
| common-migrate-2 (28) | 123eab2 | 8 servers on galion-common http; Rust real tests; `Limits::with_env`; approval packet | Shared hardened HTTP | on GitHub | strict handling H1-H3, M1-M3, D1-D4, R1-R4, C1-C5 (SLAVIK-TODO 3) |
| sqlite-pure-spike | 98686ef | Turso rejected; redb + tantivy recommended | Bundled SQLite is C (Rust-only) | on GitHub | accept redb + tantivy |
| one-hardening (29) | fed3de4 | Zagreb timestamps, /proc not ps, optional tools with timeouts | Harden galion-one | on GitHub | pdftotext/curl (C) exception |
| memory-schema (36) | 7952626 | PRAGMA user_version, single-transaction migration, opt-in import | Safe schema upgrades | on GitHub | deploy (stamps live DB v4) |
| lan-routing (31) | e8169c9 | GPU -> LAN -> CPU fallback using approved rows only | Route only to trusted devices | on GitHub | merge |
| deploy-sim (33) | c5ab8f9 | galion-deploy rehearsal with simsup; findings F1-F6 | Cutovers must roll back safely | on GitHub | fix F3/F5/F6, cutovers |
| guard-rust | d5f7874 | galion-guard: Rust fail2ban replacement, 102/102 same bans on log replay | Rust-only rule (fail2ban is Python) | on GitHub | shadow-mode rollout, allowlist |
| voice-rust | 03d21a9 | galion-voice-serve with Chatterbox in candle + Whisper ear | TTS server was Python | on GitHub | voice clips + licence, watermark, deploy |
| console-ui-rust | c242fed | Server-rendered HTML console, 13/31 routes, no JS | Console option (c) recommended | on GitHub | JS for mic/streaming rooms |
| hermes-core | 45b7980 | `hermes` coordinator: 7 sectors, governor, ledger, HADES gate, readiness G1-G15 | Slavik: program Hermes first | on GitHub; readiness 11/15 | HADES key, merges, 7 decisions |
| settings-snapshot | b7afd6d | voice + swarm settings, TOP PRIORITY RULE, Revolut lift | Keep Slavik's rules in the repo | on GitHub | none |
| slavik-todo | 87fdd91 | Croatian mobile checklist (OpenRouter item done) | Slavik's to-do list on his phone | on GitHub | items 1 and 3 |
| fuzz-sast (25) | e78b2e7 | cargo-fuzz targets, 11-min runs 0 crashes, CodeQL/ClusterFuzzLite workflows | Security S1 | **pushed now (23:28-23:30)**, was box-only | libFuzzer/CodeQL licence ruling |
| hermes-enroll (32) | f2121a8 | Device enrollment: nonce store, scoped node credentials, revocation | Phones and edges join Hermes | **pushed now (23:28-23:30)**, was box-only | merge |
| nerve-wiring (30) | 8d7a924 | hermes-rs + openmind-memory push events through nerve-client | Nerve only polled before | **pushed now (23:28-23:30)**, was box-only; still adds a .sh real test | port .sh to Rust |
| tunnel-audit (35) | d35ef73 | cloudflared ingress audit + tunnel-lint (read-only) | 33 hostnames have no listener | **pushed now (23:28-23:30)**, was box-only | merge |
| planners x7 | plans/*.md | gaia, nerve, cortex, vps-ops, integration-ci, security, docs-whitepaper: 47 sessions, 200 tasks, 645 target / 407 useful agents | Plan the launch work by sector | done | items in each plan |
| non-rust inventory | plans/non-rust-inventory.md | Every non-Rust piece in the repo and on the VPS | Rust-only rule | done | OS packages, cloudflared, CI YAML, browser JS |

## Wave 3 (from 22:52 CEST)

| Slot | Slug | Head | What it did | Status | Owner decisions |
|---|---|---|---|---|---|
| 37 | integration-2 | df42e0f | Merged 4 Gaia branches on integration-1; 999 tests, clippy 0 | on GitHub; main merge held for verifiers | 5 pending ownership rows; how main is pushed |
| 38 | cutover-prep | (=787a54d) | Rust galion-deploy F1/F2, backup, runbook | running at 23:31; same sha as deploy-rust on GitHub | — |
| 39 (first run) | deploy-rust | 787a54d | Rust galion-deploy, F1-F6 fixed, rehearsals all pass | on GitHub | install before/after cutovers |
| 39b | swarm-record | this branch | This record, the GitHub safety sweep, VPS archive | on GitHub | none |
| 40 | landing-overview | 669dc18 | `/console` one-screen big picture, no JS | on GitHub | merge |
| 41 | hades-adversarial | ccaab42 | Stopped as a duplicate (no commits; same sha as hades-approval) | not pushed, nothing to save | — |
| — | hades-redteam | e6ceca7 | 82 attacks: 48 bypasses -> 0 | on GitHub | A record-only / B typed signed steps (recommended) / C as is |
| 42 | decision-memos | 1a0b851 | gaia-ownership, licences-tools, revolut-lift memos | on GitHub | 9 + 16 yes/no items |
| 43 | cortex-sensor | e536446 | Sensor trait + registry | on GitHub | serde preserve_order |
| 44 | cortex-sources | d692b9e | 52 systems: 13 connected / 9 partial / 30 none | on GitHub | strays, host-security owner |
| 45 | vps-baseline | f403eeb | Read-only VPS baseline ledger | on GitHub | — |
| 46 | whitepaper-evidence | 8c0ac37 | Agent ledger + concurrency limit timeline | on GitHub | — |
| 47 | rust-ports | 26a2a8e | guard test + galion-backup in Rust | on GitHub | deploy timing, one backup impl |
| 48 | build-info | db9924d | `--version` on 10 binaries in ~1 ms | on GitHub | add to health JSON? |
| 49 | health-land | f4a66ea | galion-health landed; rebuild byte-identical to live | on GitHub | merge (T1.2) |
| 50-54 | bench-harness / banking / wallstreet / vending / scoreboard | ea8315c / adda699 / 4b1e127 / a9daaf1 / f2ff169 | Benchmarks at 100% on mocks/paper/sim; no money moved | on GitHub | live Revolut reads, market data, scoring mode |
| 55 | cortex-s2-a | 9557eb3 | sshd brute-force + RustSec sensors | running; **pushed now (23:28-23:30)** as backup | — |
| 56 | cortex-s2-b | 51100e1 | drift + conn-rate sensors | running; **pushed now (23:28-23:30)** as backup | — |
| 57 | cortex-s2-c | dbec5c9 | proc-owner + surface sensors | running; on GitHub (pushed by its agent by 23:31) | — |
| 58 | cortex-s2-d | f3468ce | cortex events feed + hades sensor, off by default | on GitHub | enable in prod |
| 59 | bench-hermes | f7da261 | Run the benchmarks through hermes-rs | running; **pushed now (23:28-23:30)** as backup | — |

Other tracks today: the Galion OS repo was staged at `/workspace/swarm/galion-os-repo` (copied into `docs/swarm/galion-os-staging/`); it waits for the GitHub connection to create `galion-studio/galion-os`. The voice config is recorded in Q11/Q12 of the call record.

## Open owner decisions (rolled up)

1. HADES admin key (SLAVIK-TODO 1). It blocks Hermes readiness G9 and real approvals.
2. Strict request handling packet (SLAVIK-TODO 3, common-migrate-2).
3. Merge order and the main merge of integration-2, plus how main gets pushed (the helper is `swarm/*` only).
4. POSEIDON vs HEPHAESTUS ownership of compute; 4 + 5 Gaia ownership judgement calls.
5. HADES plan format after the red team: A / B (recommended) / C.
6. Live cutovers: tuner, nerve 8670, console 8080 (Vite dev server exposes source), desk, room; when to install the Rust galion-deploy.
7. Rust-only scope: OS packages, cloudflared, CI YAML, browser JS/TS, pdftotext/curl, bundled SQLite (redb + tantivy proposal), zig cc stopgap, libFuzzer/CodeQL licences.
8. Voice: real voice clips and licence, watermark, GPU vs CPU.
9. Benchmarks: live read-only Revolut access and key location, market data source, scoring mode. No money moves without a separate written approval.
10. Galion OS acceptance questions 1-7 (PLAN.md section 11).
11. SEC-1..5 key rotation and history purge.
