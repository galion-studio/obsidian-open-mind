<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Sector plan: `gaia`

Written 2026-10-20 20:40-21:00 CEST (+02:00), Europe/Zagreb. Planning only. No code, no commits, no builds, no VPS access was used.
Sources: PLANNER-BRIEF.md, RULES.md, WAVE2.md, CALL-RECORD.md, CALL-RECAP.md, /workspace/review-20261020/{PLAN-DIGEST,PROGRESS,REVIEW,CODE,SESSION,LIVE}.md,
integration-1 (8fd26f6) docs, plus read-only `git log`/`git status`/`git diff --stat`/`stat` in the worktrees named below. Launch deadline: Oct 31.

## 1. Sector scope
All Gaia work: GAIA core plus the 9 functions and 52 systems (design: ws/gaia 04778e6 `gaia-design.md` §4.2/§6, table in PLAN-DIGEST.md:97-109).
Functions and system counts: HADES 5, MINERVA 6, APOLLO 5, HEPHAESTUS 8, ARTEMIS 6, POSEIDON 4, AETHER 9, DEMETER 6, ELEUTHIA 3, for 52 total (PROGRESS.md:8).
In scope:
- gaia-core and the `gaia` CLI (11 crates).
- The self-heal heal-tick path.
- POSEIDON: openmind-queue and the gaia-poseidon adapter.
- The HADES approval gate (hades-gate).
- LAN routing: discovery to provider route to VRAM pool.
- Landing ws/gaia (gaia-land) and the ownership registry.
- The POSEIDON vs HEPHAESTUS ownership decision.

Out of scope here: nerve/cortex internals, survival/guardian, voice, cutovers of other services, launch compliance. Gaia only consumes these.

## 2. Current state (2026-10-20 ~20:40 CEST)
| Item | Branch / head | State | Evidence |
|---|---|---|---|
| gaia-core + 9 function crates + CLI | ws/gaia 04778e6. Merged via gaia-live, so already inside integration-1 8fd26f6 (`crates/gaia*` present) | Model/router, dry-run. Not on main 13a296c. 0/52 systems under live Gaia | PLAN-DIGEST.md:96,152; PROGRESS.md:86,151; STATUS.md:35 (GAIA-1 P) |
| gaia-land (slot 01) | local c15a4b7 (18:52) + 24 uncommitted paths (5 modified, 19 untracked: `gaia-core/src/ownership.rs`, `gaia/src/view.rs`, tests, LICENSE files, `scripts/real/gaia-land.sh`, `docs/third-party/gaia-land.md`) | **Not pushed. No file changed since 19:00, and its target dir has been idle since 18:59.** WAVE2.md:16 says "running", but it may be one of the 11 agents killed at ~20:14 (CALL-RECORD.md:7). No real-tests doc. Do not touch wt/gaia-land | `git status` and `find -newer` in wt/gaia-land (read-only); CALL-RECAP.md:32, contradiction #16 |
| heal-tick | swarm/gaia-live 4fbbb7f | First end-to-end Gaia path, box-only test services: probe, nerve event, DEMETER, HADES plan, approval, capped restart 3/h, nerve. Script exit 0. 593 workspace tests | integration-1 docs/real-tests/gaia-live.md:5-146 |
| heal-tick with real tokens | swarm/gaia-hades-tokens f3f41a2 (= gaia-live + hades-approval + work) | Ed25519 single-use scoped tokens replace the per-run test token. 15 cases incl. forged/replay/expired/wrong-scope/admin-safety/capped. Real 1 passed, heal_tick 10 passed | wt/gaia-hades-tokens docs/real-tests/gaia-hades-tokens.md:1-40. "627 tests" is not in any file (CALL-RECAP.md §4.1) |
| POSEIDON queue | swarm/poseidon 7d0c478 (in integration-1) | openmind-queue: durable log, leases, DLQ, quotas, hard-stop spend caps, VRAM routing, money kinds refused. **No auth (loopback only).** Refusal counters lost on kill -9 | integration-1 docs/poseidon.md:24-76 |
| gaia → POSEIDON | swarm/gaia-poseidon 498e3d2 (= gaia-live + poseidon + work) | gaia CLI forwards to openmind-queue, read-only for budgets, never spends. Real 60/60, Rust gated test ok, musl x86_64/aarch64 ok, android/armv7 check ok | wt/gaia-poseidon docs/real-tests/gaia-poseidon.md:21-75. "639 tests" undocumented |
| Hades gate | swarm/hades-approval ccaab42 (in integration-1) | Ed25519 tokens, nonce ledger with flock+fsync, admin-safety rule, disable/stop. `check_gaia` adapter covers heal-apply/rollback/deploy-switch. Not installed on the VPS. Adds `.cargo/config.toml` rustflags (dalek serial backend) | integration-1 docs/hades-gate.md; real-tests/hades-approval.md; integration-1.md:47-57,148-150 |
| LAN routing | swarm/lan-routing b153af8 = merge of lan-discovery fe5f0bd + poseidon 7d0c478, plus **uncommitted, actively edited at 20:39** (`openmind-discovery/src/{admit,cmd,routing}.rs`, `openmind-provider/src/{lib,fallback,route}.rs`, +237/-13) | Running. Adds `vram_gb` and `pending_endpoint` to provider rows; approved rows feed GPU→LAN→CPU `route::chain`; VRAM goes to the POSEIDON pool | `git diff --stat` in wt/lan-routing |
| lan-discovery | fe5f0bd (in integration-1) | mDNS/DNS-SD, opt-in /24 probe, approve/deny/join. Tested on 127.0.0.0/24 only; real LAN not done | CALL-RECAP.md:23 |
| integration-1 | 8fd26f6 (local, per record) | 20 wave-1 branches merged. 950 tests pass. Does **not** contain gaia-land, gaia-poseidon, gaia-hades-tokens or lan-routing (checked: those SHAs are absent from its repo) | integration/integration-1.md:9,162-164 |
| Ownership | two conflicting lists | ws/gaia design and gaia-land registry: POSEIDON owns compute (cgroup-detect→quotas; exo/llama/provider/tulu→vram-pool). VPS GAIA-4 `gaia-registry`: no POSEIDON or AETHER; those crates sit under HEPHAESTUS; it has GAIA and ARES as functions. gaia-poseidon already codes the POSEIDON side | gaia-land docs/gaia-ownership.md:13-37; session-evidence/docs-oct19-20.vps.md:978-1005; PLAN-DIGEST.md:110; CALL-RECAP.md §4.9 |

