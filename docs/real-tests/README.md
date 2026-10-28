<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-test records: index

Docs T2.3 (wave 4 slot 171, `swarm/docs-realtests-index`, base `swarm/integration-4` 42de672).
Written 2026-10-21 10:15 CEST (+02:00), Europe/Zagreb. All times below are Europe/Zagreb, CEST (+02:00).

This page lists every real-life test record: the 42 records (plus 3 raw-log folders) on integration-4 42de672
in this folder, and the real-test evidence from the wave-4 branches logged in `/workspace/swarm/CALL-RECORD.md`
(box file, not in the repo). Each row says where the result comes from, so you can check it in seconds.

## How to read the tables

- **Branch / head SHA**: the `swarm/*` branch that wrote the record, and the commit that last changed it on
  integration-4 (the commit that wrote it is in brackets if it differs). Wave-4 rows give the branch head SHA from
  the DONE line in CALL-RECORD, resolved to a commit in that slot's worktree (`/workspace/swarm/wt/<slug>`).
- **Measure**: what was really run.
  `box-real` = release binary on the swarm box, real TCP/CLI on loopback, started and stopped by the test.
  `vps-ro` = read-only probes of the live VPS (GET, `systemctl status`, `ss -ltn`, journal reads).
  `vps-sandbox` = sandbox run on the VPS in `/var/tmp/sandbox-test-*` as a temp user (rule sandbox_vps_only).
  `vps-live` = an approved live change on the VPS (deploy slots only). `docs` = doc or plan work, no binary.
- **Number**: the headline result as the record states it. **Evidence** is `file:line` where that number is written.
- `GALION_REAL=1` tests skip without that variable, so a plain `cargo test` does not run them.

## A. Records on integration-4 42de672 (`docs/real-tests/`)

