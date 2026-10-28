<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-v2-merge-2 (wave-4 slot 321) — 2026-10-27 CET (+01:00)

Prep tip only. NOT the live cutover, NO deploy, no VPS writes.

## What was folded

| step | commit | source |
|---|---|---|
| base | `2087316a` | swarm/console-v2-merge-1 (slot 300: sprite-health + pairing-e2e + session-pack) |
| merge 1 (`--no-ff`) | `9c0f9d1a` | swarm/console-board-api-parity (slot 303; carries console-rust-serve-impl 2d846fb, slot 302) |
| merge 2 (`--no-ff`) | `57851e34` | swarm/console-arena-bank-rust (slot 299) |

Both merges: **0 conflicts** (ort auto-merge; `crates/console-ui/src/lib.rs`, `overview.rs`,
`tests/ui_port2.rs` and `docs/console/v2-plan.md` auto-merged; v2-plan R2 now reads
"IMPLEMENTED" with the slot-299 text). No new dependencies (see the existing
`docs/third-party/console-board-api-parity.md`, `console-rust-serve.md`,
`console-arena-bank-rust.md`).

## Gates (box, 2026-10-27 22:53-23:15 CET)

- `cargo fmt --all --check` ok
- `cargo clippy -p console-static -p console-ui --all-targets -- -D warnings` ok
- `cargo test -p console-static -p console-ui`: 28 suites, 224 passed, 0 failed, 7 ignored
- `scripts/no-python.sh` ok
- release musl `galion-console-serve`: x86_64 1024896 B (sha256 9d0d3f69af6b80f7…),
  aarch64 (rust-lld) 866936 B (d3abf7ec875c1707…); `cargo check` armv7-unknown-linux-musleabihf
  and aarch64-linux-android ok.

## Real tests (GALION_REAL=1, release, box 23:04:28-23:05:07 CET)

Ports temporarily remapped into slot 321's range 21910-21919 (remap NOT committed;
committed ports stay those of the original slots):
board_api_parity 21910/21911, landing 21912/21913, v2_merge 21914/21915,
arena_bank 21916-21919 (live sub-test 21916/21917).

| test | result | evidence |
|---|---|---|
| real_board_api_parity (+`CONSOLE_BOARD_LIVE=1`) | 1/1 pass, 13.9 s | Rust real flow ok in 21 ms (catalog, pair, lockout 429, 413/411, foreign host 401) |
| real_landing | 1/1 pass | /console 200 31678 B, 0 `<script>`, 9 cards; / 302 -> /console; 6 pages 200; light theme cookie |
| real_v2_merge | 1/1 pass, 5.5 s | pairing+session-pack 8/8 NEG/OK, 16 chrome pages -> 1 upstream health, sprite 46 symbols |
| real_arena_bank (+`CONSOLE_ARENA_LIVE=1`, read-only ssh -L to VPS :18080) | 3/3 pass | bare /arena 200 14165 B, /bank 200 14949 B; full /arena 14494 B; live upstream /api/arena 200, /api/v1/bank 200 |

Live comparison printed by real_board_api_parity (read-only GET through ssh -L to VPS :8080):

```
path | host | live :8080 | rust
/api/v1/health | 127.0.0.1 | 200 | 200
/api/v1/services .. /api/v1/markers (9 routes) | 127.0.0.1 | 404 | 200
/api/v1/admin | console.galion.app | 404 | 200
/api/v1/services | console.galion.app | 404 | 401
/console | console.galion.app | 503 | 200
/services | 127.0.0.1 | 200 | 200
```

So the live :8080 console (2d846fb8) still has the D-303-1 gap; this tip closes it in tests.

### Fold integration probe (manual, 23:05 CET, port 21916)

Merged binary with `--dist <console-dist-static> --ui rust` (the live :8080 shape, in-process
local-api): /console 200, /services 200, /api/v1/services 200, /api/v1/health 200,
**/arena 200 14165 B and /bank 200 14949 B = the "bare" pages**, because the in-process
local-api answers neither `/api/arena` nor `/api/v1/bank` (404). With the live :18080 upstream
(console-rust-primary) both data routes answer 200 — so a `--dist` cutover would show arena/bank
in their honest empty state. See D-321-1.

## VPS read-only (2026-10-27 23:05:42 CET)

nerve :8670 /state: console (:8080, console.service) `healthy`, restarts_1h 0;
`console` and `console-rust-primary` units active. Nothing changed.

## Owner decisions

- **D-321-1** `--dist` mode (live :8080 shape) has no `/api/arena` / `/api/v1/bank` source:
  either add `--upstream 127.0.0.1:18080 --proxy-prefix /api/arena --proxy-prefix /api/v1/bank`
  at cutover, or port those readers into local-api. Until then /arena and /bank render the
  empty state (no crash, no JS).
- **D-321-2** Carried from slot 313: F-313-1 (read-only admin store never locks, right code
  -> 503 = code oracle) is in this tip too (admin.rs unchanged); fix before any deploy of it.
- **D-321-3** Next fold: slot 331 console-v2-admin-fold (313's staged drop-ins) should rebase
  onto this tip instead of merge-1 to avoid a second fold.