Merge-conflict surface, from read-only `git diff --stat`:
- gaia-live changed `crates/gaia/{Cargo.toml,src/lib.rs,src/main.rs}` over ws/gaia.
- gaia-poseidon changed the same three files again (+`src/poseidon.rs`).
- gaia-hades-tokens changed `crates/gaia/src/live_heal.rs` and hades-gate.
- gaia-land's uncommitted work also edits `gaia-core/src/lib.rs`, `gaia/src/lib.rs` and `gaia/src/main.rs`.

**Expect 3-way conflicts in `crates/gaia/src/{lib,main}.rs` (CLI subcommand dispatch).**

Box limits, measured 20:39: 8 CPUs, 15 GB RAM, 69 GB disk free, swarm target dirs 11 GB.

## 3. Sessions
Agent counts: **target** is what the 250-agent / ~50-per-group model would assign. **Useful max** is how many can work in parallel without colliding.
No Gaia task can genuinely use 50 agents. The hard limits:
- One CLI crate (`crates/gaia`) is the shared merge point.
- Only 9 function crates exist.
- 8 CPU cores.
- Port slots are 19NN0-19NN9, so at most 100 slots, with 19500-19551 already taken by realtest.

Every session also follows RULES.md:
- Release-binary real-life tests on the slot's ports.
- Read-only VPS GET probes only.
- Zagreb timestamps.
- `docs/real-tests/<slug>.md`.

### S1: Land everything Gaia on one branch (Oct 20-22)
Goal: one branch `swarm/gaia-integration` = integration-1 8fd26f6 + gaia-hades-tokens f3f41a2 + gaia-poseidon 498e3d2 + gaia-land (when pushed) + lan-routing (when pushed). All gates green.