| # | Record | Branch | Head SHA | Measure | Number | Evidence | Run time (+02:00) |
|---|---|---|---|---|---|---|---|
| 1 | [bench-banking](bench-banking.md) | swarm/bench-banking | adda699 | box-real (local mock bank, no VPS) | B1 banking 100% over 10 axes | bench-banking.md:31 | 2026-10-20 23:15 |
| 2 | [bench-harness](bench-harness.md) | swarm/bench-harness | ea8315c | box-real (galion-bench-mock) | 2 passed, 0 failed | bench-harness.md:27 | 2026-10-20 23:03 |
| 3 | [bench-hermes](bench-hermes.md) | swarm/bench-hermes | 457ef74 | box-real (4 release binaries + mock LLM, no key) | 1 passed: 3 benches 100%, ports free after | bench-hermes.md:43 | 2026-10-20 23:47:38-23:47:42 |
| 4 | [bench-scoreboard](bench-scoreboard.md) | swarm/bench-scoreboard | f2ff169 (9d7b3f4) | box-real (CLI only) | real_cli 1 passed, unit 6 passed | bench-scoreboard.md:10 | 2026-10-20 23:10:00 |
| 5 | [bench-vending](bench-vending.md) | swarm/bench-vending | a9daaf1 | box-real (release, real TCP) | 1 passed, 0 failed | bench-vending.md:23 | 2026-10-20 23:14:02 |
| 6 | [bench-wallstreet](bench-wallstreet.md) | swarm/bench-wallstreet | 4b1e127 | box-real (paper broker over HTTP) | 1 passed; score 100/100 checks | bench-wallstreet.md:39, :61 | 2026-10-20 23:10-23:12 |
| 7 | [build-info](build-info.md) | swarm/build-info | db9924d | box-real (10 release binaries `--version`) | 10 binaries answer in under 1.1 ms (limit 100 ms) | build-info.md:11 | 2026-10-20 23:05 |
| 8 | [common-migrate-2](common-migrate-2.md) | swarm/common-migrate-2 | 123eab2 (cf40c1b) | box-real (8 old vs new servers, parity + hammer) | 87 byte-identical, 11 identical after masking, 0 different | common-migrate-2.md:52 | 2026-10-20 21:26:18-21:30:10 |
| 9 | [common](common.md) | swarm/common | 123eab2 (26d98b0) | box-real (script run, later ported to Rust) | legit GETs during slowloris: new tuner 41/60, old origin 0/60 | common.md:107 | 2026-10-20 20:02:25-20:04:01 |
| 10 | [cortex-sensor](cortex-sensor.md) | swarm/cortex-sensor | e536446 | box-real (replay goldens + serve; replay of a VPS capture) | replay1 61/100 D unchanged; workspace 965/0/4 | cortex-sensor.md:17, :50 | 2026-10-20 23:05-23:06 |
| 11 | [cutover-prep](cutover-prep.md) | swarm/cutover-prep | a5ac49b | box-real + vps-ro (inventory) | backup-verify passed twice; workspace 546 passed, 0 failed | cutover-prep.md:18, :45 | 2026-10-20 23:30:00, 23:44:40; VPS 23:13:33 |
| 12 | [deploy-rust](deploy-rust.md) | swarm/deploy-rust | 787a54d (9899537) | box-real (systemd stand-in rehearsals) | tuner 24/0, nerve 19/0, room 7/0, desk 8/0 (pass/fail) | deploy-rust.md:34-37 | 2026-10-20 23:20:48 |
| 13 | [deploy-sim](deploy-sim.md) | swarm/deploy-sim | c5ab8f9 | box-real (rehearsal run 5) | gated test 1 passed (22.89 s); 21/21 tuner routes byte-identical | deploy-sim.md:122, :86 | 2026-10-20 21:14-21:22 |
| 14 | [desk-linkwatch](desk-linkwatch.md) | swarm/desk-linkwatch | a54c792 | box-real (+ VPS `ss` read) | PASS; tests 1 + 3 passed | desk-linkwatch.md:11, :78-79 | 2026-10-20 19:07:36-19:08:41 |
| 15 | [docs-sync](docs-sync.md) | swarm/docs-sync | 80e28f3 | box-real (repo-check) | 723 files, 0 errors, 0 warnings | docs-sync.md:41 | 2026-10-20 18:58:32 |
| 16 | [edge-boot](edge-boot.md) | swarm/edge-boot | 753676e | box-real (omnode boot, qemu) | 84 passed, 0 failed | edge-boot.md:15 | 2026-10-20 19:38:57-19:39:07 |
| 17 | [gaia-hades-tokens](gaia-hades-tokens.md) | swarm/gaia-hades-tokens | f3f41a2 | box-real (signed tokens, real nerve + dummy) | real test 1 passed; heal_tick 10 passed | gaia-hades-tokens.md:14, :17 | 2026-10-20 20:22:19-20:22:21 |
| 18 | [gaia-land](gaia-land.md) | swarm/gaia-land | 9d3ec18 (4fee020) | box-real (CLI on a read-only VPS capture) | dry-run 52 systems, 52 planned, 5 destructive refused, 0 acted | gaia-land.md:24 | 2026-10-20 20:48 |
| 19 | [gaia-live](gaia-live.md) | swarm/gaia-live | 4fbbb7f | box-real (real nerve + dummy, end to end) | restarts 3/3 then cap; workspace 593/0 | gaia-live.md:78, :144 | 2026-10-20 19:16:46-19:16:53 |
| 20 | [gaia-poseidon](gaia-poseidon.md) | swarm/gaia-poseidon | 498e3d2 | box-real (gaia -> openmind-queue) | RESULT pass=60 fail=0 | gaia-poseidon.md:204 | 2026-10-20 20:24:32-20:24:39 |
| 21 | [guard-rust](guard-rust.md) | swarm/guard-rust | 26a2a8e (d5f7874) | box-real (user+net namespace, real nft 1.1.3) | 38 passed, 0 failed (Rust A2 run) | guard-rust.md:231 | 2026-10-20 23:13:06-23:13:57 |
| 22 | [hades-approval](hades-approval.md) | swarm/hades-approval | ccaab42 | box-real (musl release, throwaway keys) | ALL PASS; real_cli 1 passed | hades-approval.md:98, :8 | 2026-10-20 19:03:25-19:03:29 |
| 23 | [hades-redteam](hades-redteam.md) | swarm/hades-redteam | e6ceca7 | box-real (82 attacks + 9 legit plans) | 1 passed (0 of 82 bypass) | hades-redteam.md:46 | 2026-10-20 22:57:51 |
| 24 | [health-land](health-land.md) | swarm/health-land | f4a66ea | box-real vs vps-ro (:8660) | same key set (87 keys), 12/12 checks named alike | health-land.md:29 | VPS 2026-10-20 22:52:05-22:52:18; box 22:55:54 |
| 25 | [hermes-tools](hermes-tools.md) | swarm/hermes-tools | 8ccdfcc | box-real (sandboxed tools, mock model, no paid call) | agent-loop test ok; 7 escape attempts all BLOCKED | hermes-tools.md:133, :104 | 2026-10-20 20:05:00-20:05:01 |
| 26 | [integration-1](integration-1.md) | swarm/integration-1 | 8fd26f6 | box-real + vps-ro (one GET-only run) | workspace 950 passed, 0 failed, 4 ignored | integration-1.md:19 | 2026-10-20 20:26:51-20:29:23 |
| 27 | [integration-3](integration-3.md) | swarm/integration-3 | 625eaf0 | box-real (6 suites) | heal_tick 10/10, real_gaia_live 1/1, real_cli 2/2, redteam 1/1, version 1/1, health 1/1 | integration-3.md:17-22 | 2026-10-21 01:55-02:02 |
| 28 | [lan-discovery](lan-discovery.md) | swarm/lan-discovery | fe5f0bd | box-real (mDNS + /24 probe on loopback) | 1 passed; probe 4 attempts, 3 open | lan-discovery.md:112, :72 | 2026-10-20 19:24:03-19:24:29 |
| 29 | [lan-routing](lan-routing.md) | swarm/lan-routing | e8169c9 | box-real (mock OpenAI servers) | PASS 3 runs in a row + 1 gated cargo test | lan-routing.md:13 | 2026-10-20 20:47:21-20:48:01 |
| 30 | [launch-gates](launch-gates.md) | swarm/launch-gates | 79ee761 | box-real (public export + scanners) | 11 ok, 0 failed | launch-gates.md:51 | 2026-10-20 20:02:17-20:02:36 |
| 31 | [memory-real](memory-real.md) | swarm/memory-real | 723b46c | box-real + vps-ro (GET :8420) | 7 passed, 0 failed | memory-real.md:37 | 2026-10-20 19:21:22-19:22:25 |
| 32 | [nerve](nerve.md) | swarm/nerve | 5802d0e | box-real (serve + replay of a VPS capture) | replay1 61/100 D (FAIL, as expected); real test 1 passed | nerve.md:61, :241 | 2026-10-20 19:24:35 |
| 33 | [one-probe](one-probe.md) | swarm/one-probe | b44cd4f | box-real (musl release) | 1 passed | one-probe.md:144 | 2026-10-20 19:00:50-19:02:50 |
| 34 | [poseidon](poseidon.md) | swarm/poseidon | 7d0c478 | box-real (release queue) | RESULT pass=51 fail=0 | poseidon.md:131 | 2026-10-20 19:30:24-19:30:33 |
| 35 | [real-harness](real-harness.md) | swarm/real-harness | 5b3c662 | box-real (stack under galion-boot) + vps-ro | 77 checks: 66 pass, 0 unexpected FAIL, 10 known-fail, 1 skip | real-harness.md:19 | 2026-10-20 19:20:49 |
| 36 | [room](room.md) | swarm/room | 5bb27e1 | box-real | 30 passed (+5, +2, +2 in other suites) | room.md:45 | 2026-10-20 19:04:13 |
| 37 | [rust-ports](rust-ports.md) | swarm/rust-ports | 42de672 (26a2a8e) | box-real (bash parity) + vps-ro | 26 passed, 0 failed | rust-ports.md:86 | VPS 2026-10-20 22:56-23:00; box 23:12:45 |
| 38 | [sandbox](sandbox.md) | swarm/sandbox | d194c19 | box sandbox run (before the sandbox_vps_only rule, see notes) | real_escape 1 passed (all attacks CONTAINED); 24 passed x25 | sandbox.md:159, :85 | 2026-10-20 ~19:1x |
| 39 | [survival-heal](survival-heal.md) | swarm/survival-heal | 42de672 (45a18aa) | box-real (dummies) + VPS status capture | 3 restarts then cap 3/3 held; 12/12 observe on live capture | survival-heal.md:71, :124 | 2026-10-20 19:11:03-19:12:29 |
| 40 | [tuner-cutover](tuner-cutover.md) | swarm/tuner-cutover | 5fd1611 | box-real vs vps-ro (live :8650) | 21/21 routes identical, 0 diff | tuner-cutover.md:46 | 2026-10-20 19:10:59 |
| 41 | [ucpu](ucpu.md) | swarm/ucpu | 3e70ffb | box-real (x86_64 + aarch64/armv7 under qemu) | 20 passed, 0 failed | ucpu.md:16 | 2026-10-20 19:01:40 |
| 42 | [voice-mouth](voice-mouth.md) | swarm/voice-mouth | 5d776c1 | box-real (CPU STT/TTS) + vps-ro | real test ok; STT real-time factor 0.15 (4 threads) | voice-mouth.md:78, :57 | 2026-10-20 19:15-19:16 |

