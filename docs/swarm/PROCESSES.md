<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Swarm processes, 2026-10-20 (Europe/Zagreb, CEST +02:00)

Written 2026-10-20 23:31 CEST (+02:00) by swarm/swarm-record (wave 3, slot 39). **Refreshed 2026-10-21 06:41 CEST (+02:00) by swarm/processes-refresh (wave 4, slot 142)** with everything after 23:33 CEST: the end of wave 3, wave 4 (slots 100-149), the VPS deployment of 2026-10-21 and the new standing rules. Sources: `docs/swarm/record/CALL-RECORD.md` (snapshot of the live call record, refreshed 06:41 CEST), `docs/swarm/record/WAVE2.md`, `docs/swarm/plans/wave4-tasks.md`, the agents' branch heads in `/workspace/swarm/wt/*` (checked 06:36 CEST), the push-helper results quoted in the call record, and read-only probes of the VPS at 06:36 CEST (`docs/real-tests/processes-refresh.md`).

**State of `main`:** GitHub `main` is still `13a296c` (PR #8, one-source merge). Nothing from the swarm (wave 1 to wave 4) is merged into main. Every branch below is on GitHub as `swarm/<slug>` unless marked otherwise, and waits for Slavik to merge, through the integration branches (`swarm/integration-1`, `-2`, `-3`, `-4`). Slot 142 had no GitHub read access of its own (the repository is private and the token stays on the VPS); "on GitHub" below means the push helper reported the push in the call record, or a verifying ls-remote did (03:04, 04:18, 04:53 CEST).

**How to read this file:** one row per agent slot or workstream. *What* is the deliverable. *Why* is the reason it was started. *Status* is the state at 23:31 CEST for waves 1-3 (rows updated where they finished later) and at 06:41 CEST for wave 4. *Owner decisions* are open questions only Slavik can answer. Agents did not decide these themselves.

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
| fuzz-sast (25) | e78b2e7 | cargo-fuzz targets, 11-min runs 0 crashes, CodeQL/ClusterFuzzLite workflows | Security S1 | pushed 23:28-23:30; continued in wave 4 as `fuzz-sast-2` (slot 100) | libFuzzer/CodeQL licence ruling |
| hermes-enroll (32) | f2121a8 | Device enrollment: nonce store, scoped node credentials, revocation | Phones and edges join Hermes | pushed 23:28-23:30; continued as `hermes-enroll-2` (slot 102) | merge |
| nerve-wiring (30) | 8d7a924 | hermes-rs + openmind-memory push events through nerve-client | Nerve only polled before | pushed 23:28-23:30; continued as `nerve-wiring-2` (slot 101, .sh ported) | port .sh to Rust |
| tunnel-audit (35) | d35ef73 | cloudflared ingress audit + tunnel-lint (read-only) | 33 hostnames have no listener | pushed 23:28-23:30; continued as `tunnel-audit-2` (slot 103) | merge |
| planners x7 | plans/*.md | gaia, nerve, cortex, vps-ops, integration-ci, security, docs-whitepaper: 47 sessions, 200 tasks, 645 target / 407 useful agents | Plan the launch work by sector | done | items in each plan |
| non-rust inventory | plans/non-rust-inventory.md | Every non-Rust piece in the repo and on the VPS | Rust-only rule | done | OS packages, cloudflared, CI YAML, browser JS |

## Wave 3 (from 22:52 CEST)

| Slot | Slug | Head | What it did | Status | Owner decisions |
|---|---|---|---|---|---|
| 37 | integration-2 | df42e0f | Merged 4 Gaia branches on integration-1; 999 tests, clippy 0 | on GitHub; main merge held for verifiers | 5 pending ownership rows; how main is pushed |
| 38 | cutover-prep | a5ac49b | Rust galion-deploy backup/backup-verify, runbook `docs/ops/cutover-tuner-nerve.md`; found 5 VPS auto-accept settings | **done 23:46** (one request-limit stop at 23:42, resumed) | D1-D4 (cutover order, backups, auto-accept) |
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
| 55 | cortex-s2-a | 9557eb3 | sshd brute-force + RustSec sensors; found zip 7.4.0 (CVSS 9.1) via candle | **done 23:28**, on GitHub | D1-D6 |
| 56 | cortex-s2-b | 51100e1 | drift + conn-rate sensors; B41 conn-rate formula reads ~0 under steady load | **done 23:27**, on GitHub | ports/thresholds, B41 formula, drift manifest |
| 57 | cortex-s2-c | dbec5c9 | proc-owner + surface sensors; galion.app cert expires 2026-10-31 18:28 +01:00 (launch day) | **done 23:30**, on GitHub | D1-D4 |
| 58 | cortex-s2-d | f3468ce | cortex events feed + hades sensor, off by default | on GitHub | enable in prod |
| 59 | bench-hermes | 457ef74 | All 3 benchmarks through the Hermes tool layer: scripted 100/100/100, mock-LLM 100/100/100; live model path built but off | **done 23:49** (f7da261 was the 23:28 backup push) | H1-H5 (H1 = live paid model run) |

Other tracks today: the Galion OS repo was staged at `/workspace/swarm/galion-os-repo` (copied into `docs/swarm/galion-os-staging/`); it waits for the GitHub connection to create `galion-studio/galion-os`. The voice config is recorded in Q11/Q12 of the call record.

## End of wave 3 (23:33-23:49 CEST)

- 23:33 `swarm-record` done (`749ddbd`, the first version of this file). All 68 `swarm/*` heads were on GitHub at that point. VPS archive `/opt/galion/archive/swarm-20261020-233213+0200/` (root 0700): 963 files plus a 95-ref bundle, sha256 1000/0, bundle verify OK. The bundle still holds the old leaked-secret history until SEC-1..5 runs.
- 23:46 `cutover-prep` done (`a5ac49b`) and 23:49 `bench-hermes` done (`457ef74`); see the wave 3 rows above.

## Wave 4 (2026-10-21, from 00:52 CEST): slots 100-149

**Why:** Slavik raised the ceiling to 50-agent groups, max 250 (00:52 CEST), and at 04:38 asked for at least 50 agents spawned. Task list: `docs/swarm/plans/wave4-tasks.md`. Slots 100-103 resumed four wave-2 branches that had stalled for about 4 h.

**How it ran:** the request limit stopped most launches within minutes (00:55: all 20 of the first batch). A routine "Wave-4 swarm auto-restart" runs every 15 minutes, starts up to 6 agents per run and relaunches every stopped slot as the sole agent on its worktree (an agent exits as a duplicate if someone else touched the worktree in the last 3 minutes). Builds go through `bin/cargo-slot` (at most 3 cargo builds at once, waits for free RAM), `-j 2`, one target dir per slot, deleted after the slot. The 50-agents-spawned milestone was reached at 06:18 CEST (58 raw starts since 04:38, 54 without the 4 duplicates stopped at 04:41); by 06:35 the ledger counted 64 raw starts (60 without duplicates).

**Count at 06:41 CEST:** 40 of 50 done and pushed (100-139). 140-145 running (relaunched 06:34; 140-142 are docs-only and had restarted 3-4 times). 146-149 not started. Slots 150+ come from the 7 sector plans.

Tests named "workspace" below exclude `openmind-sandbox` and `cgroup-detect` from 04:29 on (rule `sandbox_vps_only`). "Real" means a `GALION_REAL=1` Rust test against the release binary on the slot's own port range, plus read-only probes of the live VPS where the slot needed them.

| Slot | Slug | Head | What it did | Evidence | Owner decisions |
|---|---|---|---|---|---|
| 100 | fuzz-sast-2 | f3a10af | Rust replay test `real_fuzz_replay.rs` replaces fuzz-sast.sh | HTTP 5666 files / 0 refused, mDNS 1429 datagrams; workspace 551/0 | S6.1 libFuzzer/CodeQL licence ruling, enable code scanning. Not done: new fuzz runs, `scripts/fuzz.sh` still bash |
| 101 | nerve-wiring-2 | d97dab9 | Rust `real_wiring.rs` replaces nerve-wiring.sh | 2 real runs; workspace 567/0 | Open: hermes p99 with nerve down +45-82% (goal 5%); live NERVE_URL off. Conflicts in 7 files with the nerve shadow branch |
| 102 | hermes-enroll-2 | c28374e | Rust `real_enroll.rs` replaces hermes-enroll.sh | real 72/0; workspace 509/0 | binary size (8.8 KB under 2 MiB), VPS enable + hostname, 404 vs 401. Integration should take `-2` |
| 103 | tunnel-audit-2 | 988e47f | Rust `real_vps_copy_with_tuner_origins` replaces the script | real run 2 passed | Apply sequence in `docs/ops/tunnel-audit.md` (33 dead routes, config mode, obsidian route, tuner DNS) |
| 104 | cortex-t20-merge | ee75f75 | cortex-s2-a/b/c/d merged onto cortex-sensor e536446; ssh.service in journal collection | workspace 1025/0; 7 real tests; replay golden 21/F unchanged | unit name ssh vs sshd to confirm |
| 105 | cortex-state-titles | 967459d | `/api/v1/state` cortex_events no longer leak bind addresses | openmind-nerve 84/0; real before/after | — |
| 106 | cortex-event-ingest | 7a55e6c | event-feed sensor reads the POST /event log back as findings (off by default) | workspace 975/0; real with a real gaia heal-tick | `/event` and `/signal` are unauthenticated (token, unix socket or allow-list) |
| 107 | cortex-t212-edgerate | 9a48217 | per-hostname edge rate from cloudflared journal, engine hook | nerve 72 + replay pass; real flood test | cloudflared debug loglevel (rec: no), per-host thresholds |
| 108 | cortex-score-v2 | 7863e3d | score v2: ddos findings 15/10/4, covered findings cost 0, one deduction per cause | goldens unchanged (21/F, replay 61/D); `real_score_v2` | ddos weights and pass rule (a rec / b / c) |
| 109 | candle-zip-fix | a4f49b5 | zip 7.4.0 -> 9.0.5 via vendored candle-core 0.9.2 (RUSTSEC-2026-0367/8/9 cleared) | workspace 534/0; real voice 98.2% word match | C1 keep vendored copy (rec), C2 ask upstream (public post) |
| 110 | integration-3 | 625eaf0 | integration-2 + 8 branches (no cortex-s2-*) | workspace 1262/0/7; real heal_tick 10/10, gaia_live, redteam | 10 ownership rows, heal plan v2 wording (security-relevant), strict HTTP for desk/room/link-watch |
| 111 | integration-verify | abd4dea | Re-ran the main-merge blockers on integration-2 | 7/7 old real scripts; REUSE 1093/0; cargo-deny licences/bans/sources pass, advisories fail (zip, paste) | VPS IP in survival-heal.md (fixed on integration-4), paste ignore or replace, 21 IPs + 1 quick-tunnel host in export |
| 112 | realscript-port-1 | 9d04111 | 9 real scripts -> Rust tests, .sh deleted | 236 pass / 1 fail (docs-sync: 29 old timestamp warnings) | fix the 29 warnings or loosen docs-sync; merge 112-114 together |
| 113 | realscript-port-2 | cd4bf26 | 9 more scripts -> Rust (`galion_realtest::realrun`) | all real pass; workspace 1011/0/4; live replay 66/100 D | gitleaks and SBOM gates skipped (tools missing) |
| 114 | realscript-port-3 | 03967fc | last 8 scripts -> Rust; new `galion-ccwrap` | workspace 1006/0/4; live check 41/43 | tuner parity: 2 malformed requests 400 vs live 200 (A keep strict, B relax) |
| 115 | ci-workflow-rust | 38a26cb | new `crates/xtask`; every CI `run:` step is `cargo xtask` | xtask 11/0; parity with bash; gitleaks 293 commits clean; launch-gates 1-7, 9 pass, 8 fails (zip/paste), 10 warn | gate 10 (public IPs in export), release enablement. Not run on GitHub Actions |
| 116 | vps-traffic-hermes | 1c40c5e | Steady conns are our own pollers (galion-one 1 s, link-watch 3 s) | measured 1.36-1.4/s + 1.00/s vs predicted 1.33 + 1.0 | P1-P5 (drop self-check, 5 s / 15 s intervals, curls -> Rust, cortex baseline) |
| 117 | vps-strays | 99e7c58 | Documented 5 strays (8431-8433, 8652/8653); found and fixed a MockProvider memory leak (7106b77) | box: +5.5 MB/round unpatched vs flat ~4.9 MB fixed | stop the strays (done in the deployment, see below), merge the leak fix |
| 118 | vps-tuner-dns | f67f37f | tuner.galion.app: only the DNS record is missing; ingress is live | NXDOMAIN at 1.1.1.1 / 8.8.8.8; tuner /health 200 | D118-1 proxied CNAME in the dashboard (rec A), D118-2 then stop tuner-quick |
| 119 | backup-perf | 5c3a8ed | galion-backup perf on a VPS-sized tree; CRC-32 slicing-by-8; gzip level knob | median of 5: bash 3.46 s, Rust gnu 3.97 s, musl 4.30 s; Rust less RAM; parity 26/26 | B1 gzip level 6 or 5 (rec 5), B2 ZFS size_kb log value |
| 120 | tuner-quick-unit | bff2d94 | `deploy/staged/tuner-quick.service` (DynamicUser, no /root) + runbook | systemd-analyze security 1.4 vs live 9.6 | O4 switch now / next reboot / stop / named hostname; live quick tunnel runs as root and can read the named tunnel credential |
| 121 | galion-deploy-fastfail | 7a1a615 | opt-in `GALION_DEPLOY_FAILFAST=N` | broken tuner rollback 20.2 s -> 4.2 s (N=2); regression 24/19/8/7 | enable N=2 in runbooks (rec) |
| 122 | shared-tar-crate | 820740e | std-only `crates/galion-tar` shared by backup and deploy | files.tar byte-identical, = GNU tar 1.35; parity 26/26 | D-T1 keep ustar (rec) |
| 123 | guard-shadow | 7b7b1fc | galion-guard shadow backend (log-only) + `compare` | live VPS replay 04:41: failures 983/983, bans 121/121, 0 one-sided = parity | start a live shadow run (a/b/c), duration, repeat-ban cap |
| 124 | hades-option-b | 0f41f29 | HADES option B prototype: typed signed plan steps, parallel path | red team prose 0/82, typed 0/164; hades-gate 61/0; workspace 512/0 | D-B1 A / B / B' / C, D-B2 service filter, D-B3 runbook step |
| 125 | hades-ledger-fail | 84fef87 | silent ledger write failures now detectable (stderr line, exit 3, HTTP 503) | workspace 504/0; real_ledger_fail, real_redteam 0/82 | deploy hades-gate as a unit |
| 126 | sec-history-purge-plan | d45b248 | Plan only for the SEC-1..5 history purge (no rewrite, no force-push) | 96 paths / 93 blobs; rehearsal rewrite 2.56 s, fsck clean; tool 20/0/1 | D1-D6 (route A/B, scope, VPS IP in 21 paths, timing after rotations); rotations P0 first |
| 127 | hermes-core-docs | 3611fc6 | `docs/hermes-core.md` section 7 for the Revolut lift | 42 tests; probe shows money asks classify as `code` (no money keywords) | money class (a) always refused (rec) / (b) HADES money kind / (c) later. Head moved c069839 -> 3611fc6 at 04:46 (re-check commit); the push of 3611fc6 is not logged |
| 128 | hermes-gateway-bench | 20d9a2f | 36 bench tools in hermes-gateway behind feature `bench` + `HERMES_RS_BENCH=1` | gateway 4/4; real loopback 8 tool calls, payment stays awaiting_owner; workspace 577/0 | H4 merge, public bind (rec refuse), cron, 2.46 MB > 2 MB |
| 129 | hermes-core-bench-sector | 12be83b | 8th Hermes sector `bench`, CLI + `GET /v1/bench`, readiness G16 | workspace 750/0; real_bench RB1-RB8 + real_hermes R1-R8 | H1 live bench, bench crate mapping, `/usr/local/bin/hermes` name clash on VPS |
| 130 | hermes-readiness-recheck | 7c7a613 | Readiness re-run: 11/15 not ready | G9 no HADES key, G11 paid model not approved, G12 not on main, G14 7 decisions; VPS: 0 OpenRouter 402 lines since 10-18 | same four gates |
| 131 | bench-banking-adv | 44e5c61 | adversarial banking set (memo injection, payee social engineering) | gate 38/38 refuse, 16/16 allow; e2e 396 write attempts / 0 ok; held-out 8/15 | live scoring waits on H1 |
| 132 | bench-ws-adv | 2ecfc19 | adversarial Wall Street set, 212 cases | screen 100% vs v1 phrase-only 30%; real 212/212 | live scoring waits on H1 |
| 133 | bench-vending-adv | 1b0a30c | hard vending level + pricing axis | scripted 200/200 hard seeds; standard seeds byte-identical to 457ef74; x86_64 = aarch64 | B3 live run waits on H1 |
| 134 | bench-data-licence | 493f175 | 19 market-data sources with terms | committable: US Treasury, Fed H.10, ECB, NY Fed SOFR | D-B2-1 a/b/c |
| 135-139 | console-port-1..5 | 394f065, 0916684, 9f32aa9, f14dbdc, 38b92a7 | The 17 remaining TS console routes ported to Rust console-ui, no JS (5 branches) | each: workspace ~550/0, real vs live VPS (GET only) 2-4 runs pass | per-route: forms for POST actions, JS ruling for Hermes ask, `/api/v1/bank` owner; branches overlap in 5 files, merge together |
| 140 | whitepaper-update | (8c0ac37) | white paper evidence with tonight's results | running (attempt 4, 06:34) | — |
| 141 | slavik-todo-hr | (87fdd91) | SLAVIK-TODO in short Croatian | running (attempt 4, 06:34) | — |
| 142 | processes-refresh | this branch | This refresh | see `docs/real-tests/processes-refresh.md` | none |
| 143-145 | galion-os-threat-model, galion-os-sandbox-proto, toolchain-aarch64 | — | Galion OS threat model, aarch64 sandbox prototype, repo-level rust-lld config | running (relaunched 06:34) | — |
| 146-149 | serde-order-pin, voice-speed, one-no-shellout, health-fixes | — | see wave4-tasks.md | not started | — |

## Side jobs on 2026-10-21

| Job | Head | What it did | Status | Owner decisions |
|---|---|---|---|---|
| live-ai-plan | 644c4f7 | Plan for testing Gaia with the live AI on real VPS problems (P0-P5), `docs/plans/live-ai-real-problems.md` | done 04:43, plan only | H1 (paid model), S1-S7 |
| whitepaper-vending | 5ddd828 | B3 vending white paper; seeds 1-200 200/200 | done 04:34 | — |
| settings-snapshot | 241aecf | `swarm-settings.toml`: sandbox_vps_only, no_skip_extensive_testing, per-sector limit placeholder (309c17b at 04:33) | on GitHub; the 20-per-sector commit was dropped (Slavik: skip it) | exact per-sector number |
| integration-4 / -squash | 42de672 / 9b5f6ad | integration-3 with the VPS IP removed; squash branch on main 13a296c has 0 IP hits in history | on GitHub; workspace 1262/0/7 | which one to merge; candle-zip-fix not folded (Cargo.lock conflict) |
| nerve-shadow-20261021 | 4800770 | integration-4 + cortex t20-merge + score-v2 + state-titles | deployed as shadow (below); push not logged | nerve-wiring-2 conflicts in 7 files |
| vps-deploy-20261021 | d9bd829 | deploy record + VPS sandbox test docs | on GitHub (push approved 04:48) | — |
| sandbox-real-problems | 334c633 | 10 real-life scenarios in openmind-sandbox on the VPS: 8 pass, S8 secrets FAIL (`link.env` mode 666), S9 no total disk quota, S10 /proc gap | **local only, not pushed** | chmod 600 link.env and/or hide /opt/galion in presets (Slavik) |

## VPS deployment, 2026-10-21 04:21-04:48 CEST

Mate relayed "do the migrations and deployment work on the VPS" (Slavik's auto-accept of Oct 20). Limits kept: no main merge, no DNS change, HADES mode unchanged, Hermes paid model off, no secrets printed, `/opt/galion/openmind` untouched.

1. 04:31 backup + verify: `galion-deploy backup` to `/var/backups/galion/cutover-20261021` (0700, stays on the VPS), `backup-verify` OK.
2. 04:31:47 Rust `galion-deploy` installed (`2a60e1ac`, from integration-4); the bash version is kept as a backup.
3. 04:32 `tuner` swapped to the integration-4 build (`89c7fcbc`). A wrong probe (`//`) caused one needless rollback at 04:32:10; re-deployed 04:32:32, identical responses to the old binary.
4. 04:34 cloudflared `config.yml` 666 -> 644. Validate OK, no restart.
5. 04:35 the 5 strays (8431-8433, 8652, 8653) stopped one at a time after archiving `/root/growth-20261018`; loopback health 200 before and after each step.
6. 04:37 new unit `galion-nerve-shadow` (127.0.0.1:8672, observe-only, dry run, no reflexes, not enabled at boot). Live `galion-nerve` 8670 untouched.
7. Not done on purpose: nothing sandbox-dependent (the VPS sandbox test at 04:27 showed the host process list is visible inside the sandbox: the LXC refuses the proc mount); Hermes tools execution stays off. The dead sites (galion.app, galion.studio, adriaticpadel.com) have no built app or service to restart.

**Re-checked read-only at 06:36 CEST (slot 142):** live nerve 8670 `/state` 200, mode live, cycle 6723, 15 components (11 healthy, 2 degrading: galion-one-mouth, cloudflared; 2 absent: voice-worker, gaia). Shadow 8672 `/healthz` 200, observe-only, cycle 238, 3 incidents, cortex not live (shadow only). `galion-deploy` = 2a60e1ac, `tuner` = 89c7fcbc, `galion-nerve-shadow` = 3a566c56 (all match the record). Ports 8431-8433, 8652, 8653 not listening. `config.yml` 644; `link.env` still 666. tuner.galion.app still NXDOMAIN. 3 failed transient units `run-u11093..5` date from 2026-10-18 22:13 CEST (older than the swarm).

## Standing rules added on 2026-10-21

- 00:52 Slavik: 50-agent groups, ceiling 250 (the swarm's ceiling, not Grok Bot's own limit).
- 04:29 `sandbox_vps_only`: the sandbox runs only on the VPS as an ordinary user in `/var/tmp/sandbox-test-<ts>`, cleaned up after; local workspace tests exclude openmind-sandbox and cgroup-detect.
- 04:33 per-sector agent limit is a placeholder in `swarm-settings.toml` until Slavik gives the number. 04:43: skip the 20-per-sector rule, nothing implemented.
- 04:38 `no_skip_extensive_testing`: every agent tests on real data on the real VPS and watches the nerve (and the cortex once it is live).
- Commit and record times in Zagreb time with +02:00 (GitHub shows +0200).

## Open owner decisions (rolled up, 06:41 CEST)

1. HADES admin key (SLAVIK-TODO 1). It blocks Hermes readiness G9 and real approvals.
2. Strict request handling packet (SLAVIK-TODO 3, common-migrate-2); tuner parity on integration-2 (slot 114: A keep strict, B relax).
3. Merge: which integration branch goes to main (`integration-4` or `integration-4-squash`, the squash has no VPS IP in history), merge order of the wave-4 branches (112-114 together, console-port-1..5 together, cortex branches vs nerve-wiring-2), and how main gets pushed (the helper is `swarm/*` only).
4. POSEIDON vs HEPHAESTUS ownership of compute; Gaia ownership rows (10 on integration-3, bench crates).
5. HADES plan format: A / B (prototype on `hades-option-b`) / B' / C; service filter; deploy hades-gate as a unit.
6. Cutovers still open: nerve 8670 (shadow on 8672 is running), console 8080, desk, room; tuner-quick unit (O4: now / next reboot / stop / named hostname); tuner.galion.app CNAME (D118-1, dashboard only).
7. `link.env` mode 666 readable from the sandbox: chmod 600 and/or hide `/opt/galion` in the sandbox presets. Sandbox-dependent deploys stay blocked until /proc isolation holds on the VPS.
8. H1: Hermes's paid model for live benchmark runs (all adversarial and live-AI work waits on it). Money class in Hermes intake: (a) always refused is recommended.
9. Rust-only scope: OS packages, cloudflared, CI YAML, browser JS/TS, pdftotext/curl, bundled SQLite (redb + tantivy), libFuzzer/CodeQL licences; candle: keep vendored copy (C1), ask upstream (C2, public post).
10. Live guard shadow run on the VPS (slot 123), cortex enable in production, `/event` auth, cortex weights.
11. Voice: clips and licence, watermark, GPU vs CPU.
12. Market data: one of the four committable public sources (D-B2-1).
13. Galion OS acceptance questions 1-7.
14. SEC-1..5 key rotation and the history purge plan (slot 126, D1-D6); the VPS archive bundle keeps the old history until then.
15. Pushes not yet logged: `sandbox-real-problems` (334c633, local only), `nerve-shadow-20261021` (4800770, the source of the running shadow), `hermes-core-docs` 3611fc6. They need a push (or a check) before anyone relies on GitHub for them.
