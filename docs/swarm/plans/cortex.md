<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Sector plan: `cortex` (PROJECT 42 / Gaia)

- Written: 2026-10-20 20:40 CEST (+02:00), planning only (no code, no builds, no VPS access used).
- Brief: /workspace/swarm/plans/PLANNER-BRIEF.md. Rules: /workspace/swarm/RULES.md, /workspace/swarm/WAVE2.md.
- Launch deadline: 2026-10-31 (docs/main.md §9). DST: 2026-10-25 03:00 CEST -> CET (+01:00) (RULES.md 7).

## 1. Sector scope

docs/main.md §3 (integration-1 8fd26f6, lines 17-24): "Build a whole cortex. Everything already connected to the nervous
system, or not yet connected to it, must be created or connected, so that the system knows in real time: about
vulnerabilities, where it is 'bleeding', about DDoS attacks, how it can behave in real time to change the environment".

In scope here: the `cortex` module of `openmind-nerve` and everything that must feed it (sensors, pushes from services,
Gaia/heal events), the reasoning on top (correlation, score, Gaia per-function rollup), and the "behave in real time"
half as *proposals* routed through Gaia/HADES approval tokens. Out of scope (other sectors): the nerve transport itself,
Gaia core, hades-gate internals, service cutovers, Cloudflare/DNS changes.

## 2. Current state (facts, with SHAs)

| item | where | state |
|---|---|---|
| cortex module | `crates/openmind-nerve/src/cortex.rs` (585 lines) on `swarm/nerve` **5802d0e** (= `swarm/nerve-wiring` head) | vulnerabilities, bleeding, DDoS; 1 unit test `vps_like_picture` |
| full nerve crate | 23 files, 8868 lines (engine.rs 1044, probe.rs 683, state.rs 702, reflex.rs 649, access.rs 614) | std + serde_json + zagreb-time (README.md:6-8) |
| in integration | `swarm/integration-1` **8fd26f6** (local, not pushed per CALL-RECORD.md:9) | nerve taken over gaia-live (integration/integration-1.md:25,68-73); 3 one-line diffs vs 5802d0e (logscan.rs, replay.rs); 950 workspace tests pass |
| real tests | docs/real-tests/nerve.md | replay of live capture: **FAIL 61/100 D** (§2); box run: kill/restart, scanner, 6000-GET flood detected at 100.5 req/s x201, RSS 4.7 MiB, ~25 ms/cycle (§3); gated `real_box` 1 passed (§4); x86_64/aarch64 musl + android/armv7 check OK (§5) |
| live | VPS galion-nerve :8670 = old Oct 18 build from `ws/nerve` a63b730 (CALL-RECORD.md:37, LIVE.md:49,64) | says **PASS** while 59% of tunnel requests fail (nerve.md:36,47) |
| design | docs/cortex.md (B41, 2026-10-19 16:12 CEST) | auth.log + memory-gateway socket-rate sources and thresholds; **not implemented** in 5802d0e (no `auth.log`/`sshd` match in `crates/openmind-nerve/src`) |
| event contract | B33 `NerveEvent` in `crates/openmind-schema/src/lib.rs:34` (present in integration-1); B40 JSONL writer = `openmind-nerve/src/writer.rs` | STATUS.md:61-62 "D (not wired)"; NERVE-ALIGN boot JSON not wired (docs/nerve-align.md:12-14,95) |
| push path | `crates/nerve-client` (210 lines, `NERVE_URL`) | **no crate depends on it** in 5802d0e; `wt/nerve-wiring` has 11 uncommitted files wiring hermes-rs (spend_cap) + openmind-memory + hub.rs (474+/48-) |
| Gaia link | `gaia-live` 4fbbb7f / gaia-hades-tokens f3f41a2 POST `/event` (gaia.heal.*, service.probe) | events are **archived to JSONL only**; engine.rs:253-279 builds cortex from probes/logs/listeners/DDoS, not from `/event` |
| reflexes | reflex.rs:28-58 allowlist 10 units, deny ssh/fail2ban/ufw/nftables/galion-control/nerve | off by default (`NERVE_REFLEX`), observe-only in tests |
| auth on API | hub.rs / server.rs | **no authentication** on `POST /api/v1/signal` or `POST /event` (no auth/token code in either file); unit runs `User=root` (deploy/nerve/galion-nerve.service:32) |