Raw logs kept next to the records: [captures/](captures/) (VPS :8660 status captures 2026-10-20, one synthetic what-if),
[deploy-rust/](deploy-rust/) (rehearsal logs for row 12), [deploy-sim/](deploy-sim/) (run-5 logs for row 13).

## B. Wave-4 branches (from the Wave 4 log in `/workspace/swarm/CALL-RECORD.md`)

Records here live on each slot's own branch, not yet on integration-4. "CR line" is the line number of the DONE
entry in CALL-RECORD.md as of 2026-10-21 10:15 (+02:00); the file only grows at the end, so the numbers stay valid.
"Push" is what the DONE line says: `pushed`, `LOCAL` (commit only in the box worktree), or `n/s` (not stated).
Numbers are copied from the DONE line; the record file on the branch has the full output.

| Slot | Branch | SHA | Push | Record on branch (file:line) | Measure | Numbers (as logged) | CR line | Done (+02:00) |
|---|---|---|---|---|---|---|---|---|
| 103 | swarm/tunnel-audit-2 | 988e47f | n/s | tunnel-audit.md:10 | box-real (VPS copy with tuner origins) | real run 2 passed | 187 | 2026-10-21 00:59 |
| 100 | swarm/fuzz-sast-2 | f3a10af | n/s | fuzz-sast.md:68 | box-real (fuzz replay) | HTTP 5666 files, 0 refused; mDNS 1429 datagrams; workspace 551/551 | 188 | 01:00 |
| 102 | swarm/hermes-enroll-2 | c28374e | n/s | hermes-enroll.md:13 | box-real | real_enroll 72/0; workspace 509/0 | 189 | 01:02 |
| 101 | swarm/nerve-wiring-2 | d97dab9 | n/s | nerve-wiring.md:172 | box-real | 2 real runs passed; workspace 567/0; hermes p99 nerve-down +45-82% (open) | 190 | 01:02 |
| 104 | swarm/cortex-t20-merge | ee75f75 | n/s | cortex-t20-merge.md:19 | box-real (replay goldens) | workspace 1025/0; 7 GALION_REAL tests pass; golden 21/F unchanged | 191 | 01:06 |
| 105 | swarm/cortex-state-titles | 967459d | n/s | cortex-state-titles.md:56 | box-real | openmind-nerve 84/0; before/after real test | 194 | ~01:15 |
| 106 | swarm/cortex-event-ingest | 7a55e6c | n/s | cortex-event-ingest.md:30 | box-real + vps-ro | workspace 975/0/4 ignored; real event feed test ok | 195 | ~01:19 |
| 107 | swarm/cortex-t212-edgerate | 9a48217 | n/s | cortex-t212-edgerate.md:31 | box-real (flood test) | nerve 72 unit + replay pass; replay1 61/D | 196 | ~01:23 |
| 109 | swarm/candle-zip-fix | a4f49b5 | n/s | candle-zip-fix.md:14 | box-real + cargo-deny | RUSTSEC-2026-0367/8/9 cleared; 534/0 | 199 | ~01:50 |
| 108 | swarm/cortex-score-v2 | 7863e3d | n/s | cortex-score-v2.md:14 | box-real (replay + serve) | replay fixture 21/F; serve 71/C then 75/C | 200 | ~01:55 |
| 110 | swarm/integration-3 | 625eaf0 | n/s | integration-3.md:17 (row 27 above) | box-real | workspace 1262/0/7 ignored; heal_tick 10/10 | 201 | ~02:02 |
| 111 | swarm/integration-verify | abd4dea | n/s | integration-verify.md:23 | box-real | 7/7 old real scripts pass; REUSE 1093/0; advisories FAIL (zip 7.4.0) | 204 | ~02:25 |
| 112 | swarm/realscript-port-1 | 9d04111 | n/s | realscript-port-1.md:22 | box-real | 9 scripts -> Rust tests; 236 pass | 205 | ~02:27 |
| 113 | swarm/realscript-port-2 | cd4bf26 | n/s | realscript-port-2.md:47 | box-real | 9 scripts -> Rust; workspace 1011/0/4; live 66/100 D | 206 | ~02:31 |
| 118 | swarm/vps-tuner-dns | f67f37f | pushed | vps-tuner-dns.md:53 | vps-ro | tuner :8650 /health 200; tuner.galion.app NXDOMAIN (DNS record missing) | 209 | ~02:37 |
| 117 | swarm/vps-strays | 99e7c58 | pushed | vps-strays.md:16 | vps-ro + box-real (leak repro) | 5 strays found, 0 connections; mock chat leak (8652 53 MB) | 210 | 02:51 |
| 116 | swarm/vps-traffic-hermes | 1c40c5e | pushed | - (docs/ops/steady-traffic.md) | vps-ro | :8642 1.36-1.4/s measured vs 1.33/s predicted; :8091 1.00/s | 219 | ~03:40 |
| 115 | swarm/ci-workflow-rust | 38a26cb | pushed | ci-workflow-rust.md:13 | box-real (xtask parity) | xtask 11/0; SBOM 153 pkgs identical to bash | 220 | ~03:45 |
| 114 | swarm/realscript-port-3 | 03967fc | n/s | realscript-port-3.md:49 | box-real | 8 scripts -> Rust; workspace 1006/0/4 | 217 | ~03:38 |
| 119 | swarm/backup-perf | 5c3a8ed | pushed | backup-perf.md:84 | box-real (VPS-sized tree) | parity 26/26; Rust gnu +15% wall vs bash, less RAM | 221 | ~03:53 |
| 120 | swarm/tuner-quick-unit | bff2d94 | pushed | tuner-quick-unit.md:20 | box (systemd-analyze) + vps-ro | security 1.4 OK vs live 9.6 UNSAFE | 227 | ~04:10 |
| 121 | swarm/galion-deploy-fastfail | 7a1a615 | pushed | galion-deploy-fastfail.md:52 | box-real (rehearsal) | tuner 12/0, nerve 5/0; regression 24/19/8/7 pass | 228 | ~04:15 |
| 122 | swarm/shared-tar-crate | 820740e | pushed | shared-tar-crate.md:32 | box-real | backup vs bash 26/26; tar byte-identical to GNU tar 1.35 | 229 | ~04:20 |
| 127 | swarm/hermes-core-docs | c069839 | pushed | hermes-core-docs.md:6 | box-real (CLI) | Revolut intake rows queued, nothing moved | 234 | 04:40:13 |
| 123 | swarm/guard-shadow | 7b7b1fc | pushed | guard-shadow.md:67 | box-real + vps-ro replay | live replay failures 983/983, bans 121/121 | 236 | ~04:45 |
| 128 | swarm/hermes-gateway-bench | 20d9a2f | pushed | hermes-gateway-bench.md:19 | box-real | 4/4; workspace 577/0 | 238 | ~04:53 |
| 125 | swarm/hades-ledger-fail | 84fef87 | pushed | hades-ledger-fail.md:86 | box-real | 504 passed; red team 0/82 | 241 | 04:53:21 |
| 124 | swarm/hades-option-b | 0f41f29 | pushed | hades-option-b.md:33 | box-real | red team 0/82, 0/164; legit 9/9; 512/0 | 240 | 04:55:12 |
| - | swarm/whitepaper-vending | 5ddd828 | pushed | - (docs/whitepapers/vending-benchmark.md) | box-real (sim re-run) | seeds 1-200: 200/200 at 10000 bp | 262 | 04:34:35 |
| - | swarm/live-ai-plan | 644c4f7 | pushed | - | docs + vps-ro | plan only, nothing run | 271 | 04:43:20 |
| 126 | swarm/sec-history-purge-plan | d45b248 | pushed | sec-history-purge-plan.md:26 | docs (plan) | 20/0/1 | 297 | 05:10:18 |
| 134 | swarm/bench-data-licence | 493f175 | pushed | bench-data-licence.md:12 | docs + HTTPS GETs | 19 sources checked; ECB/Treasury/H.10/SOFR committable | 298 | 05:14:11 |
| 132 | swarm/bench-ws-adv | 2ecfc19 | pushed | bench-ws-adv.md:58 | box-real | 147/147, 65/65, 212/212 | 300 | 05:16:44 |
| 130 | swarm/hermes-readiness-recheck | 7c7a613 | pushed | hermes-readiness-recheck.md:59 | box-real + vps-ro | readiness 11/15 NOT READY; 626/0 | 301 | 05:17:08 |
| 131 | swarm/bench-banking-adv | 44e5c61 | pushed | bench-banking-adv.md:55 | box-real | refuse 38/38, allow 16/16, episodes 7/7; 591 passed | 302 | 05:20:47 |
| 136 | swarm/console-port-2 | 0916684 | pushed | console-port-2.md:38 | box-real | workspace 553/0/2 | 303 | 05:28:31 |
| 133 | swarm/bench-vending-adv | 1b0a30c | pushed | bench-vending-adv.md:26 | box-real | hard 200/200; 601/0 | 313 | 05:45:50 |
| 135 | swarm/console-port-1 | 394f065 | pushed | console-port-1.md:57 | box-real | 556/0; 12/12 rows | 314 | 05:45:58 |
| 129 | swarm/hermes-core-bench-sector | 12be83b | pushed | hermes-core-bench-sector.md:22 | box-real | 750 passed; 8/8 done | 315 | 05:49:28 |
| 139 | swarm/console-port-5 | 38b92a7 | pushed | console-port-5.md:20 | box-real | 549/0/2; 3/3 pass | 316 | 05:51:32 |
| 137 | swarm/console-port-3 | 9f32aa9 | pushed | console-port-3.md:50 | box-real | 554/0/2 | 317 | 05:53:27 |
| 138 | swarm/console-port-4 | f14dbdc | pushed | console-port-4.md:21 | box-real | 557/0/2 | 318 | 05:54:56 |
| 141 | swarm/slavik-todo-hr | f72999e | pushed | - (docs/SLAVIK-TODO.md) | docs | spdx 0 missing | 335 | 06:36:27 |
| 143 | swarm/galion-os-threat-model | f76eb96 | LOCAL (galion-os repo) | - (docs/security/threat-model.md) | docs + vps-ro | 5/5 links ok | 336 | 06:38:23 |
| 140 | swarm/whitepaper-update | cc00dc4 | pushed | whitepaper-update.md:11 | box-real (re-runs) | red team 0/82; 9/9; 45/0; benches 100/100/100 | 337 | 06:39:24 |
| 142 | swarm/processes-refresh | 73e1641 | pushed | processes-refresh.md:44 | docs + vps-ro | 40/40 match; 35/0; nerve 11/15 healthy | 338 | 06:40:55 |
| 144 | swarm/galion-os-sandbox-proto | 4374605 | LOCAL (galion-os repo) | galion-os-sandbox-proto.md:16 | box-real + vps-sandbox | 12/12 x86_64, 12/12 aarch64; 10/10 applicable | 340 | 06:52:56 |
| 145 | swarm/toolchain-aarch64 | 4d0c036 | pushed | toolchain-aarch64.md:16 | box-real (cross builds) | 27/27 bins static; 964 passed | 344 | 06:53:11 |
| 146 | swarm/serde-order-pin | 1d222ad | pushed | serde-order-pin.md:31 | box-real | 978/0/3 and 962/0/3 | 378 | 07:08:29 |
| 149 | swarm/health-fixes | 62ddad8 | pushed | health-fixes.md:26 | box-real + vps-ro | 23/23; 14/14; 477 passed | 381 | 07:21:12 |
| 150 | swarm/sandbox-hide-galion | ad0ed9b | pushed | sandbox-hide-galion.md:36 | vps-sandbox | 2/2; 12/12; 957 passed | 390 | 07:30:34 |
| 147 | swarm/voice-speed | ff6c34f | pushed | voice-speed.md:13 | box-real | 506/0 | 407 | 07:57:30 |
| 153 | swarm/deny-paste-replace | 4bfffe8 | pushed | deny-paste-replace.md:27 | box-real + cargo-deny | 500/0/0; 60/0; 5/5 | 408 | 08:01:33 |
| 152 | swarm/gaia-hades-typed | c223109 | pushed | gaia-hades-typed.md:26 | box-real | 1242/0/6; 30/30; red team 0/82, 0/164 | 410 | 08:02:29 |
| 148 | swarm/one-no-shellout | 40454f9 | pushed | one-no-shellout.md:19 | box-real | 78/78; 40/40 repeat; 520 passed | 411 | 08:02:45 |
| 157 | swarm/nerve-dst-clock | c229a87 | pushed | nerve-dst-clock.md:56 | box-real | 151 passed; 997/0; 2/2 | 412 | 08:02:58 |
| 155 | swarm/nerve-wiring-rebase | 8046c41 | pushed | nerve-wiring-rebase.md:37 | box-real | 1225/0/6; 5/5 | 413 | 08:06:04 |
| 154 | swarm/candle-zip-rebase | b246f66 | pushed | candle-zip-rebase.md:10 | box-real + cross builds | 1229 passed; 37/37 static x86_64 + aarch64 | 416 | 08:07:40 |
| 160 | swarm/gaia-tick-loop | e4631a6 | pushed | gaia-tick-loop.md:32 | box-real | 1234/0/6; 67/0; 9/9 | 430 | 08:12:23 |
| 151 | swarm/sandbox-workdir-quota | 28f6c1d | pushed | sandbox-workdir-quota.md:15 | vps-sandbox | 5/5; 9/9; 957 passed | 432 | 08:13:53 |
| 167 | swarm/vps-dst-check | 56c8a7f | pushed | - (docs/ops/dst-2026-10-25.md) | vps-ro | VPS on Etc/UTC: timers unaffected by 2026-10-25 | 436 | 08:16:26 |
| 166 | swarm/vps-linkenv-doc | 8c257b9 | pushed | - (docs/ops/link-env-perms.md) | vps-ro | link.env mode 666 found, unchanged | 437 | 08:16:42 |
| 163 | swarm/hermes-legacy-retire | 7622b8b | pushed | hermes-legacy-retire.md:12 | vps-ro | hermes-rs :8642 200, 0 restarts | 445 | 08:18:57 |
| 159 | swarm/cortex-t26-ingresslint | 036f891 | pushed | cortex-t26-ingresslint.md:71 | box-real + vps-ro | 1003/0/3; 4/4 | 448 | 08:21:40 |
| 161 | swarm/poseidon-refusal-persist | eec2d99 | pushed | poseidon-refusal-persist.md:29 | box-real | 1221/0/6; 2/2 pass | 449 | 08:21:43 |
| 158 | swarm/cortex-merge-all | 6bcb882 | pushed | cortex-merge-all.md:54 | box-real + vps-ro capture | 1007/0/3; 12/12 real; live 66/D | 463 | 08:26:01 |
| 162 | swarm/hermes-bench-e2e | 2b11a5b | pushed | hermes-bench-e2e.md:44 | box-real | 758/0/0; 12/12 done | 467 | 08:28:00 |
| 164 | swarm/galion-os-box-escape | 7c7644e | LOCAL (galion-os repo) | galion-os-box-escape.md:15 | vps-sandbox | 14/0, 14/0, 8/8; all escape families CONTAINED | 469 | 08:29:20 |
| 172b | swarm/poseidon-deploy | d2c5cf1 | pushed | poseidon-refusal-persist.md:29 | vps-live (first install) | /healthz 200; kill -9 recovered, counters kept | 470 | 08:29:29 |
| 165 | swarm/galion-os-admin-cli | bf33cba | LOCAL (galion-os repo) | galion-os-admin-cli.md:17 | box-real + vps-sandbox | 25/25 x86_64, aarch64, armv7; real_vps 7/7 | 485 | 08:35:06 |
| t179 | swarm/galion-one-deploy | a93218d | pushed | one-no-shellout.md:19 | vps-live | :8091 200; galion.app/www/one 200; NRestarts 0 | 490 | 08:43:12 |
| 151b | swarm/hermes-tools-quota | 2b9944e | pushed | hermes-tools-quota.md:26 | vps-sandbox | 957/0/3; 10/10; 5/5; 6/6 | 491 | 08:44:57 |
| 175 | swarm/gaia-loop-decisions | b40208a | pushed | gaia-loop-decisions.md:36 | box-real | 1255/0/6; 4/4; 2/2 | 493 | 08:49:18 |
| 169 | swarm/console-port-merge | 508401d | pushed | console-port-merge.md:58 | box-real + vps GET | 645/0/7; 102/0; real_port1-5 pass | 517 | 08:58:36 |
| 181 | swarm/mouth-loading-diag | a68ad3e | LOCAL | mouth-loading-diag.md:21 | box-real + vps-ro | galion-one 39/39; real_mouth_loading 3/3 runs | 519 | 09:02:27 |
| 180 | swarm/nerve-t36-consumer | 334beaa | LOCAL (parent pushed later) | nerve-t36-consumer.md:29 | box-real | 1241/0/6; consumer_compat 11/11 | 521 | 09:10:36 |
| 179 | swarm/cortex-t32-correlate | ae604dc | pushed | cortex-t32-correlate.md:12 | box-real (replays) | 1018/0; 13/13 real | 528 | 09:29:42 |
| 178 | swarm/cortex-connrate-fix | c1a15f7 | pushed | cortex-connrate-fix.md:44 | box-real + vps-ro sample | replay vs live /proc/net/tcp sample | 534 | 09:35:03 |
| 177 | swarm/integration-5 | 0d68f46 | LOCAL (parent pushed 09:43) | integration-5.md:18 | box-real | 1241/0/6; real_wiring 2/2; 37/37 static x2 | 537 | 09:42:32 |
| 180 | swarm/nerve-eventlog-deploy | 0ddce9b | pushed | - (docs/ops/nerve-eventlog-nerveurl-20261021.md) | vps-sandbox + vps-live | nerve cycle 7, 15 comps, /events 200, 0 rollbacks | 544 | 09:48:32 |
| 176 | swarm/sandbox-quota-hide-merge | 78c5c49 | LOCAL | sandbox-quota-hide-merge.md:42 | vps-sandbox | 957/0/3; srp 10 scenarios 0 FAIL; 26/26 | 548 | 09:58:44 |
| 174 | swarm/bench-ws-ecb-data | 03714aa | LOCAL | bench-ws-ecb-data.md:30 | box-real on real ECB/Treasury data | 588/0/0; real_public_tape 1/1; verify 160/160 | 549 | 10:02:01 |