| Task | Target | Useful max | Parallel units | Inputs / deps |
|---|---|---|---|---|
| S1.1 Finish lan-routing (already running, slot 31) | 1 | 1 | one branch | running agent. Push needed |
| S1.2 Resolve gaia-land. The slot 01 agent looks stalled since 19:00. The coordinator checks it: if alive, let it push; if dead, relaunch as `gaia-land-2` from c15a4b7 (pushed copy) with a fresh worktree. **wt/gaia-land is not touched either way** | 1 | 1 | one branch | coordinator check of slot 01; c15a4b7 |
| S1.3 Merge agent: integration-1, then gaia-hades-tokens, then gaia-poseidon, then gaia-land, then lan-routing, with normal `--no-ff` merges. Resolve `crates/gaia/src/{lib,main}.rs` by keeping every subcommand (`heal-tick`, `heal-plan`, `poseidon *`, `ownership`, `status --live`) | 1 | 1 | none (single merge point) | S1.1, S1.2 |
| S1.4 Verification fan-out on the merged head, one agent per unit: (a) fmt/clippy/workspace tests, (b) ucpu gate (x86_64 + aarch64 musl builds; android + armv7 check), (c) re-run real scripts gaia-live, gaia-hades-tokens, gaia-poseidon, hades-approval, poseidon, lan-discovery, gaia-land on their own slots, (d) repo-check / public-export / REUSE / cargo deny | 8 | 4 (CPU-bound: 8 cores) | per gate; per real script | S1.3 |

Success criteria (real-life):
- All 7 real scripts exit 0 on the merged head. The results match the recorded ones: 60/60 gaia-poseidon; the 15-case token table; 3/3 restarts then capped.
- Workspace tests are ≥ 950 with 0 failures.
- `gaia ownership` lists every workspace crate, and the workspace_ownership guard passes (including openmind-queue and gaia-live, which gaia-land did not know about).
- Test counts are written into `docs/real-tests/gaia-integration.md`. This fixes the undocumented 639/627.

Slavik approves: the merge order above; that he merges integration-1 then gaia-integration (no PR opened by agents); `.cargo/config.toml` rustflags (integration-1.md:148).

### S2: Ownership decision and registry reconciliation (Oct 21-22, gated on Slavik)
Goal: one nine-function list and one crate→system→function table in code, docs and STATUS.md.

| Task | Target | Useful max | Parallel units | Deps |
|---|---|---|---|---|
| S2.1 Decision memo for Slavik (read-only; see §5): option A = design (POSEIDON owns compute), option B = VPS GAIA-4 (HEPHAESTUS owns compute; no POSEIDON/AETHER). Lists every crate that moves and the code already written against A | 1 | 1 | none | now |
| S2.2 Apply the decision: `gaia_core::ownership`, gaia-poseidon `FunctionId::COMPUTE`, `docs/gaia-ownership.md`, STATUS.md GAIA-2 row. Add rows for new crates (openmind-queue, gaia-live, hermes-tools, galion-common, galion-realtest, galion-voice-cpu, nerve-client) | 2 | 1 | single registry file | S1.3, Slavik decision |
| S2.3 Consistency checks: 52 vs 51 systems, ARES→ARTEMIS ecosystem-census, hermes-rs as POSEIDON budget-caps (PROGRESS.md:123) vs ARTEMIS hermes-agents (gaia-land table) | 1 | 1 | none | S2.2 |

Success: `cargo test -p gaia-core --test workspace_ownership` fails on a deliberately unmapped crate and passes on the real tree. `gaia systems` prints 52. `gaia ownership` matches the docs table byte for byte (a generated table, not a hand-edited one).

Slavik approves: the A/B choice; confirming 52 systems; dropping ARES as a function.

### S3: Heal-tick from one-shot to supervised loop (Oct 22-25)
Goal: heal-tick runs as a loop/timer against box test services, with shared ledgers, real HADES keys and fault coverage. A **staged** unit is ready, not installed.