What the cortex sees today (config.rs:92-165 `default_specs`, 14 components): openmind-memory (APOLLO), hermes-rs (ARTEMIS),
link-watch-rs (APOLLO), galion-desk (HEPHAESTUS), tuner (DEMETER), galion-one + mouth 8002 (ELEUTHIA/AETHER), console
(HEPHAESTUS), galion-health (DEMETER), cloudflared + tuner-quick (AETHER), voice-worker (absent), gaia CLI (absent),
lxc-host (POSEIDON), the nerve itself. Inputs: HTTP health, `systemctl show`, /proc RSS, journald JSON, cloudflared metrics,
df/meminfo, /proc/net/tcp listeners, tailed access logs (README.md:19-45).

### Gap: today's module vs the "whole cortex"

| main.md §3 need | have (5802d0e) | missing |
|---|---|---|
| vulnerabilities | public/unmanaged listeners, scanner paths, Vite dev server (cortex.rs:162-278) | sshd/auth.log signals (B41 §4.1); sshd -T / ufw / fail2ban posture; dependency advisories (RustSec) for deployed binaries; source-vs-live binary drift (REVIEW.md:100); secrets-permission exposure (main.md §15 "world-readable secret copies"); unauthenticated nerve POST surface; stray process owner/deleted binary; hades-gate denials, sandbox escape attempts |
| bleeding | dead tunnel origins, stuck loading, restarts, memory trend, error rate, host pressure (cortex.rs:280-448) | per-port connection rate (B41 §3.2); auth.log disk bleed (B41 §4.3); spend/compute bleed (hermes cap overrun 1.20 vs 1.00 USD, CALL-RECAP §3.10; openmind-queue caps); heal refused/capped from Gaia; public DNS/TLS failures (tuner.galion.app NXDOMAIN, REVIEW.md:100) |
| DDoS | per-stream median/MAD detector: access logs, cloudflared counter, nerve-api (README.md:33-37) | ssh credential flood (B41 §4.2: 54,605 failed passwords in 7 days, cortex.md:193); per-hostname edge rate; Cloudflare-side analytics (needs token, owner) |
| "everything connected or not yet" | 14 specs, 0 pushing services | 52 Gaia systems (PLAN-DIGEST.md:97-109) mostly unmapped; tanga-proto 8671 not in specs (LIVE.md:41); edge/LAN/phone nodes; boot hardware events |
| "behave in real time to change the environment" | reflex restarts of allowlisted units only | finding -> Gaia directive -> HADES plan -> approval token -> apply path -> nerve record; every edge/DNS/firewall fix stays a plan (cortex.rs:18-20) |
| real time, live | replay + box only | nothing live; live nerve is the old build |

### Relation to Gaia functions

The nerve is an AETHER system (gaia-ownership.md:37,82; config.rs:10-11), but the cortex *judges* for several functions.
Proposed reading (for T1.3 and T3.4; ownership decisions stay with Slavik):

| cortex output | Gaia consumer | why (design table, PLAN-DIGEST.md:100-108) |
|---|---|---|
| vulnerabilities | **MINERVA** (security audit, guardian-watchdog, alerting) | gaia-minerva already runs repo-check rules (crates/gaia-minerva/src/lib.rs:4-16) |
| bleeding (services, host, restarts) | **DEMETER** (survival, healer, guardian-tick) | survival classifies 85/95% and only suggests (cortex.md:22) |
| bleeding (spend, compute, queues) | **POSEIDON** (quotas, budget-caps, queues) | openmind-queue 7d0c478 / gaia-poseidon 498e3d2 |
| DDoS, edge, tunnels | **AETHER** (tunnels, origin, nerve) | cloudflared, edge group |
| proposed fixes | **HADES** (plans only, approval token) | hades-gate ccaab42, gaia-hades-tokens f3f41a2 |
| agent misbehaviour | **ARTEMIS** (hermes-agents) | hermes-tools escape tests (hermes-tools.md:104) |
| knowledge/history of findings | **APOLLO** (session-history, stack-schema) | JSONL archive, NerveEvent schema |
| overall verdict | **GAIA core** (orchestration only) | consumes per-function rollup (engine.rs:386) |

