<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real checks for slot 429 owner-decisions-digest-4 (2026-10-28 21:40-21:41 CET, +01:00)

All read-only. No secrets read or printed. No VPS writes, no service change, no push to main.
Doc-only task (row 429 "Doc only, no builds"): no cargo builds or tests. Ports 22990-22999 not used.

## VPS (ssh slavik-vps, `systemctl show/is-active`, `stat`, loopback GETs; 21:40:46-21:40:54 CET)
| Unit | Active | NRestarts | ExecStart binary |
|---|---|---|---|
| galion-nerve | active | 0 | openmind-nerve-9baa450 |
| gaia-cortex | active | 0 | gaia-cortex-58a12e6-llm |
| console (:8080) | active | 0 | galion-console-serve-5aa31927 |
| console-rust-primary (:18080) | active | 0 | galion-console-serve-5aa31927 |
| hermes-rs | active | 0 | hermes-gateway-b7fbbc78 |
| hermes-gateway | inactive | 0 | - |
| galion-one | active | 0 | galion-one |
| tuner-quick | LoadState not-found | - | - (slot 420: transient unit lost 2026-10-22) |

- HTTP: 8670 /health 200, 8674 /health 200, 8642 /health 200; Host console.galion.app: 8080 /api/v1/admin 200,
  18080 /api/v1/admin 200, 8080 /console 200.
- Cortex /health 21:40:48: ok true, mode live, nerve true, llm_enabled true, incidents_open 2, escalations_open 0, handoffs 0, events_in 41.
- Nerve /health 21:40:48: status ok, system degraded, **score 68**, cycle 18.
- `/opt/galion/rust/bin/galion-restore-test.sh`: 755 root:root, 840 B, mtime 2026-10-18 22:13 CEST (+02:00); 0 `*pre361*` files -> D-361-1 not applied.
- `/var/lib/memory-tencentdb/vectors.db` still present -> D-406-1 not run.

## GitHub API (read-only, 21:41 CET)
- PR #12: state closed, merged true, merged_at 18:09:32 CET, merge_commit_sha 7ba85b603d8b0b87c4d4dede1056e1ee80c767ac.
- main head 7ba85b60 (tree a6f6c163, matches slot 421's rebuilt tree).
