<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real checks for slot 418 owner-decisions-digest-3 (2026-10-28 18:03 CET, +01:00)

All read-only. No secrets read or printed. No VPS writes, no service change, no push to main or main-staging.
Doc-only task (row 418 "Doc only, no builds"): no cargo builds or tests.

## VPS (ssh slavik-vps, `systemctl show`, `stat` metadata, loopback GETs; 18:03:21-18:03:32 CET)
| Unit | Active | NRestarts | ExecStart binary |
|---|---|---|---|
| galion-nerve | active | 0 | openmind-nerve-9baa450 |
| gaia-cortex | active | 0 | gaia-cortex-58a12e6-llm |
| console (:8080) | active | 0 | galion-console-serve-5aa31927 |
| console-rust-primary (:18080) | active | 0 | galion-console-serve-5aa31927 |
| hermes-rs | active | 0 | hermes-gateway-b7fbbc78 |
| hermes-gateway | inactive | 0 | - |
| galion-one | active | 0 | galion-one |

- `stat` link.env: `600 root:root` (D-318-2 applied).
- HTTP (Host console.galion.app): 8670 /health 200, 8674 /health 200, 8080 /api/v1/admin 200, 18080 /api/v1/admin 200,
  8080 /console 200, 8002 /health 503 (mouth, pre-existing), 8642 /health 200.
- Cortex /health 18:03:24: ok true, mode live, nerve true, llm_enabled true, incidents_open 2, **escalations_open 0** (was 11 at 15:37), handoffs 0.
- Nerve /health 18:03:24: status ok, system degraded (edge/mouth, pre-existing), score 78, cycle 2 since the 17:40 restart.
- `/opt/galion/rust/bin/galion-restore-test.sh`: plain file 840 B, mtime Oct 18 20:13; no `.pre361`, so D-361-1 not applied.

## GitHub API (read-only, 18:03 CET)
- PR #12 `swarm/main-staging` -> `main`: open, merged false, mergeable_state unstable, base main 13a296ce.
- main-staging head 3250d07d (committed 17:58:06 CET); history: ef99947c merge 83e573e2 (373), b4fe2383 merge d50478db (374),
  811797a9 / 09fb719c / 3250d07d CI fixes (all "t334u").
- Combined status on 3250d07d: pending, 0 statuses at query time.