Known mismatches to settle: nerve maps galion-health to DEMETER/guardian-tick (config.rs:108) while gaia-land maps the unit to
MINERVA/guardian-watchdog (gaia-ownership.md:100); open nine-function list question (ARES vs POSEIDON/AETHER,
CALL-RECORD.md:39); POSEIDON vs HEPHAESTUS compute ownership (CALL-RECORD.md:16).

## 3. Design rules for this sector (to keep parallel work mergeable)

- Every new sensor is its own file `crates/openmind-nerve/src/sensors/<name>.rs` behind one `Sensor` trait, reads its own
  env config in that file, and registers with one line. Nobody else edits `engine.rs`, `cortex.rs`, `config.rs` except the
  T2.0 owner and the S3 integrator. This is what makes 16+ sensor agents possible at once.
- Every sensor ships a fixture captured read-only (or synthesised), a unit test, a replay case, and a real-life box test on its
  own slot port range.
- Read-only always; "change the environment" = HADES plans only. No new listeners, no Python, Zagreb timestamps, counts not
  raw lines/addresses in `data` (cortex.md:38).

## 4. Sessions

Agent counts: **target** = what the owner's ~50-group model would assign; **useful max** = how many can really work in
parallel without colliding. Most code tasks top out at 1-2.

### S1. Inventory and contract (Oct 21 morning)
Goal: one machine-readable list of everything that is or should be connected, and the sensor contract.

| # | task | target | useful | parallel units | inputs / deps |
|---|---|---|---|---|---|
| T1.1 | Source inventory: for each of the 52 Gaia systems and each live unit/listener, "connected? how (pull/push/none)? signal kinds? owner function?" -> `docs/cortex/sources.md` + `NERVE_COMPONENTS` JSON fixture | 9 | 9 | one agent per Gaia function (read-only doc work) | gaia-land ownership.md, LIVE.md, ss captures in /workspace/swarm/nerve-capture |
| T1.2 | `Sensor` trait + `sensors/` module + cortex Input extension; align `Finding` with B33 `NerveEvent` and B41 kinds/severities | 2 | 1 | none (one owner; 2nd agent reviews) | integration-1 8fd26f6 as base; **blocks all of S2** |
| T1.3 | Gaia mapping reconciliation (config.rs `gaia`/`gaia_system` vs gaia-core `UNITS`), add tanga-proto, stray ports to NERVE_KNOWN_PORTS policy | 1 | 1 | - | gaia-land c15a4b7 (not pushed, 24 dirty) |
| T1.4 | Fresh read-only VPS captures (3 at different hours, incl. ssh-flood hour) with `scripts/real/nerve-capture-vps.sh`, stdout only (no /tmp) | 1 | 1 | serialised to limit VPS load (load 6.3, CALL-RECORD.md:10) | ssh read-only per brief |

Success: sources.md lists all 52 systems + 13 units + 5 stray ports with a status; T1.2 compiles with zero behaviour change
(replay of replay1 still 61/D, `real_box` passes); captures stored under /workspace/swarm/nerve-capture/ with Zagreb stamps.
Approval: none (read-only).

### S2. Sensors: connect what is not connected (Oct 21 pm - Oct 23)
Goal: every main.md §3 gap in §2 has a sensor. Each task = one sensor file + fixture + test + box real test.

