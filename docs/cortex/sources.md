<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Cortex source inventory (plan cortex.md T1.1)

- Written: 2026-10-20 23:05 CEST (+02:00), branch `swarm/cortex-sources` (slot 44), base `swarm/integration-1` 8fd26f6.
- Registry: `gaia_core::system::System::ALL` (52 systems) and `gaia_core::ownership::UNITS` from `swarm/gaia-land` 9d3ec18
  (`crates/gaia-core/src/system.rs:144-206`, `ownership.rs:177-194`).
- Nerve code read: `crates/openmind-nerve` at 8fd26f6 (`config.rs:92-165` built-in table, `collect.rs:103-191`,
  `hub.rs`, `server.rs:207-300`, `cortex.rs:183`), `gaia-live/src/nerve.rs`, `guardian-tick/src/heal.rs:168-181`,
  `openmind-survival/src/nerve.rs`, `swarm/nerve-wiring` 8d7a924.
- Live evidence: three batched READ-ONLY ssh sessions to the VPS, 2026-10-20 22:52:48 - 22:53:19 CEST
  (`systemctl list-units`, `ss -ltnp`/`-lunp`, HTTP GET only). Raw output kept on the box in
  `/workspace/swarm/target/cortex-sources/captures/` (no secrets read; unit Environment not read).
- Fixture: `crates/openmind-nerve/tests/fixtures/nerve-components-vps.json` (proposed `NERVE_COMPONENTS`, 18 rows + host + self).
  Loaded by the integration-1 nerve on the box: see `docs/real-tests/cortex-sources.md`.

## Legend

- **connected** = does a real signal from it reach the nerve's cortex *today*:
  `yes` (live 8670 receives it), `partial` (a code path exists but nothing reaches the cortex live: not deployed,
  file sink only, archive only, or only a label/spec), `no`.
- **how**: `pull` (nerve probes it: HTTP GET health, `systemctl show`, /proc RSS/CPU, journald JSON, cloudflared
  metrics, /proc/net/tcp listener scan, df/meminfo), `push` (service POSTs `/api/v1/signal` or `/event`),
  `file` (writes nerve-format JSONL nobody ingests), `self`, `none`.
- **runtime**: is there anything running on the VPS for it (unit/process), `lib` (code only), `design` (no code).
- Live nerve = galion-nerve 8670, old build (`ws/nerve` a63b730), started 2026-10-18 22:35:04 CEST, 15 components,
  all `pull` except itself (`connection` field of `/state`). Its `/signals` showed `{"rejected":0,"sources":[]}`:
  **zero services push**.

## Counts

| | yes | partial | no | total |
|---|---:|---:|---:|---:|
| Gaia systems | **13** | 9 | **30** | 52 |
| running stack units (gaia-core UNITS, non-legacy) | 11 | 0 | 2 (tanga-proto, galion-backup) | 13 |
| other running/host units (ssh, fail2ban, nftables, ufw, rsyslog, postfix, cron, journald, resolved, networkd, sysstat, user@0) | 0 | 0 | 12 | 12 |
| TCP listeners (`ss -ltn`, 21 sockets on 19 ports + 22) | 11 health-probed | 4 known-ignored (25 x2, 53 x2) + 8671 known-ignored + 22 public-ok | **5 strays** (8431, 8432, 8433, 8652, 8653) | 21 |

Per function (yes / partial / no): HADES 0/2/3, MINERVA 1/1/4, APOLLO 2/1/2, HEPHAESTUS 2/1/5, ARTEMIS 1/0/5,
POSEIDON 1/0/3, AETHER 4/1/4, DEMETER 1/3/2, ELEUTHIA 1/0/2.

## 1. The 52 Gaia systems