| Task | Target | Useful max | Parallel units | Deps |
|---|---|---|---|---|
| S3.1 Tick loop / guardian-tick timer integration (gaia-live.md:152 says "no daemon or timer loop yet"). Global restart ledger so a new `--state` dir cannot reset the 3/h budget (gaia-live.md:153) | 2 | 1 | gaia-live crate | S1 |
| S3.2 Fault-injection scenarios, one agent per scenario, own port slot: hang (accepts but never answers), slow /health, bad JSON, flapping, port stolen by another process, nerve down, nerve returns no event, hades disabled mid-run, ledger corrupted/torn, clock skew (CEST→CET on Oct 25 03:00), concurrent ticks on one service | 12 | 6 | per scenario (each is its own test file + script section) | S3.1 |
| S3.3 Staged systemd unit and timer text for `gaia heal-tick` plus a cutover doc (text only; follows docs/cutover-approval.md) | 1 | 1 | doc | S3.1 |
| S3.4 Read-only VPS classification: `gaia status --live` / heal-tick in plan-only mode against live endpoints over ssh GET (8420, 8670, 8646, 8650...). No apply, no token, no files written on the VPS | 1 | 1 | none | S1 |

Success (real-life):
- Every scenario produces the documented outcome on the nerve: `gaia.heal.{refused,capped,failed}`, never a restart without a valid token.
- 0 leftover pids and closed ports after teardown.
- The live dry-run classifies every live service with zero POSTs. Its output is compared with the survival-heal live dry-run (survival-heal.md).

Slavik approves: any unit install, any heal-apply on a live service, and where the Ed25519 admin seed lives (never on the VPS).

### S4: POSEIDON and the job queue hardening (Oct 22-26)
Goal: openmind-queue is safe to serve beyond loopback and is the queue Gaia functions submit to.

| Task | Target | Useful max | Parallel units | Deps |
|---|---|---|---|---|
| S4.1 Token/auth layer before any LAN bind (poseidon.md: "No authentication"). Reuse hades-gate Ed25519 verification or a shared-secret file (mode 600) | 2 | 1 | server module | S1 |
| S4.2 Persist refusal counters across kill -9 (poseidon.md limits) | 1 | 1 | store module | S1 |
| S4.3 `gaia-poseidon` real `SubordinateFunction::handle` impl, replacing vocabulary-only coupling (poseidon.md:69-72) | 1 | 1 | gaia-poseidon crate | S1, S2 |
| S4.4 Crash/soak suite: kill -9 loops during ack, torn tail, compaction under load, two servers on one dir (flock), 10k-job soak with RSS recorded | 6 | 4 | per scenario | S4.2 |
| S4.5 Fuzz targets (coordinate with the fuzz-sast sector): queue.log record parser, HTTP request parser, JSON job body, hades token parser | 4 | 4 | per fuzz target | fuzz-sast harness |
| S4.6 Swarm use case: hermes-tools / agents submit work through `gaia poseidon submit` with per-agent caps (no paid calls; caps are numbers only) | 2 | 1 | adapter | S4.1, S4.3 |

Success:
- Restart replays with 0 lost jobs, budgets, caps, devices or counters.
- An unauthenticated non-loopback request gets 401.
- 1 h of fuzzing per target with no crash.
- The soak shows bounded RSS. Spend caps hard-stop under concurrency (no overspend in the ledger sum).

Slavik approves: POS-1 (Hermes cap $5 → $10, unrecorded approval, STATUS.md:67), POS-2 (any GPU/RunPod spend, 1500 GB target), and binding the queue on the LAN.

### S5: HADES gate wiring (Oct 22-26)
Goal: every destructive Gaia path goes through `hades_gate::adapter::check_gaia`, and the gate's own surfaces are hardened.