| # | sensor | target | useful | notes / deps |
|---|---|---|---|---|
| T2.1 | sshd auth (journald ssh unit or /var/log/auth.log; B41 §4.1/4.2/4.3 thresholds verbatim) | 2 | 1 | fixture from T1.4; never stores addresses/names |
| T2.2 | host security posture: `sshd -T`, `ufw status`, `fail2ban-client status sshd` (read-only) | 2 | 1 | needs root (nerve is root) |
| T2.3 | secrets-exposure posture: **stat mode bits only**, never open | 1 | 1 | **Slavik approval** (RULES.md 2 forbids reading under secrets dirs) |
| T2.4 | dependency advisories: RustSec DB (offline snapshot) vs Cargo.lock of the deployed SHA | 2 | 2 | units: DB loader / lock matcher; licence check of any crate used |
| T2.5 | source-vs-live drift: unit binary sha/mtime/--version vs release manifest | 2 | 1 | artifacts/tuner/SHA256SUMS pattern; launch-gates |
| T2.6 | tunnel ingress lint as a sensor (config -> closed ports, before traffic fails) | 1 | 1 | after `swarm/tunnel-audit` (crates/tunnel-lint, uncommitted) |
| T2.7 | per-port loopback connection rate (B41 §3.2, ESTAB+TIME-WAIT delta) | 1 | 1 | /proc/net/tcp, no bind |
| T2.8 | stray process owner: listener inode -> pid/exe, "(deleted)" binary | 1 | 1 | REVIEW.md:100 "one on a deleted binary" |
| T2.9 | public surface: DNS + TLS expiry + HTTP status per tunnel hostname | 2 | 1 | 110-line host list in evidence/tunnel-audit/hosts-owners.txt; GET only |
| T2.10 | Gaia/event feed: `/event` JSONL (gaia.heal.*, service.probe, survival lines, boot hardware) -> signals | 2 | 1 | gaia-live 4fbbb7f, survival-heal 45a18aa, nerve-align gaps |
| T2.11 | POSEIDON budget/queue bleed: openmind-queue caps + hermes `spend_cap` events | 1 | 1 | poseidon 7d0c478, after nerve-wiring lands |
| T2.12 | per-hostname edge request rate as DDoS streams (cloudflared journal `dest=`) | 2 | 1 | access.rs detector reuse |
| T2.13 | agent/approval misbehaviour: hades-gate denials, sandbox escape attempts | 1 | 1 | hades-approval ccaab42, hermes-tools 8ccdfcc |
| T2.14 | remote node federation: omnode/phone/LAN nodes push signed signals | 2 | 1 | edge-boot 753676e pairing; likely post-launch |
| T2.15 | push wiring: nerve-client in each service (hermes-rs, openmind-memory, galion-one, galion-desk, tuner, room, link-watch, galion-origin) | 8 | 6 | one agent per service; **after nerve-wiring commits** (it owns nerve-client + hub.rs now) |
| T2.16 | Cloudflare analytics/WAF events (read-only API) | 1 | 1 | design only until **Slavik approves a read-only token** (no new accounts, RULES.md 5) |
| T2.0 | (gate) sensor registration/merge owner for S2 | 1 | 1 | reviews each sensor PR-branch, keeps engine.rs edits |

Success (each sensor): fixture + unit test; replay case shows the expected finding; `scripts/real/cortex-<sensor>.sh` on the
box in its slot range shows the finding appear and clear; no new listener; RSS stays < 8 MiB and cycle < 50 ms with all
sensors on. Approvals: T2.3, T2.16, any VPS-side probe that needs a new command class (T2.2 commands should be listed for Slavik).

### S3. Cortex reasoning (Oct 23 - Oct 24)

| # | task | target | useful | units / deps |
|---|---|---|---|---|
| T3.1 | threshold calibration from real 300 s windows (B41 says tighten, never loosen in code) | 3 | 2 | per capture set; needs T1.4 + T2.1 |
| T3.2 | cross-sensor correlation causes (ssh flood + auth.log disk bleed = one incident; dead origin + DNS + ingress lint = one `edge`) | 2 | 1 | incident.rs owner only |
| T3.3 | score v2: deductions for new kinds, no double-count (`cause` field), pass rule | 1 | 1 | score.rs |
| T3.4 | Gaia per-function rollup (table §2) + `/api/v1/cortex` v2 by function; feed gaia-minerva/demeter reports | 2 | 1 | after Slavik settles mapping mismatches |
| T3.5 | real-time out: push alert path (journald exists; Telegram `telegram_env` in config.rs:185) | 2 | 1 | external push = **Slavik approval** |

Success: replay of all captures gives stable verdicts (no flapping over 20 cycles); live-capture replay still flags the
13-host/10-port edge incident and the 8002 stuck-loading; a box ssh-flood fixture scores crit; DST fold test across
2026-10-25 03:00 keeps `+02:00`/`+01:00` correct.

### S4. "Behave in real time": proposals through Gaia/HADES (Oct 24 - Oct 25)

| # | task | target | useful | units / deps |
|---|---|---|---|---|
| T4.1 | finding -> Gaia directive mapping (who owns the fix, verb, scope) | 2 | 1 | gaia-core (gaia-land), T3.4 |
| T4.2 | HADES plan templates per fix family: unit restart, stray process stop, ingress rule removal, ban/rate-limit at edge, rebind to loopback | 3 | 2 | per family; plans only, never executed by the nerve |
| T4.3 | reflex extension, box only, still allowlisted and budgeted (e.g. restart on stuck-loading after grace) | 1 | 1 | reflex.rs |
| T4.4 | end-to-end real test: cortex finding -> gaia -> hades-gate token -> heal-apply on dummy -> nerve record | 2 | 1 | gaia-live + gaia-hades-tokens path (gaia-live.md:15-27) |