| # | function | system | component (registry) | runtime on VPS | connected | how | signal kinds today | missing / proposed |
|---:|---|---|---|---|---|---|---|---|
| 1 | HADES | teardown | hades-gate | lib (not deployed) | no | none | - | plan created/approved/denied, token refusal (push `/event`) |
| 2 | HADES | rollback | deploy/rollback*.sh | scripts | partial | self | nerve reflex may run rollback (`NERVE_ROLLBACK`), recorded in nerve `/audit` only | rollback outcome as an event; source-vs-live binary drift |
| 3 | HADES | kill-switch | design | design | no | none | - | - |
| 4 | HADES | heal-apply | galion-healer / gaia-live heal | lib (gaia CLI absent on VPS) | partial | push (code) | `gaia-live/src/nerve.rs` POSTs `gaia.heal.*`, `service.probe` to `/event`; **archived to events.jsonl only, not read by the cortex** (engine.rs:253) | feed `/event` into cortex input; auth on `/event` |
| 5 | HADES | secret-rotation-plan | design | design | no | none | - | secrets-permission stat (T2.3, owner approval) |
| 6 | MINERVA | repo-check | repo-check | lib (CI/box) | no | none | - | self-eval result per deployed SHA |
| 7 | MINERVA | secret-scan | repo-check | lib | no | none | - | - |
| 8 | MINERVA | guardian-watchdog | galion-health (8660), one-defence | **galion-health.service** | yes | pull | GET /status.json, systemd, journald, cross-check of its 12-service verdict (`NERVE_GALION_HEALTH_PORT`) | nerve labels it DEMETER/guardian-tick (mismatch, §4) |
| 9 | MINERVA | alerting | guardian health-tick alerting | none | partial | self | nerve `alerts.rs` journald alerts only; live `/state`: "external: none configured" | external channel (T3.5, owner approval) |
| 10 | MINERVA | vision | design | design | no | none | - | - |
| 11 | MINERVA | sandbox-policy-review | openmind-sandbox policy | lib | no | none | - | sandbox denials/escape attempts |
| 12 | APOLLO | memory-gateway | openmind-memory (8420) | **openmind-memory.service** (+3 stray copies) | yes | pull | HTTP health, systemd, RSS/CPU, journald; push (heartbeat/5xx/slow/conn-cap) only on `swarm/nerve-wiring` 8d7a924 | socket-rate (B41 §3.2); strays 8431-8433 |
| 13 | APOLLO | link-watch | link-watch-rs | **link-watch-rs.service** | yes | pull | systemd, RSS/CPU, journald (no port) | liveness heartbeat (no port: unit state is the only evidence) |
| 14 | APOLLO | teacher-docs | TEACHER/, docs/ | lib (files) | no | none | - | - |
| 15 | APOLLO | session-history | desk notes | none | no | none | - | - |
| 16 | APOLLO | stack-schema | openmind-schema, openmind-mcp-stack | lib | partial | contract | defines B33 `NerveEvent` (schema/src/lib.rs:34); not a source itself | - |
| 17 | HEPHAESTUS | terminal | openmind-sandbox | lib | no | none | - | command plans run / refused |
| 18 | HEPHAESTUS | cargo-build | workspace builds | box only | no | none | - | - |
| 19 | HEPHAESTUS | release-artifacts | release/, dist/bin | files | no | none | - | deployed-binary SHA vs source (drift) |
| 20 | HEPHAESTUS | galion-desk | galion-desk (8646) | **galion-desk.service** | yes | pull | HTTP health, systemd, RSS/CPU, journald | - |
| 21 | HEPHAESTUS | omnode-sandbox-runner | planned | design | no | none | - | - |
| 22 | HEPHAESTUS | crafting | gaia/ | design | no | none | - | - |
| 23 | HEPHAESTUS | console | console (8080) | **console.service** (node) | yes | pull | GET / (302), systemd, RSS/CPU, journald; integration-1 cortex flags the Vite dev server | - |
| 24 | HEPHAESTUS | deploy-switch | deploy/switch*.sh | scripts | partial | pull (process scan) | `reflex::deploy_in_progress()` pauses reflexes while switch.sh/galion-deploy runs; no event | switch start/end as an event |
| 25 | ARTEMIS | hermes-agents | hermes-rs (8642) | **hermes-rs.service** (+2 stray gateways) | yes | pull | HTTP health, systemd, RSS/CPU, journald; push (heartbeat/5xx/spend-cap) only on nerve-wiring 8d7a924 | spend-cap overrun; strays 8652/8653 |
| 26 | ARTEMIS | boss-node | Hermes boss node | none | no | none | - | - |
| 27 | ARTEMIS | room | room | box only | no | none | - | - |
| 28 | ARTEMIS | agent-spawning | Gaia/Hermes | none | no | none | - | spawn count/rate |
| 29 | ARTEMIS | agent-retirement | Gaia/Hermes | none | no | none | - | - |
| 30 | ARTEMIS | ecosystem-census | galleon-arena | lib | no | none | - | - |
| 31 | POSEIDON | vram-pool | openmind-provider/exo/llama/tulu | lib (no GPU live) | no | none | - | provider reachability, spend (no paid calls) |
| 32 | POSEIDON | quotas | cgroup-detect, sandbox rlimits | host | yes | pull | `lxc-host`: df /, meminfo (built-in Host kind) | cgroup limits (cgroup-detect not wired), CPU/load |
| 33 | POSEIDON | queues | openmind-queue | lib | no | none | - | queue depth / drops |
| 34 | POSEIDON | budget-caps | hermes spend caps | inside hermes-rs | no | none | - | spend-cap hit (push exists only on nerve-wiring) |
| 35 | AETHER | messaging | hermes-gateway | **hermes-gateway process inside hermes-rs.service** | yes | pull | via the `hermes-rs` component (labelled ARTEMIS/hermes-agents) | per-channel errors |
| 36 | AETHER | nerve | openmind-nerve | **galion-nerve.service** (old a63b730) | yes | self | own cycle, API rate (nerve-api DDoS stream in integration-1) | auth on POST; cutover (owner) |
| 37 | AETHER | voice-relay | galion-voice-core, persona-plex-relay | not deployed | partial | pull (spec, absent) | `voice-worker` spec, `expected=false` -> "absent" | - |
| 38 | AETHER | speech-io | galion-one mouth (8002), openmind-speech | inside galion-one.service | yes | pull | GET 8002/health (503 at 22:52 CEST), stuck-loading timer | - |
| 39 | AETHER | wifi-identity | omnode | design | no | none | - | - |
| 40 | AETHER | lan-discovery | openmind-discovery | lib | no | none | - | - |
| 41 | AETHER | tunnels | cloudflared (20241), tuner-quick (20242) | **cloudflared.service, tuner-quick.service** | yes | pull | systemd, cloudflared /metrics counters, origin errors (live: 174/478 in 10 min), edge DDoS stream | per-hostname edge rate, DNS/TLS (tuner.galion.app NXDOMAIN) |
| 42 | AETHER | origin | galion-origin | not deployed | no | none | - (origin errors only seen through cloudflared) | - |
| 43 | AETHER | zagreb-clock | zagreb-time | lib (used by the nerve) | no | none | - | clock skew / DST fold check |
| 44 | DEMETER | survival | openmind-survival, galion-backup | **galion-backup.timer** | partial | file (code) | `survival::nerve_line` emits nerve JSONL nobody ingests; the backup timer is not a nerve component | timer last-run result (fixture row) |
| 45 | DEMETER | healer | galion-healer heal-dry, guardian-tick heal | not deployed | partial | file | `guardian-tick heal --nerve` writes `<state>/nerve.jsonl` (heal.rs:17-19, "file sink only; no socket") | ingest or push |
| 46 | DEMETER | tuner | tuner (8650) | **tuner.service** | yes | pull | HTTP health, systemd, RSS/CPU, journald | - |
| 47 | DEMETER | config-health | openmind-config | lib | no | none | - | config drift |
| 48 | DEMETER | guardian-tick | guardian-tick | not deployed | no | none (label only) | the nerve uses this label for galion-health; the crate is not running | - |
| 49 | DEMETER | boot-units | galion-boot, deploy/*.service | systemd | partial | pull | `systemctl show` of the nerve's own listed units only; failed units outside the list are invisible | boot events, all-unit failed scan |
| 50 | ELEUTHIA | persona-plex | openmind-persona | lib | no | none | - | - |
| 51 | ELEUTHIA | galion-one | galion-one (8091), tanga-proto (8671) | **galion-one.service, tanga-proto.service** | yes | pull | 8091 health, systemd, RSS/CPU, journald; tanga-proto **not** a component (8671 only in `NERVE_KNOWN_PORTS`) | tanga-proto GET /tanga/ (fixture row) |
| 52 | ELEUTHIA | gaia-seeds | gaia/ seeds | lib | no | none | - | - |

GAIA core itself: the nerve has a `gaia` spec (`gaia status`), live status **absent** (CLI not installed).

## 2. Live units (VPS, 2026-10-20 22:52:48 CEST)

| unit | state | Gaia (gaia-core UNITS) | in live nerve | how | signals | note |
|---|---|---|---|---|---|---|
| openmind-memory | active/running | APOLLO memory-gateway | yes | pull | health 200, systemd, RSS, journald | |
| hermes-rs | active/running | ARTEMIS hermes-agents | yes | pull | health 200, ... | process name `hermes-gateway` |
| link-watch-rs | active/running | APOLLO link-watch | yes | pull | systemd, RSS, journald | no port |
| galion-desk | active/running | HEPHAESTUS galion-desk | yes | pull | health 200 | |
| tuner | active/running | DEMETER tuner | yes | pull | health 200 | |
| galion-one | active/running | ELEUTHIA galion-one | yes | pull | 8091 200, mouth 8002 503 | mouth = AETHER speech-io |
| console | active/running | HEPHAESTUS console | yes | pull | GET / 302 | node / Vite |
| galion-health | active/running | MINERVA guardian-watchdog | yes (as DEMETER/guardian-tick) | pull | /status.json 200 | mismatch |
| cloudflared | active/running | AETHER tunnels | yes | pull | metrics 200 (26954 B), "degrading: origin errors 174/478 in 10 min" | |
| tuner-quick | active/running | AETHER tunnels | yes | pull | metrics 200 (17806 B) | |
| galion-nerve | active/running | AETHER nerve | self | self | live verdict `degrading` (11 healthy, 2 degrading, 2 absent); self-eval `pass: true` | User=root |
| tanga-proto | active/running | ELEUTHIA galion-one | **no** | none | (GET /tanga/ 200, /health 404) | fixture adds it |
| galion-backup (.timer) | timer active/waiting; .service inactive (oneshot) | DEMETER survival | **no** | none | - | fixture adds the timer |
| hermes, link-watch, memory-gateway | inactive/dead (legacy) | legacy rows | no | - | - | correctly ignored |
| ssh | active/running | **unmapped** | no | none | only port 22 is "public-ok" in the listener scan | B41 auth.log not implemented |
| fail2ban | active/running | unmapped | no | none | - | reflex deny list |
| nftables, ufw | active/exited | unmapped | no | none | - | |
| rsyslog, postfix(@-), cron, systemd-journald/-resolved/-networkd, networkd-dispatcher, sysstat, user@0 | active | unmapped | no | none | - | |
| run-u11093, run-u11094, run-u11095 | **failed** (transient `systemctl is-active ssh.service`) | unmapped | **no** | none | - | nobody sees failed units outside the nerve's list |

## 3. Live listeners (`ss -ltnp`, 22:52:48 CEST)

| port | bind | process (unit / scope) | Gaia | nerve today | in integration-1 cortex |
|---|---|---|---|---|---|
| 22 | `*` | sshd (ssh.service) | unmapped | public-ok (port only) | `NERVE_PUBLIC_OK_PORTS=22` |
| 25 | 127.0.0.1, ::1 | postfix master | unmapped | known-ignored | known |
| 53 | 127.0.0.53, 127.0.0.54 | systemd-resolved | unmapped | known-ignored | known |
| 8002 | 127.0.0.1 | galion-one | AETHER speech-io | health probe (503) | component |
| 8080 | 127.0.0.1 | node (console.service) | HEPHAESTUS console | health probe (302) | component + Vite finding |
| 8091 | 127.0.0.1 | galion-one | ELEUTHIA galion-one | health probe (200) | component |
| 8420 | 127.0.0.1 | openmind-memory (unit) | APOLLO memory-gateway | health probe (200) | component |
| **8431** | 127.0.0.1 | openmind-memory, pid in `user-0.slice/session-48893.scope`, up 2 d | APOLLO memory-gateway (by process name) | **unseen** | unmanaged-listener finding |
| **8432** | 127.0.0.1 | openmind-memory, `session-49009.scope`, up 2 d | APOLLO | **unseen** | unmanaged |
| **8433** | 127.0.0.1 | openmind-memory, `session-49012.scope`, up 2 d | APOLLO | **unseen** | unmanaged |
| 8642 | 127.0.0.1 | hermes-gateway (hermes-rs.service) | ARTEMIS / AETHER messaging | health probe (200) | component |
| 8646 | 127.0.0.1 | galion-desk | HEPHAESTUS | health probe (200) | component |
| 8650 | 127.0.0.1 | tuner | DEMETER | health probe (200) | component |
| **8652** | 127.0.0.1 | hermes-gateway, `session-48941.scope`, up 2 d | AETHER messaging (by process name) | **unseen** | unmanaged |
| **8653** | 127.0.0.1 | hermes-gateway, `session-49065.scope`, up 2 d | AETHER messaging | **unseen** | unmanaged |
| 8660 | 127.0.0.1 | galion-health | MINERVA guardian-watchdog | health probe (200) | component |
| 8670 | 127.0.0.1 | galion-nerve | AETHER nerve | self | self |
| 8671 | 127.0.0.1 | tanga-serve (tanga-proto.service) | ELEUTHIA galion-one | known-ignored | known (`NERVE_KNOWN_PORTS`) |
| 20241 | 127.0.0.1 | cloudflared (named) | AETHER tunnels | metrics | component |
| 20242 | 127.0.0.1 | cloudflared (quick) | AETHER tunnels | metrics | component |
| UDP 36761, 41169, 43406, 55151, 55587 | `*` | cloudflared QUIC | AETHER tunnels | not scanned (TCP only) | not scanned |

All 5 strays answer `GET /health` 200 and were started from root ssh sessions about 2026-10-18 22:15-22:35 CEST
(parent PID 1, cgroup = session scope, not a unit). Whether to stop them is an owner decision (§5); nothing was touched.

## 4. Mapping mismatches (for T1.3, owner decisions)

1. **galion-health**: gaia-core UNITS = MINERVA/guardian-watchdog; nerve built-in = DEMETER/guardian-tick. The fixture uses the registry.
2. **hermes-rs unit vs hermes-gateway process**: the unit is ARTEMIS/hermes-agents, the listening binary is AETHER/messaging. One component, two systems.
3. **galion-one mouth 8002**: nerve says AETHER/speech-io, registry puts the unit (galion-one) in ELEUTHIA. Kept as AETHER, grouped with galion-one.
4. **Host security** (ssh, fail2ban, nftables/ufw) has no Gaia system among the 52. Fixture uses `MINERVA` / `unmapped:host-security` as a proposal.
5. **guardian-tick** label used for a unit that is not guardian-tick; the crate itself is not running.

## 5. Biggest blind spots (ranked)

1. **The only public door is unwatched.** sshd (`*:22`), fail2ban and the firewall send nothing; the B41 auth.log sensor
   (54,605 failed passwords in 7 days, docs/cortex.md) is not implemented, and no Gaia system owns host security.
2. **Five stray daemons, invisible.** 3 openmind-memory (8431-8433) + 2 hermes-gateway (8652/8653) run outside systemd from
   root ssh sessions for 2 days. The live nerve's `/state` has no listener or cortex section; integration-1 would flag "unmanaged" but cannot
   say whose they are or that they are copies of live services (nerve-wiring 8d7a924 adds port ownership).
3. **Zero push.** No service pushes (`/signals` sources = []). Spend caps, HADES denials, Gaia heals, sandbox refusals,
   queue drops are invisible. The push code exists (nerve-client, gaia-live `/event`), but `/event` is archived only and
   never feeds the cortex, and neither POST endpoint is authenticated.
4. **Whole functions dark.** HADES 0/5 yes, POSEIDON only `lxc-host` disk/RAM (no budget, queue, compute), ARTEMIS only
   a health probe (no agent population signal). 30 of 52 systems have no signal at all; 29 of those have no VPS runtime of their own (budget-caps lives inside hermes-rs).
5. **"No evidence = healthy".** On the box (§real test) `cloudflared`, `tuner-quick`, `ssh`, `fail2ban`, `link-watch-rs`
   reported `healthy` with `unit_state: null`, no metrics and no HTTP (NERVE_SYSTEMD=0, metrics ports closed). On the VPS
   systemd is on, but a cloudflared whose metrics port dies is judged on unit state only. T1.2 should add an `unknown`
   status when a component yields no evidence.
6. **Failed units outside the list.** Three failed transient units (`run-u11093..95`) and the backup timer are not seen;
   the nerve only queries units it was told about.
7. **The live verdict lags reality.** Live 8670 (old a63b730) says `degrading` with 36% tunnel origin errors and its
   self-eval says `pass: true`; tanga-proto and the strays are not in its world at all.

## 6. Next steps this inventory unblocks

- T1.2: add `unknown` status (blind spot 5); make `/event` a cortex input (blind spot 3).
- T1.3: settle §4; load `tests/fixtures/nerve-components-vps.json` as the reference VPS table (it already parses and runs, see real test).
- T2.x sensors in priority order from §5: auth.log/sshd (1), listener ownership (2, port from nerve-wiring), failed-units scan (6), timer last-run (6).
- Owner: decide the 5 strays (stop / adopt as units / keep), host-security owner function, nerve cutover.
