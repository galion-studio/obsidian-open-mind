<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Concurrency: the measured limit (swarm of 2026-10-20)

All times Europe/Zagreb, CEST +02:00. CR:n = /workspace/swarm/CALL-RECORD.md line n. Per-agent rows: evidence/fullcap/ledger.md.

## Result
- **Measured real limit: about 18-20 agents running at once.** Above that, the agent platform's request limit stopped agents (CR:42, :43, :80 "Real concurrency limit: ~18-20").
- **Configured ceiling: 250 agents with auto-accept** (stated in the call, CR:41; not verified from settings, see M10). The configured ceiling was never the binding constraint; the request limit was.
- CPU was not the limit at that moment: box load 3.29 on 8 vCPU at 20:40 (one sample, plans/docs-whitepaper.md §4 item 5).
- Consequence: wave 3 was sized to 17 running (CR:117 "under the measured 18-20"); the hermes-core governor defaults to cap 16 and its real test used cap 24 vs limit 18 (2 stops, 2 retries, 25/25 done, CR:104).

## Timeline
| time (+02:00) | event | running after | source |
|---|---|---|---|
| ~20:14 | request limit killed 11 of 15 wave-2 agents | ~4 of wave 2 | CR:7 |
| 20:26 | sec-runbooks (26), console-static (34), common-migrate-2 (28) restarted (retry 1) | - | CR:62-64 |
| 20:31 | 6 agents on box | 6 | CR:7 |
| 20:34 | full-capacity run starts: 7 running + 8 restarted (fuzz-sast, one-hardening, nerve-wiring, lan-routing, hermes-enroll, deploy-sim, tunnel-audit, memory-schema); target 16 | 16 | CR:26-29, :56-61 |
| 20:38 | 7 planners added | 22 (15 coding + 7 planners) | CR:41 |
| 20:38 | limit stopped deploy-sim (33) and one-hardening (29) | 20 | CR:42 |
| 20:38 | limit stopped 2 planners (later identified as vps-ops and security) | 18 (13 coding + 5 planners) | CR:43, :52-53 |
| 20:41 | standing order: restart every limit-stopped agent until done | - | CR:47 |
| 20:42 | restarts: planners vps-ops + security (retry 1), deploy-sim + one-hardening (retry 2) | 20 (13 coding + 7 planners) | CR:52-55, :65 |
| 20:44 | 5 planners done; gaia-land stall restart (not a limit stop) | - | CR:72-73 |
| 20:45 | security planner done after retry 1 | - | CR:76-77 |
| 20:46 | vps-ops planner done after retry 1 | - | CR:79-80 |
| 20:52 | one-hardening done after retry 2; gaia-land done after stall restart | - | CR:83-86 |
| 21:26 | deploy-sim done after retry 2 | - | CR:99-100 |
| 22:52 | wave 3: 4 running + 13 new = 17, 0 stops so far | 17 | CR:115-134 |

Outcome of every limit-stopped agent identified by name: all restarted ones completed or are still running; none was given up (retry ledger CR:49-65, :77, :80, :86, :100).
No limit stops are recorded after 20:38 (CR:49-134).

## What is NOT recorded
- **Raw error text** of any stop: the record only says "request limit stopped" (CR:7, :42, :43). No platform error message, status code or log.
- **The slugs of the 11 agents killed at ~20:14** (CR:7; CALL-RECAP §4.16). The exact time is "~20:14", not a timestamp.
- Which 2 planners were stopped at 20:38 was not written at the time (CR:45 "sector not yet identified"); vps-ops and security are inferred from the 20:42 retry ledger (CR:52-53).
- No per-minute concurrency series (M1) and no stepped test 16 → 20 → 24 → 32 (M2). "18-20" rests on two stop events at 22 → 20 → 18 within one minute; there is no test of 19 or 20 holding for 20 minutes.
- The 250/auto-accept ceiling is as stated in the call (CR:41), not from a settings export (M10). Request/token counts per agent (M6) are not visible from the box.
- CR:65 has an inconsistent note ("14 coding incl. gaia-land, 6... — 13 coding + 7 planners"); 20 total is used here.