Success: on the box, a killed dummy and a stray listener each produce a signed single-use plan; an unsigned/replayed token is
refused and recorded; no action touches ssh/firewall/DNS. Approval: any apply on the VPS, any new allowlist entry.

### S5. Verification at scale: the real ~50-agent group (Oct 25 - Oct 27)

| # | task | target | useful | parallel units |
|---|---|---|---|---|
| T5.1 | chaos/real-life scenarios on the box, one per agent: unit kill, restart loop, stuck loading, dead origin, scanner burst, nerve-api flood, distributed flood, ssh brute-force fixture, disk fill, memory leak, stray listener, deleted binary, spend-cap hit, heal refused, forged `/event`, DST fold, clock skew, journald off, systemd off, cloudflared metrics gone, ... | 25 | 20 | per scenario; limited by slot port ranges (10 ports each) and box CPU/disk |
| T5.2 | fuzz targets for every parser: logscan, access CLF, access JSON, ss/proc-net-tcp, journald JSON, hub signal, `/event`, cloudflared metrics, auth.log, sshd -T | 10 | 8 | per fuzz target; uses fuzz-sast harness (wt/fuzz-sast/fuzz, uncommitted) |
| T5.3 | replay regression corpus: every capture + expected verdict, CI job | 4 | 3 | per capture |
| T5.4 | resource and cross-target budget (musl x86_64/aarch64, android/armv7 check, RSS/CPU) | 2 | 1 | - |
| T5.5 | adversarial code review per module (cortex, engine, hub, server, reflex, access, probe, sensors) | 9 | 5 | per module, read-only |

Success: every scenario has a script + Zagreb-stamped record in `docs/real-tests/cortex-*.md`; fuzzers run 10 min each with 0
crashes; corpus green in CI; forged-signal scenario documented (see risk 3).

### S6. Live readiness (Oct 28 - Oct 31, owner-gated)

| # | task | target | useful | notes |
|---|---|---|---|---|
| T6.1 | shadow by replay: periodic read-only captures replayed on the box, diff vs live 8670 verdict | 1 | 1 | cadence needs **Slavik approval** (VPS load) |
| T6.2 | galion-nerve cutover runbook + deploy-sim rehearsal (old a63b730 -> cortex build), rollback | 2 | 1 | deploy-sim (slot 33) already rehearses nerve cutover (CALL-RECORD.md:28) |
| T6.3 | docs: main.md §3 status, STATUS.md B33/B40/B41 rows, launch gate entry | 1 | 1 | docs-sync conventions |
| T6.4 | integration branch `swarm/integration-cortex` merging S2-S5 onto integration-1 | 2 | 1 | single merger |

Success: cutover rehearsal passes on the box; Slavik decides the live switch. Approval: **cutover of galion-nerve 8670**,
`NERVE_URL` in live service envs, any live reflex.

## 5. Merge order and dependencies

1. integration-1 8fd26f6 (owner merge) -> base for this sector. gaia-land c15a4b7 needed for T1.3/T3.4/T4.1.
2. nerve-wiring (uncommitted hermes-rs/memory/nerve-client/hub.rs) lands before T2.11, T2.15.
3. T1.2 sensor trait -> all S2 sensors (any order, one registration line each) -> T3.x (single owners of incident.rs/score.rs) -> S4 -> S5 -> T6.4 -> T6.2.
4. tunnel-audit before T2.6; fuzz-sast harness before T5.2; poseidon/gaia-poseidon before T2.11.

## 6. Realistic schedule at 16 concurrent agents

Launch at most ~4 agents per minute (a burst of 15 lost 11 to the request limit, CALL-RECORD.md:7).

