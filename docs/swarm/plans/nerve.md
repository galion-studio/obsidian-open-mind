<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Sector plan: nerve (openmind-nerve, galion-nerve/galion-health live, nerve-client wiring, cutover, event coverage)

Written 2026-10-20 ~20:45 CEST (+02:00) by the nerve sector planner. Planning only: no code, commits, builds, pushes or VPS writes.
One read-only ssh probe was made at 2026-10-20T20:39:42+02:00 (`systemctl show`, `ls -l`, GET 8670 `/self-eval.md` and `/signals`).
Launch deadline: Oct 31. DST: CEST (+02:00) ends 2026-10-25 03:00, then CET (+01:00).

## 0. Scope
In scope:
- `crates/openmind-nerve`, except the cortex module: collect, probe, hub, engine, incident, score, replay, server, reflex, alerts, state, writer, config, logscan, main.
- `crates/nerve-client`.
- nerve-client wiring into hermes-rs and openmind-memory (slot 30).
- The source and provenance of the live binaries galion-nerve and galion-health.
- Nerve cutover (slot 33 deploy-sim rehearsal, then the live cutover, which needs the owner).
- Event coverage of every service, live and swarm-built.

Out of scope: the cortex module (separate planner). That covers `crates/openmind-nerve/src/cortex.rs`, the vulnerability/bleeding catalogue, the DDoS detector policy in `access.rs`, and `docs/cortex.md`. The interface with the cortex planner is the `state["cortex"]` JSON that `score.rs` and the `/cortex` and `/ddos` routes consume (server.rs:272-273). Any change to that shape has to be agreed with the cortex planner.

## 1. Current state (facts, with sources)
| item | SHA / value | source |
|---|---|---|
| main | 13a296c. It has only a 189-line JSONL `Writer` openmind-nerve lib, plus `openmind-survival/src/nerve.rs`, `NerveEvent` in openmind-schema and docs nerve-align/nerve-match. It has no nerve-client and no galion-health. | `git ls-tree 13a296c`; CODE.md:119 |
| swarm/nerve | 5802d0e (19:39:59 +02:00): 35 files, +9549. Runnable openmind-nerve (8328 src lines in 19 modules), nerve-client, deploy/nerve unit + install.sh, real tests | `git show --stat 5802d0e`; docs/real-tests/nerve.md |
| nerve evidence | Replay of the live capture (19:23-19:25) = **FAIL 61/100 D**: 3 incidents (edge: 13 hostnames -> 10 closed ports, 59% of tunnel requests fail; mouth 8002 stuck loading for 1.9 d; console scanner probes) plus 5 unowned listeners (8431-8433, 8652, 8653). Box run: kill/restart detection 15 s; flood 100.5 req/s detected; RSS 4.7 MiB; gated test `real_box` 1 passed; x86_64/aarch64 musl build, android/armv7 check | real-tests/nerve.md §2-5 |
| live galion-nerve (VPS 8670) | pid 1466797, active since 2026-10-18 22:35 CEST, NRestarts 0, binary 816456 B (Oct 18). **Reflexes LIVE** (`NERVE_DRY_RUN=0`, runs as root, restart + rollback allowlist). Self-eval at 20:34:35 +02:00: **PASS** (0 errors, 3 warnings), "system degrading". `/signals` = `{"rejected":0,"sources":[]}`, so **no service pushes today** | ssh probe 20:39 +02:00; ws-nerve.md "Reflexes"; deploy/nerve/galion-nerve.service |
| live galion-nerve source | ws/nerve a80c5b3 + a63b730 (2026-10-18 22:36 +02:00), based on 317d1d4 and **not an ancestor of main**. Crate name `crates/galion-nerve` (later merged into openmind-nerve on swarm/nerve) | `git merge-base --is-ancestor` = no; LIVE.md:49,64 |
| live galion-health (VPS 8660) | pid 1374810, binary 503512 B (Oct 18 22:14 CEST), 12 checks OK. **Correction to the brief:** its source is **ws/foundation e43d7ab** (`crates/galion-health/src/main.rs`, 150 lines, plus galion-deploy, galion-backup and the timer). It is not ws/nerve a63b730, and it is not in main | LIVE.md:48; ws-nerve.md "Coordination with ws/foundation"; base.git ws/foundation |
| a63b730 wiring | nerve-client in hermes-rs `api::spawn` and memory `main`: started event, 60 s heartbeat, 5xx. **Not in the live hermes-rs/memory binaries.** Those were built from rust-v1/switch-gaps | LIVE.md:64; REVIEW.md:100 |
| slot 30 swarm/nerve-wiring | head 5802d0e plus **10 uncommitted files** (474+/48-). nerve-client gets a bounded queue (256), 1 worker, drop/fail counters, `observe_request`, `cap_hit` (spend_cap, conn_cap), `NERVE_SLOW_MS`. hermes-rs gets http.rs conn cap and spend.rs. memory gets main.rs and http.rs. The hub gets `incident_signals` + 1 test. **At 20:4x the tree does not compile:** the hermes-rs `api.rs:63` insert landed inside the `/// What a chat request…` doc comment (mid-edit) | `git -C wt/nerve-wiring diff` |
| slot 33 swarm/deploy-sim | head 5fd1611 (tuner-cutover). Clean tree. No nerve rehearsal artefacts yet | `git -C wt/deploy-sim status` |
| integration-1 | 8fd26f6 has nerve 5802d0e merged (R4: nerve's main.rs over gaia-live's). gaia-live heal-tick -> `openmind-nerve serve` cross-test: 5 events. realtest `--with-nerve` 81/81 locally, not in CI | docs/integration/integration-1.md:25,68-73,160; real-tests/integration-1.md:56-82 |
| event formats (3) | (a) nerve-client `Signal {source,kind,name,value,msg}` -> `POST /signal`. (b) `openmind_nerve::Event`/schema `NerveEvent {ts,source,kind,severity,data}` -> `POST /event` + JSONL (gaia-live) and file-only `--nerve PATH` (guardian-tick). (c) galion-boot `to_nerve` prints (b) to stdout and nothing consumes it. server.rs:165 maps (b) to (a) | server.rs; guardian-tick/src/heal.rs:17-19; nerve-align.md |

