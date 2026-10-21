<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Agent ledger, swarm of 2026-10-20 (Europe/Zagreb, CEST +02:00)

Sources: /workspace/swarm/CALL-RECORD.md (CR:line; results log, retry ledgers CR:49-65, wave-3 table CR:120-134) and a box snapshot taken 2026-10-20 22:51 +02:00:
`git -C /workspace/swarm/wt/<slug> log -1 --format='%h %ci'` and `git -C ... status --short | wc -l` (= dirty).
"Start" = latest (re)start recorded; earlier starts are not recorded unless stated. "End" = DONE line time in CALL-RECORD. Head = worktree HEAD at 22:51 (commit time in brackets). Stops = request-limit stops (or stall) from the retry ledgers.

## Full-capacity run, coding agents (CR:26-28)
| slot | slug | start | end | outcome | head (commit time) | dirty | stops/retries | source |
|---|---|---|---|---|---|---|---|---|
| 01 | gaia-land | before 20:34; resumed 20:44 | 20:52 | done, 9d3ec18 | 9d3ec18 (20:51:22) | 0 | 1 stall / 1 | CR:27, :73, :83-84 |
| 22 | integration-1 | before 20:34 | 20:34 | done, 8fd26f6 pushed | 8fd26f6 (20:32:58) | 0 | 0/0 | CR:27, :31 |
| 25 | fuzz-sast | 20:34 (restart) | not recorded | e78b2e7 committed, not on GitHub at 21:24; still running at 22:52 | e78b2e7 (21:05:50) | 0 | 1/1 | CR:28, :56, :98, :115 |
| 26 | sec-runbooks | 20:26; restarted 21:03 | 21:03 | done, c5fc269 | c5fc269 (21:02:50) | 0 | 1/1 (+1 interrupt 20:54) | CR:62, :88-89, :94 |
| 27 | flake-fix | before 20:34 | 20:44 | done, b2e69b9 pushed | b2e69b9 (20:41:04) | 0 | 0/0 | CR:27, :71 |
| 28 | common-migrate-2 | 20:26; follow-up 22:06 | 21:05; follow-up 22:06 | done, cf40c1b then 123eab2 | 123eab2 (21:55:28) | 0 | 1/1 + follow-up 1/1 | CR:64, :95, :97, :107-108 |
| 29 | one-hardening | 20:42 | 20:52 | done, fed3de4 | fed3de4 (20:51:43) | 0 | 2/2 | CR:28, :42, :55, :85-86 |
| 30 | nerve-wiring | 20:34 | not recorded | still running at 22:52 | 8d7a924 (21:08:04) | 0 | 1/1 | CR:28, :57, :115 |
| 31 | lan-routing | 20:34 | 20:48 | done, e8169c9 | e8169c9 (20:48:19) | 0 | 1/1 | CR:28, :58, :82 |
| 32 | hermes-enroll | 20:34 | not recorded | still running at 22:52 | f2121a8 (20:56:07) | 0 | 1/1 | CR:28, :59, :115 |
| 33 | deploy-sim | 20:42 | 21:26 | done, c5ab8f9 | c5ab8f9 (21:25:47) | 0 | 2/2 | CR:28, :42, :54, :99-100 |
| 34 | console-static | 20:26 | 20:47 | done, 6bd3580 | 6bd3580 (20:47:03) | 0 | 1/1 | CR:27, :63, :81 |
| 35 | tunnel-audit | 20:34 | not recorded | still running at 22:52 | d35ef73 (20:46:59) | 0 | 1/1 | CR:28, :60, :115 |
| 36 | memory-schema | 20:34 | 20:52 | done, 7952626 (on GitHub) | 7952626 (20:52:07) | 0 | 1/1 | CR:28, :61, :87, :98 |
| - | recap agent | before 20:34 | 20:35 | CALL-RECAP.md written | n/a | n/a | 0/0 | CR:27, :32 |

## Planners (CR:41, start 20:38)
| slot | slug | start | end | outcome | head | dirty | stops/retries | source |
|---|---|---|---|---|---|---|---|---|
| - | planner gaia | 20:38 | 20:44 | done, plans/gaia.md | n/a (box file) | n/a | 0/0 | CR:41, :72 |
| - | planner nerve | 20:38 | 20:44 | done | n/a | n/a | 0/0 | CR:41, :72 |
| - | planner cortex | 20:38 | 20:44 | done | n/a | n/a | 0/0 | CR:41, :72 |
| - | planner integration-ci | 20:38 | 20:44 | done | n/a | n/a | 0/0 | CR:41, :72 |
| - | planner docs-whitepaper | 20:38 | 20:44 | done | n/a | n/a | 0/0 | CR:41, :72 |
| - | planner security | 20:42 | 20:45 | done | n/a | n/a | 1/1 | CR:43, :53, :76-77 |
| - | planner vps-ops | 20:42 | 20:46 | done | n/a | n/a | 1/1 | CR:43, :52, :79-80 |