| wave | when (CEST/CET) | slots | what |
|---|---|---|---|
| W1 | Oct 21 08:00-13:00 CEST | 16 | T1.1 x9, T1.2 x1, T1.3 x1, T1.4 x1, T5.2 x4 (fuzz existing parsers: logscan, access, ss, hub) |
| W2 | Oct 21 13:00 - Oct 22 CEST | 16 | T2.1, T2.2, T2.4 x2, T2.5, T2.6, T2.7, T2.8, T2.9, T2.10, T2.11, T2.12, T2.13, T2.15 x2, T2.0 merger |
| W3 | Oct 23 CEST | 16 | T2.15 x4, T2.14, T2.3/T2.16 (if approved), T3.1 x2, T3.2, T3.3, T3.4, T3.5, T6.4 merger, T5.1 x2 |
| W4 | Oct 24-25 (DST at 03:00 Oct 25) | 16 | T4.1, T4.2 x2, T4.3, T4.4, T5.1 x10, merger |
| W5 | Oct 26-27 CET | 16 | T5.1 x8, T5.2 x4, T5.3 x3, T5.4 |
| W6 | Oct 28-29 CET | 10 | T5.5 x5, T6.1, T6.2, T6.3, T6.4, fix-ups |
| buffer | Oct 30-31 CET | <=4 | owner decisions, cutover if approved |

About 90 agent-runs over 6 waves. If the cortex sector gets only 4 of the 16 slots (shared with other sectors), keep the
order but cut to: S1 (T1.2, T1.4, T1.1 x2 covering all functions), S2 top six sensors (T2.1, T2.7, T2.10, T2.8, T2.9, T2.15
for hermes-rs + memory), T3.1-T3.3, T4.4, T5.1 x8 scenarios, T6.2/T6.4; defer T2.3, T2.14, T2.16, T3.5, T5.5 past Oct 31.

## 7. Totals

| session | tasks | target agents | useful agents |
|---|---:|---:|---:|
| S1 inventory + contract | 4 | 13 | 12 |
| S2 sensors | 17 | 32 | 23 |
| S3 reasoning | 5 | 10 | 6 |
| S4 proposals via Gaia/HADES | 4 | 8 | 5 |
| S5 verification (~50 group) | 5 | 50 | 37 |
| S6 live readiness | 4 | 6 | 4 |
| **total** | **39** | **119** | **87** |

Only S5 can genuinely use a ~50-agent group (per-scenario / per-fuzz-target units). S2 can use ~20 thanks to the
one-file-per-sensor rule; S3/S4 are 1-2 per task because they edit shared files.

## 8. Needs Slavik

1. Cutover of live galion-nerve 8670 (old a63b730) to the cortex build; `NERVE_URL` in live service envs; any live reflex.
2. T2.3 stat-only check of secrets paths (touches RULES.md 2).
3. T2.16 read-only Cloudflare analytics token; T3.5 external push alerts (Telegram).
4. Gaia mapping: galion-health DEMETER vs MINERVA; cortex outputs per function (§2 table); nine-function list; POSEIDON vs HEPHAESTUS.
5. Capture cadence for T6.1 (VPS read-only load).
6. Authentication for nerve `POST /api/v1/signal` and `/event` (local forgery risk) before services push.
7. The fixes the cortex already names (edge ingress/DNS for 13 hosts/10 ports, mouth 8002 restart, Vite dev console, stray listeners): nerve.md:80,89.

## 9. Top 5 risks

1. **Nothing is live.** The cortex runs only in replay and on the box; live 8670 is the old build that reports PASS while
   59% of tunnel requests fail. No "real-time knowledge" by Oct 31 without an owner-approved cutover.
2. **Merge contention.** engine.rs (1044 lines), cortex.rs and config.rs are shared hot spots; nerve-wiring has 11 uncommitted
   files in nerve-client/hub.rs; integration-1 and gaia-land are not merged. Without the T1.2 sensor trait first, parallel
   sensors will conflict.
3. **The cortex is itself attackable.** It runs as root (galion-nerve.service:32) and accepts unauthenticated local POSTs;
   adding root-level sensors (sshd -T, fail2ban, /proc fds) and pushes from every service widens that surface. A forged
   signal could fake or hide an incident.
4. **Calibration and flapping.** Thresholds come from 39 hourly totals and one 35-minute capture (cortex.md:157-167,
   nerve.md:19); the DDoS baseline needs a 30 min warm-up; DST on Oct 25; one false "memory rising" was already found.
   A flapping score would undermine the self-eval gate.
5. **Swarm capacity and box resources.** ~50-agent groups trip the request limit (11 of 15 lost); disk hit 92-95% during
   the nerve run (nerve.md:229-231; 47% now); 10-port slot ranges cap concurrent real tests; Gaia ownership questions
   (§8.4) block S3.4/S4.1.