### Event coverage today (live units, LIVE.md:39-41; default_specs in config.rs:92-150)
| service (live unit, port) | nerve pull probe | pushes events | note |
|---|---|---|---|
| openmind-memory 8420 | yes /health | no (wired on branch only) | slot 30 |
| hermes-rs 8642 | yes /health | no (wired on branch only) | slot 30 |
| link-watch-rs | unit+journal | no | |
| galion-desk 8646 | yes | no | live binary is the Oct 10 pre-workspace build |
| tuner 8650 | yes | no | |
| galion-one 8091 / mouth 8002 | yes | no | mouth 503 for 1.9 d |
| console 8080 (Vite) | yes / | no | scanner hits seen |
| galion-health 8660 | yes /status.json | no (pull only) | |
| cloudflared 20241 / tuner-quick 20242 | metrics | n/a | dead routes now detected by logscan |
| **tanga-proto 8671** | **no spec** | no | coverage gap |
| **galion-backup.timer** | **no spec** | no | only journal visibility |
| stray 8431-8433, 8652-8653 | flagged as vulnerability | n/a | owner cleanup |
Swarm binaries with no nerve output yet: galion-one, galion-desk, desk-hermes, tuner, link-watch, room, galion-origin, openmind-queue, hades-gate, galion-voice-serve, openmind-discovery, hermes-tools, one-defence, persona-plex-relay, openmind-mcp-stack, galleon-arena, galion-boot, edge/openmind-node. guardian-tick writes a file sink only, and gaia-live already POSTs `/event`.

## 2. Sessions

### S1. Stabilise and prove provenance (start now)
Goal: every nerve-side live binary has its source and a reproducible build in the repo, the wiring compiles and is proven, and the replay corpus is regression-grade.

