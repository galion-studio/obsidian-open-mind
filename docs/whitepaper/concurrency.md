<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Concurrency: the measured limit (swarm of 2026-10-20)

All times Europe/Zagreb, CEST +02:00. CR:n = /workspace/swarm/CALL-RECORD.md line n. Per-agent rows: evidence/fullcap/ledger.md.

## Result
- **Recorded: the 20:38 stops happened at 22 -> 20 -> 18 running** (CR:42, :43; CR:79 "Real concurrency limit: ~18-20"). Fact-check (D1, docs/whitepaper/factcheck-2026-10-21.md): this is not a stable count limit. Every recorded stop follows a burst of new starts (20:38: 7 planners at once; 00:55: 20 launched, all 20 stopped, CR:184; 06:59: 9 stopped within ~1 min of start, 18 -> 9, CR:367; 08:55-08:56: 11 started at once, 16 -> 8, CR:514-516), while ~23 ran at 20:42-20:44 without a stop (D3). Until the stepped test M2 is run, "about 18-20" is the count at one burst, not a measured ceiling.
- **Configured ceiling: 250 agents with auto-accept** (stated in the call, CR:41; not verified from settings, see M10). The configured ceiling was never the binding constraint; the request limit was.
- CPU was not the limit at that moment: box load 3.29 on 8 vCPU at 20:40 (one sample, plans/docs-whitepaper.md §4 item 5).
- Consequence: wave 3 was sized to 17 running (CR:117 "under the measured 18-20"); the hermes-core governor defaults to cap 16 and its real test used cap 24 vs limit 18 (2 stops, 2 retries, 25/25 done, CR:104).

## Timeline
| time (+02:00) | event | running after | source |
|---|---|---|---|
| ~20:14 | (P0 pre-run) request limit killed 11 of 15 wave-2 agents | ~4 of wave 2 | CR:7 |
| 20:26 | (P0 pre-run) sec-runbooks (26), console-static (34), common-migrate-2 (28) restarted (retry 1) | - | CR:62-64 |
| 20:31 | (P0 pre-run) 6 agents on box | 6 | CR:7 |
| 20:34 | full-capacity run starts: 7 coding agents + the recap agent running (8) + 8 restarted (fuzz-sast, one-hardening, nerve-wiring, lan-routing, hermes-enroll, deploy-sim, tunnel-audit, memory-schema); target 16 | 16 | CR:26-29, :56-61 |
| 20:38 | 7 planners added | 22 (15 coding + 7 planners) | CR:41 |
| 20:38 | limit stopped deploy-sim (33) and one-hardening (29) | 20 | CR:42 |
| 20:38 | limit stopped 2 planners (later identified as vps-ops and security) | 18 (13 coding + 5 planners) | CR:43, :52-53 |
| 20:41 | standing order: restart every limit-stopped agent until done | - | CR:47 |
| 20:42 | restarts: planners vps-ops + security (retry 1), deploy-sim + one-hardening (retry 2) | 20 recorded (13 coding + 7 planners); ~23 if the 4 agents dispatched ~20:41 (settings-snapshot, nonrust-inventory, voice-rust, guard-rust; CR:45, :70) are counted (fact-check D3) | CR:52-55, :65, :70 |
| 20:44 | 5 planners done; gaia-land stall restart (not a limit stop) | - | CR:72-73 |
| 20:45 | security planner done after retry 1 | - | CR:76-77 |
| 20:46 | vps-ops planner done after retry 1 | - | CR:79-80 |
| 20:52 | one-hardening done after retry 2; gaia-land done after stall restart | - | CR:83-86 |
| 21:26 | deploy-sim done after retry 2 | - | CR:99-100 |
| 22:52 | (P2 wave 3, after the recorded run section) wave 3: 4 running + 13 new = 17, 0 stops so far | 17 | CR:115-134 |

Outcome of every limit-stopped agent identified by name: all restarted ones completed or are still running; none was given up (retry ledger CR:49-65, :77, :80, :86, :100). Fact-check (S4/S6): fuzz-sast, nerve-wiring, hermes-enroll and tunnel-audit later stalled; their work was finished by successor slots 100-103 at 00:59-01:02 (CR:187-190).
No limit stops were recorded between 20:38 and 22:52 (CR:49-119, as written at 22:52). Fact-check (D2): later stops are recorded: CR:123-124 (38-cutover-prep 2, 39-swarm-record 1), CR:177 (23:42), CR:184 (00:55, all 20), CR:237, :287-288, :367, :515-516.

## What is NOT recorded
- **Raw error text** of any stop: the record only says "request limit stopped" (CR:7, :42, :43). No platform error message, status code or log.
- **The slugs of the 11 agents killed at ~20:14** (CR:7; CALL-RECAP §4.16). The exact time is "~20:14", not a timestamp.
- Which 2 planners were stopped at 20:38 was not written at the time (CR:45 "sector not yet identified"); vps-ops and security are inferred from the 20:42 retry ledger (CR:52-53).
- No per-minute concurrency series (M1) and no stepped test 16 → 20 → 24 → 32 (M2). "18-20" rests on two stop events at 22 → 20 → 18 within one minute; there is no test of 19 or 20 holding for 20 minutes.
- The 250/auto-accept ceiling is as stated in the call (CR:41), not from a settings export (M10). Request/token counts per agent (M6) are not visible from the box.
- CR:65 has an inconsistent note ("14 coding incl. gaia-land, 6... — 13 coding + 7 planners"); 20 total is used here.
