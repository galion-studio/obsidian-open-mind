<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-v2-admin-fold real tests (wave-4 slot 331) — 2026-10-28 CET (+01:00)

Box only for binaries; VPS read-only (systemctl cat/show, test -e, loopback GETs). Ports
remapped into the slot range 22010-22019 for the run only (remap not committed; each test
ran sequentially with `--test-threads=1`). All processes and tunnels gone after; ports free.

## Box, release build, `GALION_REAL=1` (00:14:11–00:15:01 +01:00)

| test | result | key output |
|---|---|---|
| real_admin_flags_stage | 1/1 | A: 10 board 200, remote admin 200, pair page 200, Node cookie ok, store=mirror 0600 hashes only; B: F-313-1 fixed — RO store 6 wrong → 503,503,503,503,429,429, right → 429; no RW → new pair 503 'store not writable', store unchanged; C: live argv on new build → remote /api/v1/admin 503 'admin store not configured'; D: mirror RO → pair 200, mirror stale |
| real_admin_lockout | 1/1 | A–E pass in 400 ms: 5×401 → 429 persisted across restart; RO → identical 503 ×5 then 429; 8×6 parallel wrong → 401 ×5, 429 ×43; device resume 200; corrupt store → 503, file untouched |
| real_board_api_parity | 1/1 | Rust board routes 200 |
| real_v2_merge | 1/1 | |
| real_pairing_e2e | 1/1 | |
| real_landing | 1/1 | |
| real_sprite_health | 1/1 | |
| real_arena_bank | 3/3 (live leg separate below) | bare GET /arena 200 14165 B |

## VPS read-only (00:15:23–00:15:45 +01:00)

`GALION_REAL=1 CONSOLE_UNIT_LIVE=1 cargo test -p console-static --test real_admin_flags_live` 1/1:

```
console: mirror == live (1230 B), NRestarts=0 ActiveState=active SubState=running DropInPaths= dropin-dir=no
console-rust-primary: mirror == live (1047 B), NRestarts=0 ActiveState=active SubState=running DropInPaths= dropin-dir=no
8080  /api/v1/admin lo=404 remote=404 | services 404/404 | catalog 404/404 | health 200/200 | /console remote=503
18080 /api/v1/admin lo=404 remote=404 | services 404/404 | catalog 404/404 | health 200/200 | /console remote=503
nerve /state 200
live bin galion-console-serve-2d846fb8 sha256 516556f946977d2d…; galion-console-serve-9c0f9d1a absent
```

`CONSOLE_ARENA_LIVE=1 … real_arena_bank real_live_vps_upstream_readonly` 1/1 (ssh -L 22019 → VPS :18080):
`/api/arena -> 404, /api/v1/bank -> 404` upstream; box `/arena` 200 14165 B, `/bank` 200 14949 B
(empty state). Note: slot 321 at 23:05 +01:00 recorded these upstream routes as 200; at 00:15 they
are 404 — worth a look under D-321-1.

The live gap the fold closes is still there (admin + board routes 404 on both live units, remote
/console 503); units unchanged, staged only. Cortex observation: not used here; nerve /state read (200).