| task | target agents | useful max | parallel units | inputs / deps | success criteria (real-life) |
|---|---|---|---|---|---|
| T1.1 finish slot 30 nerve-wiring (already running) | 1 | 1 | one change set: hermes-rs + memory + nerve-client + hub share one Cargo.lock | 5802d0e; fix api.rs:63 splice first | fmt/clippy/workspace tests green. Box: openmind-nerve on 19300 plus real hermes-rs (19301) and memory (19302) with `NERVE_URL=http://127.0.0.1:19300`. `/signals` lists both sources; a forced 5xx and a spend_cap show as incident signals; with the nerve killed, hermes p99 latency over 1000 requests is within 5% of NERVE_URL unset, and `dropped`/`failed` counters rise in the next heartbeat |
| T1.2 land galion-health (+galion-deploy, galion-backup) from ws/foundation e43d7ab onto 13a296c (`swarm/foundation-land`) | 2 | 2 | (a) galion-health, (b) galion-deploy + galion-backup/timer | base.git ws/foundation | Release build on box answers `/status.json` with the same key set as live 8660 (read-only GET diff, recorded). galion-deploy rollback drill on box with /bin/false (as infra/DEPLOY.md did on Oct 18) |
| T1.3 provenance of live galion-nerve 816456 B | 1 | 1 | one | ws/nerve a63b730 | x86_64 musl rebuild of a80c5b3/a63b730: size and `strings` compared with the VPS binary (read-only `sha256sum` over ssh). The rebuilt old binary's `once --md` on the replay capture gives **PASS**, which proves the old verdict bug comes from the source (cloudflared ERR at priority 6, 503 "loading" for days) |
| T1.4 replay corpus and golden verdicts | 5 | 4 | 1 read-only capture agent (several captures 30 min apart; VPS GETs only, to stdout) plus fixture authors per incident class: edge dead route, stuck loading, scanner, unowned listener, restart loop, disk/RAM trend, memory leak, push-signal incidents | nerve-capture/, scripts/real/nerve-capture-vps.sh | `cargo test -p openmind-nerve` has one golden test per class (expected incidents, score, grade). Every capture replays deterministically twice |
S1 approvals: none (read-only VPS probes only). The ws/foundation land branch is a merge decision for Slavik.

### S2. Event contract and per-service coverage (widest fan-out)
Goal: every service in the tables above either pushes events through one contract or is explicitly probe-only, and a gate enforces this.

| task | target | useful max | parallel units | deps | success criteria |
|---|---|---|---|---|---|
| T2.1 event contract `docs/nerve-events.md` + nerve-client API freeze | 1 | 1 | one (must be first) | T1.1 merged | One kind/name catalogue (started, heartbeat, http_5xx, slow_request, spend_cap, conn_cap, probe, heal.*, incident.*) and a rule mapping `/event` (NerveEvent) to `/signal`. galion-boot `to_nerve` and guardian-tick file sink get an optional socket. Contract tests in nerve-client and openmind-nerve |
| T2.2 per-service wiring (one agent per crate) | 20 | 18 | galion-one, galion-desk, desk-hermes, tuner, link-watch, room, galion-origin, openmind-queue, hades-gate, guardian-tick, galion-voice-serve, openmind-discovery, hermes-tools, one-defence, persona-plex-relay, openmind-mcp-stack, galleon-arena, galion-boot, edge/openmind-node (android/armv7 check), gaia-live (align to contract) | T2.1; base swarm/nerve-wiring | Per crate: real release binary on the box slot's port with `NERVE_URL` pointing at a box nerve. Start, heartbeat and one induced error appear in `/signals` and the expected incident opens. With NERVE_URL unset there is zero extra threads/sockets (`/proc/<pid>/task` count unchanged) |
| T2.3 nerve-side specs: tanga-proto 8671, galion-backup.timer, room, origin, queue, hades-gate, voice-serve; known-port list | 1 | 1 | one (config.rs is a conflict hotspot) | T2.1 | Replay of a capture shows tanga-proto and backup as components; no new false incidents on the corpus from T1.4 |
| T2.4 coverage gate | 1 | 1 | one | T2.2 | A realtest scenario (or repo-check rule) fails if a workspace bin neither depends on nerve-client nor is listed probe-only. Turn `--with-nerve` on in `realtest.yml` |
| T2.5 integrator for S2 merges | 1 | 1 | one | all T2.x | One branch `swarm/nerve-coverage`: Cargo.lock conflicts resolved, workspace tests green, `realtest stack --with-nerve` all PASS |
S2 honest limit: about 18 agents. Each wiring unit is about 20-60 lines in one crate, so more agents per crate would only collide. Agents 19-50 would idle or fight over Cargo.lock and config.rs.
S2 approvals: none for the code. Enabling push on live services belongs to S5.

