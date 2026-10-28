<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Fact-check of the white-paper evidence (docs S4: T4.1 claim check + T4.2 pre-run/during-run labels)

Checker: slot 197 (whitepaper-factcheck), 2026-10-21 16:36-16:50 CEST (+02:00). Independent of the writers (slots 46 and 140); they were not consulted.
Base: swarm/whitepaper-update cc00dc4. All times Europe/Zagreb, CEST +02:00. CR:n = /workspace/swarm/CALL-RECORD.md line n as of 16:36. Every cited line was re-read; no line shift was found (the file is appended to, except the wave-3 table rows CR:120-134, which were edited after 22:52).
Re-run evidence (commands and outputs): docs/real-tests/whitepaper-factcheck.md. file:line references are to the base cc00dc4 versions (this commit inserts one line in RES and LED).

## 0. Scope, and what does not exist yet
- The S3 draft `docs/whitepaper/2026-10-20-full-capacity.md` **does not exist** on any branch in base.git or /workspace/swarm/wt/* (checked 16:45). This check covers the three evidence files that do: `docs/whitepaper/results-2026-10-21.md` (RES), `docs/whitepaper/concurrency.md` (CON), `evidence/fullcap/ledger.md` (LED), and the slot-140 record `docs/real-tests/whitepaper-update.md` (RT).
- Every number, time and SHA in the four files was checked. Every commit SHA was resolved, every CR:n citation was read, every live number was re-probed on the VPS (read-only) at 16:38, and the HADES and benchmark numbers were re-run on the box at 16:38-16:42.
- Verdicts: 16 claim groups confirmed by re-run or live re-probe (§3); the rest confirmed against the cited source (§4); **12 disagreements** (D1-D12, 4 substantive: D1-D4); **7 stale** (true when written, superseded now: S1-S7). **Unsourced numbers: 0** (every number in the four files has a CR line, a file:line or a command; after this commit that includes the corrected ones).

## 1. Disagreement list (each one is fixed in this commit unless marked "owner")
| id | file:line | claim as written | what the source / re-run shows | fix applied |
|---|---|---|---|---|
| **D1** | CON:8, :39 | "Measured real limit: about 18-20 agents running at once" | The record does not show a stable count limit. Every recorded stop follows a **burst of new starts**: 20:38 (7 planners added at once, 22 -> 18, CR:41-43); 00:54-00:55 (20 launched, **all 20 stopped**, 0 running, CR:184); 06:58-06:59 (9 stopped "within ~1 min of start", 18 -> 9, CR:367); 08:55-08:56 (11 started at once, 16 -> 8, CR:514-516). By contrast the ledger reconstruction shows about 23 running at 20:42-20:44 with no stop (D3). | Headline reworded: stops are tied to start bursts; "18-20" kept only as the count at which the 20:38 stops happened. **Owner**: the paper's headline number needs Slavik's OK (or the stepped test M2). |
| **D2** | CON:33 | "No limit stops are recorded after 20:38 (CR:49-134)" | True for the record as it stood at 22:52. CR:123-124 now show 2 stops for 38-cutover-prep and 1 for 39-swarm-record; CR:177 (23:42), :184 (00:55, 20 stops), :237, :287-288, :367, :515-516 record many more. | Scoped to "up to 22:52" and the later stops listed. |
| **D3** | CON:24 (and CR:65) | "20:42 ... 20 (13 coding + 7 planners)" | The 4 agents dispatched at ~20:41 (settings-snapshot, nonrust-inventory, voice-rust, guard-rust; CR:45, :70; LED:42-45) are in no running count. With them the 20:42-20:44 count is ~23 (24 if settings-snapshot was still running after its 20:40:10 commit 685f57f; flake-fix ended 20:44, nonrust-inventory 20:46), and the "15 coding" at 20:38 (CR:41) cannot be rebuilt from the ledger (13 coding agents are listed). | Note added; count shown as "20 recorded, ~23 by ledger". |
| **D4** | RES:61 | "a rebuild from the branch is a provable no-op swap" | The source it cites (docs/real-tests/health-land.md:16-18 on f4a66ea) shows the **e43d7ab tree** rebuilds byte-identical (6afe668c...), but a rebuild of **the health-land branch itself** gives a different binary (sha256 6356b6b6..., same size 503512 B). Only a rebuild of e43d7ab is a no-op swap. | Sentence corrected. |
| D5 | CON:8 | quote "Real concurrency limit: ~18-20" cited as CR:80 | The quote is on CR:79 (CR:80 is the vps-ops retry row). | CR:80 -> CR:79. |
| D6 | CON:19 | "7 running + 8 restarted ... 16" | CR:27 says "7 + recap": 7 coding agents plus the recap agent = 8 running; 7 + 8 = 15, not 16. | Reworded "7 coding + recap (8) + 8 restarted = 16". |
| D7 | RES:48 | cloudflared "59 % at ~03:20" (1c40c5e) | docs/ops/steady-traffic.md on 1c40c5e: measured 02:55-03:08, checked live 03:09; 55858/94031 is a **cumulative counter since cloudflared started**, not a 10-minute window like the nerve's 251/401. | Time and measure type corrected. |
| D8 | RES:6, RT:8 | "38 short SHAs ... resolved" | RES + RT cite **32 distinct commits** (31 in RES; 8421bbc only in RT) in 62 mentions; c4b11d2642a0b6be and e3f5e643e7750856 are output hashes, not commits. All 32 resolve (re-checked 16:37). | Count corrected. |
| D9 | LED:15 | sec-runbooks "restarted 21:03" | CR:88: interrupted after cecc081 and restarted at ~20:54; 21:03 is the DONE time (CR:95). The retry-ledger column "Latest start" holds the completion time on completed rows (also CR:84, :86, :100, :103, :107). | Start -> "~20:54". |
| D10 | LED:44 | voice-rust "restart 21:41" | CR:102: interrupted and restarted at 21:33; 21:41 is the DONE time (CR:105). | Start -> "21:33". |
| D11 | LED:17 | common-migrate-2 "follow-up 22:06" as a start | CR:97: follow-up started 21:09; 22:06 is the DONE time (CR:108). | Start -> "follow-up 21:09". |
| D12 | LED:42 | settings-snapshot start "~20:41" | Its first commit 685f57f is 2026-10-20 20:40:10 +02:00, so it started before 20:40:10. | Start -> "before 20:40:10". |

Smaller wording issue (no number wrong): RES:32 "standard policy 94.00 % / mean 75.72 %" omits "on hard seed 42 / hard seeds 1-200" (CR:313); added.

## 2. Stale since writing (true at the time, superseded now)
| id | file:line | written | now (16:38 +02:00, read-only) | fix |
|---|---|---|---|---|
| S1 | RES:39, :52, RT:34 | "no cortex is deployed"; `/state` has no cortex field | The nerve binary was replaced at 09:43:54 (openmind-nerve aa77864, CR:541); `/state` now carries a `cortex` object (vulnerabilities, bleeding, incidents, ddos, exposure). No separate cortex service exists. | "as of 06:35" added, current state noted. |
| S2 | RES:52 | nerve 06:35: started 2026-10-18 22:35:04, cycle 6722, 11 healthy / 2 degrading / 2 absent | started 2026-10-21 09:43:54, cycle 829, 11 healthy / 1 degrading (galion-one-mouth 503, "stuck loading for 6.9 h") / **1 failing-soon (cloudflared)** / 2 absent; score 61/D, 3 open incidents | dated + current line added |
| S3 | RES:48 | cloudflared degrading, 251/401 errors in 10 min | cloudflared **failing-soon**: 18 public hostnames routed to 7 closed origin ports, 132.0 failed requests/min over 29 min | noted |
| S4 | LED:14, :19, :21, :24 | fuzz-sast, nerve-wiring, hermes-enroll, tunnel-audit "still running at 22:52" | the original agents stalled; their work was finished by successor slots 100-103 at 00:59-01:02 (CR:187-190) | noted |
| S5 | LED:52 | wave 3 "0 stops, 0 retries" | CR:123 38-cutover-prep 2/2, CR:124 39-swarm-record 1/1 | noted |
| S6 | CON:32 | "none was given up" | still true for the work; the 4 agents in S4 were replaced, not resumed | noted |
| S7 | RES:8 | "main is still 13a296c" | still 13a296c in base.git and in the record; not re-checked on GitHub (no read access from the box) | none needed |

## 3. Confirmed (re-run or re-probed by this checker)
| claim | file:line | re-run result (16:38-16:42) |
|---|---|---|
| HADES before fix 48/82 (shell 16, file 12, access 6, allowverb 9, unicode 0, traversal 5), legit refused 0/9 | RES:14, RT:14 | ccaab42 + e6ceca7 corpus/example: identical numbers |
| HADES after fix 0/82 in-process, legit 0/9 | RES:15 | e6ceca7 redteam_enum: `TOTAL WouldApply: 0 / 82`, `legit refused: 0 / 9` |
| 0/82 over real HTTP | RES:15 | real_redteam on 127.0.0.1:20671: `REAL TOTAL WouldApply: 0 / 82`, `REAL legit not approved: 0 / 9` |
| hades-gate 45 passed / 0 failed (release, GALION_REAL=1) | RES:16 | 17+12+5+5+1+1+3+1 = 45 passed, 0 failed |
| families 25/19/14/10/6/8 = 82 | RES:13 | same per-family attempt counts |
| ccaab42 is the right "before" (8421bbc parent, empty src diff) | RT:15 | e6ceca7^ = 8421bbc; `git diff ccaab42 8421bbc -- crates/hades-gate/src` empty |
| benchmarks via Hermes scripted: banking 100 % (114 checks, forbidden 0), wallstreet pnl 130894 + fingerprint, vending 30 shifts / 443 calls / 10000 bp / hash c4b11d2642a0b6be | RES:27, RT:20-25 | identical output lines at 457ef74 |
| same with mock LLM | RES:28 | identical (mock on 127.0.0.1:20672) |
| B3 hard: 200/200 seeds 1-200, 999/1000 seeds 1-1000, seed 909 = 99.93 %, standard policy 94.00 % seed 42, mean 75.72 % seeds 1-200 | RES:32 | 1b0a30c galion-vending: 200, 999 (909 = 9993 bp), 9400 bp, mean 7572 bp, 82 capped at 5000 bp |
| zip 7.4.0 in 03d21a9 lock, 9.0.5 on a4f49b5, none in 13a296c | RES:45 | `git show <c>:Cargo.lock`: same |
| live galion-health 503512 B, sha256 6afe668c..., mtime 2026-10-18 22:14:38 +02:00, active, /status.json 200 1394 B | RES:57, RT:32-33 | same at 16:38:12 |
| galion.app cert notAfter 2026-10-31 18:28:08 +01:00 | RES:42 | same |
| sshd passwordauthentication no, ssh active | RES:44 | same |
| tuner.galion.app does not resolve on the VPS | RES:43 | same (getent: no answer) |
| HADES `no-live-systems` in the live nerve | RES:22 | same at 16:38:31 |
| all cited commit SHAs exist; ledger head commit times | RES, RT, LED | all 32 + 30 LED SHAs resolve; every LED commit time matches `git show -s --format=%ci` |

## 4. Confirmed against the cited source (not re-run)
RES:17-20 (CR:159, :240, :241), :29-31 (CR:170, :302, :300), :33-35 (CR:262, :238, :315), :46-47 (CR:173, :172, :194), :49-50 (CR:191, :196, :200, :195), :58 (health-land.md:17: zig 0.13.0 cc, rust-lld, rustc 1.99.1), :59 (CR:221, :229, :96), :64 (CR:159, :179, :199, :173-175, :200); CON:9-11, :18-31 (except D3/D6), :36-41; LED rows not listed in D9-D12, the three LED spot-checks (LED:76-78). Not re-run because they need long builds or the VPS sandbox: galion-health rebuild, adversarial B1/B2 sets, cortex S2 goldens, backup/tar parity, Option B typed redteam.

## 5. T4.2: pre-run vs during-run labels
Phases (from CR:25-29, :112, :184; the white-paper cut-off M9 was never set):
- **P0 pre-run**: before 2026-10-20 20:34.
- **P1 full-capacity run (recorded section)**: 20:34 -> 22:52.
- **P2 wave 3**: 22:52 -> 00:54. **P3 wave 4**: from 00:54.
- **R140** = slot-140 re-checks 2026-10-21 06:35-06:39; **R197** = this check 16:38-16:42.

Findings:
1. **RES has no phase labels and no per-row times**; its title "night of 2026-10-20/21" mixes P0 baselines with P2/P3 results. Nothing in RES is a result *of* the 20:34 full-capacity run except the P1 items below. Fix: the table below (time = commit time, and DONE time where recorded) and a pointer in RES.
2. **LED puts two P0 results in the "Full-capacity run" table without a label**: integration-1 (LED:13; commit 8fd26f6 20:32:58, built 20:15-20:35) and the recap agent (LED:26, ran before 20:34). LED:70 lists gaia-poseidon 498e3d2 (20:25:35) and gaia-hades-tokens f3f41a2 (20:25:54) without saying they are pre-run. Fix: "(P0 pre-run)" labels added.
3. **CON timeline rows ~20:14, 20:26, 20:31 are P0** and the 22:52 row is P2; labels added. The "11 of 15 killed ~20:14" fact (CON:16) is a P0 fact and must not be counted as a stop during the run.
4. The hermes-core "cap 24 vs limit 18" test (CON:11) uses the 18 from D1 as an input; it is a simulation, not a measurement of the platform.

| RES row | commit time (+02:00) | DONE (CR) | phase |
|---|---|---|---|
| HADES before (ccaab42) | 2026-10-20 19:06:28 | - | P0 (baseline) |
| HADES after, redteam e6ceca7 | 2026-10-20 23:02:56 | 23:04 (CR:159) | P2; R140; R197 |
| Option B 0f41f29 | 2026-10-21 04:54:20 | 04:55 (CR:240) | P3 |
| Ledger failure 84fef87 | 2026-10-21 04:51:45 | 04:53 (CR:241) | P3 |
| benchmarks via Hermes 457ef74 | 2026-10-20 23:47:55 | 23:49 (CR:179) | P2; R140; R197 |
| scoreboard f2ff169 | 2026-10-20 23:19:14 | 23:19 (CR:170) | P2 |
| B1 adversarial 44e5c61 | 2026-10-21 05:20:23 | 05:20 (CR:302) | P3 |
| B2 adversarial 2ecfc19 | 2026-10-21 05:16:22 | 05:16 (CR:300) | P3 |
| B3 hard 1b0a30c | 2026-10-21 05:45:27 | 05:45 (CR:313) | P3; R197 |
| vending white paper 5ddd828 | 2026-10-21 04:34:08 | 04:34 (CR:262) | P3 |
| gateway bench 20d9a2f | 2026-10-21 04:52:46 | 04:53 (CR:238) | P3 |
| bench sector 12be83b | 2026-10-21 05:49:03 | 05:49 (CR:315) | P3 |
| cert / tuner / sshd (dbec5c9, 9557eb3) | 2026-10-20 23:27:49 / 23:26:24 | 23:30 / 23:28 (CR:175, :174) | P2; live R140, R197 |
| zip finding 9557eb3, fix a4f49b5 | 23:26:24 / 2026-10-21 01:45:11 | 23:28 / ~01:50 (CR:174, :199) | P2 / P3 |
| B41 conn-rate 51100e1 | 2026-10-20 23:26:10 | 23:27 (CR:173) | P2 |
| state titles f3468ce, fix 967459d | 23:24:24 / 2026-10-21 01:14:23 | 23:26 / ~01:15 (CR:172, :194) | P2 / P3 |
| tunnel errors 1c40c5e | 2026-10-21 03:08:21 | measured 02:55-03:09 | P3; live R140, R197 |
| S2 merge ee75f75 | 2026-10-21 01:06:08 | 01:06 (CR:191) | P3 |
| edge rate 9a48217, score v2 7863e3d, ingest 7a55e6c | 01:23:21, 01:40:50, 01:21:52 | ~01:23, ~01:55, ~01:19 | P3 |
| galion-health live binary | built 2026-10-18 (e43d7ab 22:15:08), active since 2026-10-18 22:14:38 | - | before P0 (artefact) |
| byte-identical rebuild f4a66ea | 2026-10-20 23:08:02 | 22:52 table (CR:134) | P2 |
| backup parity 5c3a8ed, tar 820740e | 2026-10-21 03:54:59, 04:15:50 | ~03:53, ~04:20 | P3 |
| migrate 88+10 identical, cf40c1b | 2026-10-20 21:04:33 | 21:05 (CR:96) | **P1** |

## 6. Decisions for Slavik (the checker does not decide)
- **DF-1 (D1) concurrency headline.** (a) keep "18-20" with the burst caveat (this commit's wording); (b) replace it with "stops follow start bursts; steady 16-20 ran without stops" until M2 is measured; (c) run the stepped test M2 (20 min per level, costs requests). Recommendation: (b) now, (c) only if the paper must state a number.
- **DF-2 (T4.2) scope of the paper.** Is the paper about the 20:34 run only (P1, with P0 as context) or the whole 2026-10-20/21 swarm (P0-P3)? RES today is mostly P2/P3. Set the cut-off M9 either way.
- **DF-3 (S1) wording of "cortex deployed".** Since 09:43 the live nerve carries cortex data in /state without a cortex service. Say "cortex logic live inside the nerve" or keep "not deployed" until a cortex unit exists?