## Other agents started during the run (no slot recorded)
| slot | slug | start | end | outcome | head (commit time) | dirty | stops/retries | source |
|---|---|---|---|---|---|---|---|---|
| - | settings-snapshot | ~20:41 | 20:41 (685f57f); later 20:57 rule update | pushed | aab8d85 (20:57:23) | 0 | 0/0 | CR:45, :93 |
| - | nonrust-inventory | ~20:41 | 20:46 | done, plans/non-rust-inventory.md | n/a | n/a | 0/0 | CR:70, :78 |
| - | voice-rust | ~20:41; restart 21:41 | 21:41 | done, 03d21a9 | 03d21a9 (21:41:15) | 0 | 1 interrupt / 1 | CR:70, :102-103, :105 |
| - | guard-rust | ~20:41 | 21:30 | done, d5f7874 | d5f7874 (21:29:49) | 0 | 0/0 | CR:70, :101 |
| - | hermes-core | 21:02 | 21:33 | done, 45b7980 | 45b7980 (21:32:17) | 0 | 0/0 | CR:94, :104 |
| - | console-ui-rust | not recorded | 21:24; pass 2 21:51 | done, 3d6c436 then c242fed | c242fed (21:50:18) | 0 | 0/0 | CR:98, :106 |
| (28) | sqlite-pure-spike | 21:09 (slot 28 follow-up) | 22:06 | done, 98686ef | 98686ef (22:05:43) | 0 | in slot 28 row | CR:97, :108 |
| - | slavik-todo (t107u/t108u) | not recorded | 22:16; update 22:35 | pushed d7e5895 then 87fdd91 | 87fdd91 (22:33:56) | 0 | 0/0 | CR:109-110 |

## Wave 3 (launched 22:52, CR:112-134)
All 13 at 22:52, 0 stops, 0 retries, "running" at the time of writing (CR:122-134). Heads are the base at the 22:51 snapshot, not results.
| slot | slug | head at 22:51 | dirty |
|---|---|---|---|
| 37 | merge-gaia (worktree integration-2) | 328a386 (22:49:46) | 3 |
| 38 | cutover-prep | c5ab8f9 (base) | 0 |
| 39 | swarm-record | 13a296c (base) | 0 |
| 40 | landing-overview | c242fed (base) | 0 |
| 41 | hades-adversarial | ccaab42 (base) | 0 |
| 42 | decision-memos | worktree created after the 22:51 snapshot | - |
| 43 | cortex-sensor | 8fd26f6 (base) | 0 |
| 44 | cortex-sources | 8fd26f6 (base) | 0 |
| 45 | vps-baseline | worktree created after the 22:51 snapshot | - |
| 46 | whitepaper-evidence | 13a296c (this branch) | - |
| 47 | rust-ports | worktree created after the 22:51 snapshot | - |
| 48 | build-info | worktree created after the 22:51 snapshot | - |
| 49 | health-land | worktree created after the 22:51 snapshot | - |

## Not covered
- Wave-1 slots 02-21 (20 branches "green at push", CR:8) have no per-agent lines in CALL-RECORD, so no start/end/outcome rows here. Their worktrees exist (common, desk-linkwatch, docs-sync, edge-boot, gaia-live, hades-approval, hades-redteam (1 dirty), hermes-tools, lan-discovery, launch-gates, memory-real, nerve, one-probe, poseidon, real-harness, room, sandbox, survival-heal, tuner-cutover, ucpu, voice-mouth; plus gaia-poseidon 498e3d2, gaia-hades-tokens f3f41a2, deploy-rust c5ab8f9).
- The 11 wave-2 agents killed ~20:14 (CR:7) are not identified by slug.

## Spot-checks (22:51 +02:00)
| row | CALL-RECORD says | git says | match |
|---|---|---|---|
| 33 deploy-sim | c5ab8f9, DONE 21:26 (CR:99) | `log -1` c5ab8f9 2026-10-20 21:25:47 +0200, 0 dirty | yes |
| 31 lan-routing | e8169c9, DONE 20:48 (CR:82) | e8169c9 2026-10-20 20:48:19 +0200, 0 dirty | yes |
| 28 common-migrate-2 | 123eab2, DONE 22:06 (CR:108) | 123eab2 2026-10-20 21:55:28 +0200, 0 dirty | yes (commit 11 min before the DONE line) |