### S3. Nerve hardening (parallel with S2 once T2.1 is done)
| task | target | useful max | parallel units | deps | success criteria |
|---|---|---|---|---|---|
| T3.1 fuzz/property targets for nerve parsers | 6 | 6 | `/signal` body, `/event` body + HTTP `read_req`, journal JSON, cloudflared metrics + logscan lines, access log CLF/JSON, `ss -ltn` + replay loader | coordinate with fuzz-sast (slot 25) on tooling. cargo-fuzz/libFuzzer is C++; if RULES.md "no C deps" is read strictly, use a std-only property harness | 10^6 inputs per target with no panic, OOM or hang (>1 s); each crash fixed with a regression test |
| T3.2 reflex safety on the new verdicts | 2 | 2 | (a) dry-run replay of reflex decisions across the corpus, (b) allowlist/never-touch audit | T1.4 | Replaying today's capture in dry-run gives zero restart of cloudflared and zero rollback (the edge dead-route incident must be alert-only). audit.jsonl lines recorded |
| T3.3 soak + client backpressure | 2 | 2 | (a) 12 h box soak at 30 s interval with 8 pushing services, (b) dead/slow nerve vs hermes/memory latency | T1.1, part of T2.2 | RSS < 32 MiB (self-rss check), cycle < 50% interval, 0 panics; client drops counted and never block |
| T3.4 DST and clock | 1 | 1 | one | none | Fake-clock replay over 2026-10-25 02:00-03:00 Zagreb: self-eval stamps go from +02:00 to +01:00, 2 h trend windows do not break and no false "restart" appears from monotonic vs wall time |
| T3.5 cross-target re-verify after S2 | 1 | 1 | one | T2.5 | x86_64/aarch64 musl build + qemu smoke (`/health`, `POST /signal`), android/armv7 `cargo check`, ucpu gate pass |
| T3.6 consumer compatibility | 1 | 1 | one | none | galion-realtest live probe expects 8670 dashboard body `Contains("galion-nerve")` (live.rs:148-152) and `/healthz`; the old API (`/state`, `/self-eval(.md)`, `/alerts`, `/audit`, `/signals`) is byte-shape compatible or the consumers are updated |
S3 approvals: Slavik rules on the GPL/C tooling question for fuzzing, if libFuzzer is used.

### S4. Cutover rehearsal (slot 33 deploy-sim, running)
| task | target | useful max | parallel units | deps | success criteria |
|---|---|---|---|---|---|
| T4.1 nerve cutover rehearsal on the box | 4 | 4 | (a) state-dir compat: the new binary loads an old `/var/lib/galion-nerve` (state.json, reflex.json budgets, mode, self-eval history) copied from fixtures, not from the VPS; (b) unit + install.sh with `DRY=1`, `systemd-analyze verify`, env diff vs live; (c) rollback drill to the 816456 B binary rebuilt in T1.3 via galion-deploy with health-url `/healthz`; (d) runbook `docs/cutover/nerve.md` | T1.2, T1.3, T3.2, T3.6 | Simulated install, health in under 20 s, rollback restores the old binary and the old verdict. Runbook lists every command and the expected output |
| T4.2 push-enable rehearsal (hermes-rs, memory) | 1 | 1 | one | T1.1; hermes/memory cutover sectors | Box sim of unit drop-ins `Environment=NERVE_URL=http://127.0.0.1:8670` + restart; `/state` shows `"connection":"push+pull"` |
S4 approvals: none (box only).

### S5. Live cutover (owner-gated)
| task | target | useful max | units | deps | success criteria |
|---|---|---|---|---|---|
| T5.1 replace live galion-nerve with openmind-nerve, **reflexes in dry-run** (`DRY=1`), then 24 h observe, then live reflexes | 2 | 2 | operator + independent observer | S4, Slavik approval | 8670 `/self-eval.md` shows FAIL with the same 3 incidents as the replay. `/healthz` 200. realtest live 8670 checks pass. No unit restarted by the nerve during the dry-run window (audit.jsonl dry-run lines only) |
| T5.2 enable push on hermes-rs/memory (and later each S2 service as it cuts over) | 1 | 1 | one per service cutover, run in series | T5.1, hermes/memory redeploy from a branch containing T1.1 | `/signals` sources >= 2 and push_age_s < 120. Hermes/memory /health unchanged |
S5 approvals (Slavik): live binary replacement of galion-nerve; dry-run to live reflex switch; unit drop-ins and restarts of hermes-rs and openmind-memory; the timing relative to the Oct 25 DST change (avoid 02:00-04:00 that night).

### S6. Remediation follow-through (owner actions, nerve verifies)
| task | target | useful max | units | deps | success criteria |
|---|---|---|---|---|---|
| T6.1 track the nerve's open incidents to closure | 1 | 1 | one | T5.1 | Each owner action is followed by a read-only `/self-eval` showing the incident resolved and the score up: edge ingress/DNS for 13 hostnames (nerve.md:80), mouth 8002 restart (nerve.md:89), the 5 stray listeners, Vite console exposure (console-static), SEC-5 tuner-quick |
S6 approvals (Slavik): cloudflared ingress/DNS, mouth restart, killing the stray processes, the console switch.

