<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/CALL-RECAP.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# PROJECT 42 / Gaia swarm: call recap for Slavik
Written 2026-10-20 ~20:45 CEST (+02:00), read-only. Sources: CALL-RECORD.md, RULES.md, WAVE2.md, /workspace/review-20261020/*.md, docs/ in each /workspace/swarm/wt/<slug>, `git log -3`.
Push status: the clones' `origin` is /workspace/swarm/base.git, which has no swarm/* refs, so **pushed/local could not be verified from the box**. The "Pushed" column below repeats WAVE2.md:77-79 and CALL-RECORD.md:9 (marked "per record"). "wt dirty" = `git status --porcelain` line count now.
Docs copied into integration-1 are byte-identical to each branch's own docs (checked with cmp).

## 1. Branch table
| slug | head | pushed (per record) | what it does | tests / real-life result (as recorded) | new deps (licence) | open items |
|---|---|---|---|---|---|---|
| docs-sync | 80e28f3 | yes (wave 1) | main.md/goals.md, SoT notes, STATUS.md, repo-check utc warnings 43->0 | workspace 497 passed, 0 failed; real/docs-sync PASS (docs/real-tests/docs-sync.md:41,51,64) | none | sandbox `group_kill_leaves_no_grandchild` failed once (docs-sync.md:73) |
| ucpu | 3e70ffb | yes | universal-CPU gate, SQLite trim, armv7 seccomp fix | real 20 passed, 0 failed on aarch64/armv7 under qemu (ucpu.md:16) | none; tools zig 0.15.2 (MIT), qemu-user-static (GPL-2.0, tool only); ARM syscall numbers from kernel table (GPL-2.0 WITH syscall-note) (third-party/ucpu.md:12-20) | must merge before edge-boot |
| one-probe (= one-hardening, same SHA) | b44cd4f | yes | galion-one fail-closed pairing, ONE-PROBE, /api/v1/services | real fail-closed + live GET compare OK; Rust real test 1 passed (one-probe.md:144) | none | cutover is owner's |
| tuner-cutover | 5fd1611 | yes | tuner --version/--check, live golden fixtures, cutover plan | 21/21 routes identical vs live 8650; 3 real tests; workspace 506 passed (tuner-cutover.md:46,61,82) | none | cutover; merge before common |
| room | 5bb27e1 | yes | room lib split, RoomError, real script, cutover plan | unit 30+5+2+2 ok; real script ok (room.md:45-83) | none; test tools strace (LGPL-2.1+), util-linux (GPL-2.0+) (third-party/room.md:22-23) | no live room to compare |
| sandbox (= flake-fix, same SHA) | d194c19 | sandbox yes; flake-fix local | fixes caps/net flake (EMFILE root cause), per-run cgroup, escape test | 25x stress: 24 passed (sandbox.md:85); escape 7/7 blocked (hermes-tools.md:104) | none (libc MIT/Apache) | privileged test on VPS host not done (sandbox.md:203); flake-fix has 15 dirty files |
| hermes-tools | 8ccdfcc | yes | sandboxed fs_read/fs_write/git/cargo tools for hermes-rs (includes sandbox) | real run all PASS, 7 escape attempts blocked (hermes-tools.md:46-112) | none | spend cap overrun 1.20 vs 1.00 USD (hermes-tools.md:120) |
| hades-approval | ccaab42 | yes | hades-gate Ed25519 single-use scoped admin tokens replace "approved-test" | real script all PASS (replay, tamper, forged, scope, expiry); Rust real 1 passed (hades-approval.md:8-64) | ed25519-dalek 2.2.0 (BSD-3), sha2 0.10.9, zeroize 1.9.1 (MIT/Apache) | `.cargo/config.toml` rustflags (integration-1.md Needs #2); SIMD exceptions |
| survival-heal | 45a18aa | yes | guardian/healer repair path onto openmind-survival + guardian-tick | box heal capped 3/3 per hour; live VPS classify dry-run only (survival-heal.md:43-137) | none (ported own code 317d1d4) | WS-SURV not live (no unit installed) |
| gaia-live | 4fbbb7f | yes | ws/gaia merge + gaia heal-tick + nerve serve, first end-to-end Gaia path (box only) | script exit 0, Rust real 1 passed; workspace 593 passed (gaia-live.md:6-7,144) | none | nerve main.rs superseded by nerve branch |
| nerve (= nerve-wiring, same SHA) | 5802d0e | yes | openmind-nerve: cortex, incidents, score, DDoS, replay | replay of live capture = FAIL 61/100 (D) while live nerve says PASS (nerve.md:47,61) | none | tunnel ingress/DNS + mouth restart need owner (nerve.md:80,89) |
| memory-real | 723b46c | yes | openmind-memory real suite, pre-v3 migration fix, filter-before-limit | real 7 passed; live 8420 GET contract matches (memory-real.md:37-53) | none | shared schema change = owner decision (memory-real.md:100-101) |
| desk-linkwatch | a54c792 | yes | galion-desk, desk-hermes, link-watch tests + desk cutover doc | real run PASS; desk 38 + 3 gated, desk-hermes 12 (desk-linkwatch.md:11,144-145) | none (rusqlite feature trim, curl runtime tool) | desk cutover |
| voice-mouth | 5d776c1 | yes | honest :8002 /health, galion-voice-serve CPU ear+mouth, 503 root cause | real JFK STT + TTS runs; builds ok (voice-mouth.md:22-91) | via vps/ws-voice: candle 0.9.2, num-traits (MIT/Apache), opus-pure 0.2.2 (BSD-3) | live mouth still 503 |
| lan-discovery | fe5f0bd | yes | mDNS/DNS-SD find+announce, opt-in /24 probe, approve/deny/join | real run OK on 127.0.0.0/24 only (lan-discovery.md:31-91) | socket2 0.6.5 (MIT/Apache) | LAN mDNS on real LAN not done (lan-discovery.md:121) |
| poseidon | 7d0c478 | yes | openmind-queue: durable queue, quotas, hard-stop spend caps, VRAM routing | real script PASS lines + gated test pass (poseidon.md:28-55) | none | no GPU/LAN endpoint (poseidon.md:32); RunPod spend = owner |
| real-harness | 5b3c662 | yes | galion-realtest platform harness, 12 scenarios, live probe, CI | 77 checks: 66 pass, 10 known-fail, 1 skip; live 43: 41 pass, 2 known-fail (real-harness.md:19,43) | none | galion-one fail-open (fixed by one-probe); mouth 503 |
| edge-boot | 753676e | yes | omnode first boot, fail-closed pairing, seccomp gate | real 84 passed, 0 failed (edge-boot.md:15) | none new (dalek BSD-3 etc. existing) | aarch64 hardware enforcement unproven (edge-boot.md:43); needs ucpu first |
| launch-gates | 79ee761 | yes | public-export gates, SCORECARD, GATES-OCT31, REUSE | 9 PASS + gate 10 WARN; workspace 497 passed (launch-gates.md:19-28,108) | tools: comply 0.1.1, gitleaks, cargo-deny, cyclonedx (MIT/Apache) | gate 10 IPv4/tunnel disclosure: owner decides |
| common (= common-migrate-2, same SHA) | 26d98b0 | common yes; c-m-2 unclear | galion-common std-only sha256/hex/base64/json/time/http; migrate 9 crates | tuner/origin stress compare table (common.md:99-105) | none | touches tuner: merge after tuner-cutover; common-migrate-2 has 1 dirty file |
| gaia-poseidon | 498e3d2 | yes (CALL-RECORD Q2 implies) | gaia-core POSEIDON forwards to openmind-queue (never spends) | script RESULT pass=60 fail=0 + gated test ok (gaia-poseidon.md:27) | none | POSEIDON vs HEPHAESTUS |
| gaia-hades-tokens | f3f41a2 | yes (implied) | gaia heal-tick uses hades-gate tokens instead of random test token | real 1 passed; hermetic heal_tick 10 passed (gaia-hades-tokens.md:14,17) | none new (dalek BSD-3 via hades-gate) | - |
| integration-1 | 8fd26f6 | local | all 20 wave-1 branches merged + fixups | workspace 950 passed, 0 failed, 4 ignored; omnode 72 passed; ucpu-gate ok; public-export 0 FAIL 1 WARN (integration-1.md real-tests:19-23) | none (Apache-2.0 licence text added) | 8 "Needs Slavik" items (integration/integration-1.md:143-160) |
| gaia-land | c15a4b7 | local, 24 dirty files | merge ws/gaia (04778e6): gaia-core, 9 function crates, CLI; ownership registry | no real-tests doc found | none | overlaps gaia-live; not in integration-1 |
| fuzz-sast | bb89351 | local (merge only) | merge of common + lan-discovery; no own commit yet | none own | - | agent killed? (CALL-RECORD:7) uncertain |
| hermes-enroll | 92f7693 | local (merge only) | merge of ucpu + edge-boot | none own | - | uncertain |
| lan-routing | b153af8 | local (merge only) | merge of lan-discovery + poseidon | none own | - | uncertain |
| console-static | 13a296c | local, 2 dirty | no commit yet | none | - | in progress (CALL-RECORD:7) |
| sec-runbooks | 13a296c | local, 1 dirty | no commit yet | none | - | in progress |
| deploy-sim | 13a296c (branch main) | n/a | no swarm branch created | none | - | - |

## 2. VPS state (CALL-RECORD.md:10, 11, 20)
- Working copy unchanged at 2f5a730 + 17 uncommitted, 13 units running, load 6.3, binaries Oct 10-18 builds, v0.1.0, no tags. link-watch-rs IS running (old link-watch unit inactive).
- Live galion-nerve (8670) is the old Oct 18 build; it reports PASS while the new nerve replay gives FAIL 61/D; ~59% of tunnel requests fail.
- Nothing from the swarm is deployed (integration-1.md Needs #8). Box /workspace/oom-target deleted (disclosed, CALL-RECORD:20).

## 3. Needs Slavik (deduplicated)
1. POSEIDON vs HEPHAESTUS ownership of compute (CALL-RECORD:16, Q7).
2. Merge order / merges: ucpu before edge-boot; tuner-cutover before common; nerve's main.rs over gaia-live; merge integration-1 (no PR opened); gaia-land after, expect overlap.
3. `.cargo/config.toml` `[build] rustflags` (dalek serial backend; any RUSTFLAGS env breaks simd_free test) and SIMD exceptions.
4. SEC-1..5 and service cutovers (tuner, desk, one, room, memory).
5. OpenRouter 402 top-up; any GPU/RunPod spend (POS-2, 1500 GB VRAM target).
6. galion-health / galion-nerve live binaries without source in main.
7. Public-export gate 10: 20 public IPv4 literals + 1 quick-tunnel hostname.
8. Cloudflare ingress/DNS for 13 hostnames on 10 closed ports; restart of galion-one-mouth (nerve.md:80,89).
9. openmind-memory shared schema change (memory-real.md:100).
10. hermes-tools spend cap overrun handling (1.20 vs 1.00 USD).
11. Turn on `--with-nerve` in realtest CI (81/81 local); WS-SURV marked D; 28 utc warnings.
12. Use of GPL/LGPL tools in tests (strace, util-linux, qemu) and GPL-2.0-with-syscall-note constants in ucpu (tools only, not linked) - confirm acceptable.

## 4. Contradictions and problems
1. **gaia-poseidon / gaia-hades-tokens test counts:** CALL-RECORD.md:8 says "639 tests" and "627 tests"; no file found contains 639 or 627. gaia-poseidon.md:27 records 60/60 script + 1 Rust test; gaia-hades-tokens.md:14,17 records 1 real + 10 heal_tick. Unverified.
2. **integration-1 SHA:** CALL-RECORD.md:7 says "881de9d local" and :8 "not yet tested"; actual head is 8fd26f6 (20:32:58, after the 20:31 answer) with a test record showing 950 passed (docs/real-tests/integration-1.md:19). Record is stale.
3. **Pushed count:** CALL-RECORD.md:9 says 23 swarm branches pushed; 33 swarm branches exist locally, 7 listed local-only -> 26 remain, and WAVE2.md:77 lists 20 wave-1. Duplicates (nerve-wiring=nerve, one-hardening=one-probe, common-migrate-2=common, flake-fix=sandbox SHA) and empty console-static/sec-runbooks make 23 unverifiable; base.git has no swarm refs.
4. **flake-fix "local (14)":** CALL-RECORD.md:9 says 14 files; `git status` shows 15 lines now. Its committed head d194c19 equals the pushed sandbox head, so "local" means only uncommitted work.
5. **Running units:** CALL-RECORD.md:10 says 13 units running; LIVE.md:39 (~17:50) lists 11 Rust/other units + console.service = 12. Possibly a later change or counting difference; uncertain.
6. **"17 uncommitted":** CALL-RECORD.md:10; LIVE.md:10 says VPS main has "17 on Oct 20" commits and LIVE.md:36 lists untracked dist/bin-ws/, stage/, 15 *.bak files. No file confirms 17 uncommitted files. Uncertain.
7. **Nerve source:** CALL-RECORD.md:11 "source not in main" and :19 "no source in repo"; LIVE.md:49,64 says galion-nerve was built from ws/nerve, pushed to GitHub (a63b730). It is in the repo on a branch, just not in main.
8. **Which nerve "passes":** live galion-nerve verdict PASS vs new openmind-nerve replay FAIL 61/D on the same moment (nerve.md:47 vs :61); LIVE.md:59 already showed the 8670 dashboard as "system degrading".
9. **POSEIDON vs HEPHAESTUS ownership:** gaia-land docs/gaia-ownership.md:15-37 moves cgroup-detect -> POSEIDON quotas and openmind-exo/llama/provider/tulu -> POSEIDON vram-pool; the VPS docs (review-20261020/session-evidence/docs-oct19-20.vps.md:978,996-1005) assign cgroup-detect, openmind-provider, openmind-route, openmind-sandbox etc. to HEPHAESTUS; SESSION.md:93 lists nine functions with ARES and no POSEIDON/AETHER; PLAN-DIGEST.md:110 notes the same discrepancy. gaia-poseidon.md:32 implements POSEIDON ownership already, before the decision.
10. **Systems count:** gaia-land commit and ownership.md say 52 systems; PROGRESS.md:8 notes design docs elsewhere said 51.
11. **Public IPv4 literals:** launch-gates.md:28 says 7; integration-1.md:22 and integration/integration-1.md:45 say 20 (explained there: new nerve/lan-discovery fixtures).
12. **repo-check utc warnings:** docs-sync.md:41 reached 0; integration-1 real-tests:23 shows 28 again (explained in integration/integration-1.md:48-51).
13. **REUSE counts inside launch-gates.md:** 723 total/683 passed (:25,68) vs 812 total/771 passed (:100) - likely different heads (export vs tree); not explained clearly. Uncertain.
14. **omnode test counts:** edge-boot.md:15 "84 passed" (real-life checks) vs integration-1 real-tests:20 "72 passed" (cargo test of edge/openmind-node). Different measures, but easy to confuse.
15. **Workspace test totals differ by branch** (docs-sync/launch-gates 497, tuner-cutover 506, gaia-live 593, integration-1 950) - expected per branch, not an error; CALL-RECORD.md:8 "wave-1 20 branches green at push" has no single per-branch count file.
16. **gaia-land status:** WAVE2.md:79 and CALL-RECORD.md:13 say "running"; head c15a4b7 is from 18:52 with 24 dirty files and no real-tests doc. CALL-RECORD.md:7 says 11 of 15 wave-2 agents were killed ~20:14; which ones is not recorded. fuzz-sast, hermes-enroll, lan-routing have only merge commits.
17. **Sandbox flake still seen:** docs-sync.md:73 reports a sandbox test failure in a workspace run, and sandbox.md:85 shows 24/25 in a stress run, while the sandbox branch claims the flake root cause fixed.
18. **VPS IP in docs:** survival-heal.md:21 (also copied to integration-1) contains the VPS public IP in plain text, while the 859ddd0 commit redacted the VPS IP in stack/ snapshots. Not a secret, but inconsistent with that policy and relevant to gate 10.
19. **Mouth 8002:** voice-mouth claims an "honest :8002 /health" on the branch; live still 503 "loading chatterbox" (real-harness.md:48, nerve.md:70). Branch fix is not deployed.
20. **Licence rule vs tools:** RULES.md:38 forbids GPL/LGPL; room uses strace (LGPL) and util-linux (GPL) in tests, ucpu uses qemu (GPL) and kernel syscall numbers (GPL-2.0 WITH syscall-note). Docs say tools only / constants; needs a ruling.