Wave-4 DONE entries with no branch (VPS operations, logged only in CALL-RECORD): 172a hermes-legacy-archive
(line 457, 08:25:44), tunnel-fix-502 (line 466, 08:27:15, no change), t175 cloudflared ingress (line 482, 08:33:23).
Earlier 180 sandbox step: swarm/nerve-eventlog-deploy 364d266 (line 532, 09:33:37), replaced by 0ddce9b.
Not done yet at 10:15 (+02:00): slots 170, 172, 173, 182-185 and this slot 171.

## C. Spot check: 5 random rows re-checked (2026-10-21 10:15-10:19 +02:00)

Picked with `shuf -n 5 --random-source=<(yes 171)` over all 122 rows of tables A and B. For each row: the SHA
resolves in the slot's worktree and equals the `swarm/*` branch head there; the cited `file:line` at that SHA holds
the number; the CALL-RECORD line matches. Then the result was measured again on the box or read again on the VPS
(read-only). Rebuilds used exported trees (`git archive` into /tmp), never the slot's worktree.

| Row | SHA / line check | Measured again | Same result? |
|---|---|---|---|
| B 107 swarm/cortex-t212-edgerate 9a48217 | branch head = 9a48217; line 31 = `1 passed`; CR 196 ok | `GALION_REAL=1 cargo-slot test --release --locked -p openmind-nerve --test real_cortex_sensor` at 9a48217, 10:16:52: replay fixture 21 F, replay1 61 D, serve :19430 6 sensors incl. edge-rate, `1 passed; 0 failed ... 11.05s` | yes, identical |
| B 118 swarm/vps-tuner-dns f67f37f | branch head = f67f37f; line 53 = `:8650/health -> 200`; CR 209 ok | VPS read-only 10:16:39: `:8650/health` 200, tuner active; Cloudflare DNS-over-HTTPS for tuner.galion.app = Status 3 (NXDOMAIN), one.galion.app = Status 0 | yes: DNS record still missing (owner D118-1 still open) |
| B 108 swarm/cortex-score-v2 7863e3d | branch head = 7863e3d; line 14 = `score 21 grade "F"`; CR 200 ok | release `openmind-nerve replay` of the cortex-replay fixture at 7863e3d, 10:17:22: score 21, grade F, pass false, ddos firing true, exit 1 | yes |
| A 14 desk-linkwatch (a54c792) | on 42de672: line 11 = `Result: **PASS**`, lines 78-79 = 1 + 3 passed | VPS read-only 10:16:39: `:8646/health` 200; galion-desk, link-watch-rs, tuner all `active`; 4 of 4 expected listeners (8646, 8650, 8091, 8642) present | yes (the record's VPS facts still hold) |
| B 111 swarm/integration-verify abd4dea | branch head = abd4dea; line 23 = row 1 PASS, line 80 = `Real tests: 7/7 PASS`; CR 204 ok | row 7 re-run at abd4dea: `GALION_REAL=1 cargo-slot test --release --locked -p gaia --test real_cli`, 10:17:35: `2 passed; 0 failed`, dry-run 52/52/5 refused/0 acted (also confirms row A 18); then with `GAIA_VPS` 10:18:05: live view 18 units seen, 14 running, 52 systems, 10 live, unmapped listener 8672 (galion-nerve-shadow) | yes |

Nerve during the checks (VPS `127.0.0.1:8670/state`, read-only, 10:12:54): cycle 59, mode live, started 09:43:54,
9 healthy / 3 degrading / 1 failing-soon / 2 absent, score 51 F, 5 open incidents (top: tunnel routes 11 public
hostnames to 10 closed origin ports). `/cortex` answered 200.

## D. Notes and gaps

- **sandbox.md** (row A 38) is a box sandbox run from 2026-10-20 ~19:1x, before the sandbox_vps_only rule
  (2026-10-21 04:29). Current sandbox evidence is VPS-only: rows B 150, 151, 151b, 144, 164, 165, 176.
- **hermes-tools.md:55** shows `test result: FAILED`. That is the planned red step of the fake ticket (the agent's
  first `cargo test` before its fix), not a failing suite; the green run is line 61 and the real test line 133.
- **Scripts vs Rust**: rows A 9, 14-17, 19-20, 22, 25-26, 28-36, 38-42 were first run as `scripts/real/*.sh`. The
  ports to Rust `GALION_REAL=1` tests are on swarm/realscript-port-1, -2, -3 (rows B 112, 113, 114), not yet on
  integration-4, so `scripts/real/` still exists here.
- **nerve.md / cortex-sensor.md** FAIL scores (61/100 D) are expected: they grade a real VPS capture, not the code.
- **Push status**: rows marked LOCAL (B 143, 144, 164, 165 in the separate galion-os repo; 176, 181, 174) exist only
  as commits in `/workspace/swarm/wt/<slug>` or `/workspace/swarm/galion-os-repo`.
- Wave-4 records are not in this folder until those branches are merged into an integration branch. When they are,
  move the row from table B to table A and point the evidence at this folder.
- The box resolver answers every name with 198.18.0.1 (a local stand-in), so DNS facts must be checked through
  DNS-over-HTTPS or from the VPS, as in row B 118 above.