## 3. Merge order
1. swarm/nerve 5802d0e (already in integration-1 8fd26f6; Slavik merges). 2. swarm/nerve-wiring (T1.1, based on 5802d0e). 3. swarm/foundation-land (T1.2; independent). 4. T1.3/T1.4 test and doc branches. 5. T2.1 contract. 6. T2.2 crates through the T2.5 integrator, on top of 2 and 5. Each touches Cargo.lock, so they must be serialised there. T2.2 galion-one/desk/tuner/room must merge after their wave-1 branches (one-probe, desk-linkwatch, tuner-cutover then common, room). 7. T2.3/T2.4. 8. S3 fixes. 9. S4 runbook. 10. S5 only after Slavik approves.

## 4. Schedule at the real limit (16 agents across ALL sectors; nerve assumed to get 4-5 slots)
| when (Zagreb) | nerve slots in use | running |
|---|---|---|
| Oct 20 eve to Oct 21 morning | 4 | T1.1 (slot 30), T4.1 prep (slot 33), T1.3, T1.2(a) |
| Oct 21 | 5 | T1.2(b), T1.4 x2, T2.1, T3.4 |
| Oct 22 to Oct 23 | 5 | T2.2 in 4 batches of 5 crates (about half a day each), T2.3 |
| Oct 24 | 5 | T2.5 integrator, T2.4, T3.1 x3, T3.2 |
| Oct 25 (DST night, no cutover) | 5 | T3.1 x3, T3.3 soak (12 h), T3.5, T3.6 |
| Oct 26 | 4 | T4.1 finish, T4.2, then hand to Slavik for S5 approval |
| Oct 27 to Oct 28 | 2 | T5.1 dry-run 24 h, then live reflexes if approved |
| Oct 29 to Oct 30 | 2 | T5.2 push enable per service cutover, T6.1 |
| Oct 31 | 1 | T6.1 final read-only self-eval for launch |
With 250 agents and auto-accept, the whole sector peaks at about 40 useful agents (S2 fan-out + S3 + the S1 tail at once, Oct 21-24; S1+S2+S3 sum to 43 useful but T2.2 cannot start before T2.1). **No nerve task can use a 50-agent group.** The largest is T2.2 at 18 useful. Wall-clock is then bound by the merge chain (T1.1 -> T2.1 -> T2.5) and by the owner gates in S5/S6, not by agent count.

## 5. Totals
| session | tasks | target agents | useful max |
|---|---|---|---|
| S1 | 4 | 9 | 8 |
| S2 | 5 | 24 | 22 |
| S3 | 6 | 13 | 13 |
| S4 | 2 | 5 | 5 |
| S5 | 2 | 3 | 3 |
| S6 | 1 | 1 | 1 |
| **total** | **20** | **55** | **52** |

## 6. Top risks
1. **Live reflexes on a stricter verdict.** The live nerve runs as root with `NERVE_DRY_RUN=0` and has restart and rollback reflexes. The new build sees cloudflared as failing-soon (194 errors/5 min) and opens 3 error incidents. A cutover without `DRY=1` could restart cloudflared (1/h budget) or roll back a service on a fault that is really DNS/ingress. Mitigation: T3.2, and T5.1 starts in dry-run.
2. **Provenance.** The live galion-nerve (ws/nerve a63b730) and galion-health (ws/foundation e43d7ab, not ws/nerve) are not in main, and their builds are not yet proven reproducible. The rollback target exists only as the VPS binary. Mitigation: T1.2, T1.3.
3. **Wiring sits on the hot path and is unfinished.** Slot 30's tree does not compile right now (api.rs:63 splice), and `observe_request` runs on every hermes/memory request. Live hermes/memory are built from rust-v1/switch-gaps, so push cannot go live until the hermes/memory cutovers (other sectors, owner) happen. Mitigation: the T1.1 latency test with a dead nerve; T4.2/T5.2 are sequenced after those cutovers.
4. **Protocol fork and merge contention.** There are 3 event formats (`/signal`, `/event`+JSONL, boot stdout). About 20 parallel wiring agents without T2.1 first would fork the contract, and all of them collide on Cargo.lock and config.rs. Mitigation: T2.1 first, one integrator (T2.5), config.rs owned by T2.3 only.
5. **The score stays D after cutover until Slavik acts, and the clock is tight.** The FAIL causes are owner-gated (13 dead tunnel hostnames, mouth 503, stray listeners, Vite console). The cutover window crosses the Oct 25 DST change, and the request limit (11 of 15 lost tonight) can kill sector agents mid-merge. Mitigation: T3.4, no cutover that night, small batches of 5.