| Task | Target | Useful max | Parallel units | Deps |
|---|---|---|---|---|
| S5.1 Wire `rollback` and `deploy-switch` plans (dry-run executors only; deploy/rollback.sh stays plan-only) | 2 | 2 | per action | S1 |
| S5.2 Admin-safety rule corpus: one agent per refusal category (destructive cmds, secret exposure, account/ssh/firewall, unreadable plans, protected targets) with adversarial plan files | 5 | 5 | per category | S1 |
| S5.3 Key custody and rotation design doc (keygen on the owner's device, pubkey only on host, rotation, revocation via disable file) | 1 | 1 | doc | none |
| S5.4 Resolve the rustflags fragility: `RUSTFLAGS` env silently drops the serial-backend cfg and `simd_free.rs` fails (integration-1.md:148-150). Options: dalek feature selection or a build.rs check | 1 | 1 | hades-gate | Slavik SIMD ruling |

Success:
- Rollback/deploy-switch execute nothing without a valid token, and are refused with one when admin-safety triggers.
- 100% of the corpus is refused.
- The token-parser fuzz (S4.5) is clean.
- A build with `RUSTFLAGS` set either still passes or fails loudly at compile time.

Slavik approves: SIMD exception / rustflags approach; key custody.

### S6: LAN routing end to end (Oct 21-27)
Goal: an approved LAN device shows up in the POSEIDON pool and is chosen by the GPU→LAN→CPU route.

| Task | Target | Useful max | Parallel units | Deps |
|---|---|---|---|---|
| S6.1 Finish and push lan-routing (= S1.1) | - | - | - | - |
| S6.2 `/vram/sync-providers` fed from discovery-approved rows with `vram_gb`; re-approval on `pending_endpoint` | 1 | 1 | queue + provider | S1 |
| S6.3 Box real test with two loopback "providers" (fake GPU and LAN endpoints on slot ports): approve, route, provider dies, falls back to CPU | 2 | 2 | per scenario | S6.2 |
| S6.4 Real-LAN mDNS test on a second physical device (lan-discovery.md:121). Needs hardware | 1 | 1 | none | Slavik device |

Success: the route chain picks GPU, then LAN, then CPU in the real test. A pending or rejected row is never routed. A moved device needs re-approval. The VRAM summary gap is computed against 1500 GB.

Slavik approves: a real LAN device / network to test on.

### S7: The nine functions own real systems (Oct 23-29)
Goal: move from 0/52 under Gaia (PROGRESS.md:151) to at least one read-only real adapter per function, with `gaia status --live` showing real state per system.

| Task | Target | Useful max | Parallel units | Deps |
|---|---|---|---|---|
| S7.1 One agent per function crate: a read-only status adapter for the function's I/L systems (e.g. APOLLO memory-gateway GET 8420 contract, AETHER nerve 8670, DEMETER guardian-tick/survival, MINERVA repo-check, ELEUTHIA galion-one health, ARTEMIS hermes-rs caps, HEPHAESTUS sandbox/desk). HADES and POSEIDON are covered by S3-S5 | 7 | 7 | per function crate | S1, S2 |
| S7.2 Larger functions get a second agent: AETHER (9 systems), HEPHAESTUS (8) | 2 | 2 | per system group inside the crate | S7.1 |
| S7.3 CLI/registry integrator: wires the adapters into `crates/gaia`. Serialised merge point | 1 | 1 | none | S7.1 |
| S7.4 ART-1 (spawn/retire) and ELEU-1 (seeds) leave sim-only state, box-only (STATUS.md:63-64) | 2 | 2 | per item | S7.1 |

Success: `gaia status --live` on the box (and read-only against the VPS) reports a real state for ≥ 1 system per function. No adapter writes or POSTs. The rogue/containment test still passes with real adapters.

Slavik approves: none for read-only. Any write adapter goes through HADES and needs approval.

### S8: Verification, docs, launch alignment (Oct 27-30)
| Task | Target | Useful max | Parallel units | Deps |
|---|---|---|---|---|
| S8.1 Add Gaia scenarios to galion-realtest (heal-tick, poseidon, hades) so CI runs them (`--with-nerve`, integration-1.md:160) | 3 | 3 | per scenario | S3-S5 |
| S8.2 STATUS.md rows GAIA-1/2, HADES-1, ART-1, ELEU-1, POS-*, recomputed by `scripts/status-percent.sh`. PROGRESS-style 52-system table refreshed | 1 | 1 | single file | S7 |
| S8.3 Public-export pass for Gaia crates (IPv4 literals, REUSE, SPDX-in-string case of gaia-minerva) | 1 | 1 | none | S7 |

Success: realtest CI green with the Gaia scenarios, and the STATUS table matches the code.

Slavik approves: turning on `--with-nerve` in CI; merging to main.

## 4. Totals
| Session | Tasks | Target agents | Useful max |
|---|---|---|---|
| S1 Land | 4 | 11 | 7 |
| S2 Ownership | 3 | 4 | 3 |
| S3 Heal-tick loop | 4 | 16 | 9 |
| S4 POSEIDON/queue | 6 | 16 | 12 |
| S5 HADES gate | 4 | 9 | 9 |
| S6 LAN routing | 3 (+S6.1 = S1.1) | 4 | 4 |
| S7 Nine functions | 4 | 12 | 12 |
| S8 Verify/docs | 3 | 5 | 5 |
| **Total** | **31** | **77** | **61** |

The owner's model of ~50 per task fits nowhere here. The largest single task is S3.2 (12 target, 6 useful). Useful max is a concurrency ceiling per task, not a simultaneous total. The dependency chain (S1 → S2 → S7) means at most ~25-30 can usefully run at the same moment.

### Realistic schedule at 16 concurrent agents (box-wide)
Assumption: gaia gets 4-5 of the 16 slots, because other sectors share the box. That fits 8 cores at `-j 3`.

| Window (CEST/CET) | Gaia slots | Running |
|---|---|---|
| Oct 20 eve – Oct 21 | 3 | S1.1 lan-routing (running), S1.2 gaia-land check/relaunch, S2.1 memo |
| Oct 21 – Oct 22 | 5 | S1.3 merge, then S1.4 ×4 verifiers |
| Oct 22 – Oct 24 | 5 | S2.2 (after decision), S3.1, S4.1, S4.2, S5.1 |
| Oct 24 – Oct 26 | 5 | S3.2 ×2 (scenarios batched 6 per agent pair), S4.4, S5.2 ×2 (categories batched) |
| Oct 25 03:00 | - | CEST→CET switch; S3.2 clock scenario must run across it |
| Oct 26 – Oct 28 | 5 | S7.1 ×4 (batched: 7 functions over 4 agents), S6.2/S6.3, S4.3 |
| Oct 28 – Oct 29 | 4 | S7.2, S7.3 integrator, S7.4, S4.5 fuzz (if fuzz-sast harness ready) |
| Oct 29 – Oct 30 | 3 | S8.1-S8.3 |
| Oct 31 | 0 | owner merges / launch gates |

About 34 agent-runs across 10 days at ≤ 5 concurrent. If gaia got all 16 slots, S3/S4/S5/S7 would compress by about 2 days. They would still wait on S1 and on Slavik's S2 decision.

## 5. POSEIDON vs HEPHAESTUS: facts for the decision (S2.1)
- **Design (ws/gaia 04778e6):** POSEIDON = compute resources and budgets (vram-pool, quotas, queues, budget-caps). HEPHAESTUS = fabrication (terminal, cargo-build, release-artifacts, galion-desk, omnode-sandbox-runner, crafting, console, deploy-switch). PLAN-DIGEST.md:103,105.
- **VPS GAIA-4 registry (2026-10-19 18:48):** no POSEIDON. cgroup-detect, openmind-exo/llama/provider/route/fallback/origin and one-* sit under HEPHAESTUS. session-evidence/docs-oct19-20.vps.md:978-1005.
- **SOURCE-OF-TRUTH.md** already picks ws/gaia ("52 systems, approval-token HADES path") over VPS gaia-core + gaia-registry (PLAN-DIGEST.md:110). `gaia-registry` is not on main 13a296c.
- **Code written against the design:** openmind-queue (`gaia::REGISTRY_ROW` → POSEIDON), gaia-poseidon 498e3d2 (FunctionId::COMPUTE), the gaia-land registry (moves cgroup-detect→quotas and exo/llama/provider/tulu→vram-pool), and lan-routing (VRAM for the POSEIDON pool).
- **Undisputed:** openmind-sandbox stays HEPHAESTUS terminal in both lists.
- **Planner's reading** (a recommendation, not a decision): option A matches SOURCE-OF-TRUTH and all wave-1/2 code. Option B would mean reverting gaia-poseidon's ownership and re-homing 6 crates.

## 6. Items needing Slavik (sector)
1. POSEIDON vs HEPHAESTUS (A/B); 52 systems; ARES dropped.
2. Merge order and his merges: integration-1, then gaia-integration (S1.3 order).
3. `.cargo/config.toml` rustflags / SIMD exception (hades-gate).
4. Ed25519 admin key custody: seed off the VPS, pubkey on the host.
5. Any install of a heal-tick timer, hades-gate or openmind-queue unit on the VPS. Any live heal-apply.
6. POS-1 cap record; POS-2 GPU/RunPod spend; LAN bind for the queue.
7. A real LAN device for the mDNS / LAN-route test.
8. Relaunching gaia-land if slot 01 is dead (its worktree stays untouched).
9. `--with-nerve` in realtest CI.
