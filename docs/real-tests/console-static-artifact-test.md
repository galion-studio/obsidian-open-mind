<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# real_console_static: asset set located, verified or generated (wave 4 slot 349)

Branch `swarm/console-static-artifact-test` off console-dist-arena-bank 7888d8fb.
Times Europe/Zagreb (CET +01:00). Ports 22190 (front) and 22191 (ssh forward).

## Problem (slot 336)

`crates/console-static/tests/real_console_static.rs` asserted that
`/workspace/swarm/artifacts/console-static/out` existed. After the box restore that
directory was gone, so `GALION_REAL=1` runs failed for an environment reason, and nothing
checked that a set which *was* present was the right one.

## Change

- New committed manifest `deploy/console-static/asset-set.2169d1c.manifest`:
  sha256 + size + path for the 37 files of `client/` and `routes.txt` (38 entries,
  871 756 B client bytes = BUILD-INFO.txt "files: 37 bytes: 871756"; the older
  docs/console-static.md said 36 files, which was one short).
- The test now locates the set in this order and never passes silently:
  1. `CONSOLE_STATIC_OUT` set: must contain `client/_shell.html` and match the manifest,
     else FAIL with a prerequisite message.
  2. Default box path present: must match the manifest, else FAIL (lists every bad file:
     size, sha256, missing, or extra file not in the manifest).
  3. Nothing found: with `CONSOLE_STATIC_REQUIRE_ARTIFACT=1` FAIL with the restore
     recipe below; otherwise it generates a minimal fixture set (shell, favicon, manifest,
     og.jpg, committed `routes.2169d1c.txt`) in a temp dir, prints
     `asset set: FIXTURE ... Real asset bytes are NOT checked in this mode`, runs every
     route/probe/method/ETag check and removes the dir.
- In ARTIFACT mode every built file except `_shell.html` is fetched over HTTP and its body
  sha256 and length are compared with the manifest (36 files).
- No new dependencies (uses the crate's own `console_static::sha256`). No Node is run.

## Restore recipe (box, read-only on the VPS)

The original set (Vite build 2026-10-20 20:36:44 +02:00, console-ide 2169d1c) is archived
on the VPS at `/srv/box-archive/2026-10-21/workspace/swarm/artifacts/console-static/out`:

```
mkdir -p /workspace/swarm/artifacts/console-static
ssh -i ~/.ssh/id_ed25519_vps -o IdentitiesOnly=yes root@<vps> \
  'tar -C /srv/box-archive/2026-10-21/workspace/swarm/artifacts/console-static -cf - out' \
  | tar -C /workspace/swarm/artifacts/console-static -xf -
```

Done on the box 2026-10-28 03:28 CET; the test then verified all 38 manifest entries.
(No secrets in the set: built JS/CSS/images, routes, BUILD-INFO, npm licence list.)

## Evidence (box, release build, 2026-10-28 03:27-03:33 CET)

| Run | Result |
|---|---|
| No set, default (clean box) | ok; `asset set: FIXTURE`; 16 pages 200, 16 probes 404 (3 replayed from nerve-capture), POST /agents 405, POST /.env 404, If-None-Match 304 |
| No set + `CONSOLE_STATIC_REQUIRE_ARTIFACT=1` | FAIL (expected): prerequisite message |
| `CONSOLE_STATIC_OUT=/nonexistent` | FAIL (expected): "has no client/_shell.html" |
| Real set (`CONSOLE_STATIC_OUT`) | ok; 38 entries verified; 36 built assets served byte-identical (sha256); 16/16 pages, 16 probes 404, 405, 304 |
| Tamper: 1 byte appended | FAIL (expected): `x-WYtXMPOY.js: 144 bytes, want 143` |
| Tamper: same size, 1 byte changed | FAIL (expected): `favicon.svg: sha256 differs` |
| Extra file in client/ | FAIL (expected): `extra.txt: not in manifest` |
| Missing file | FAIL (expected): `og.jpg: No such file` |
| Live leg 03:28:00 (`CONSOLE_STATIC_LIVE=1`, forward 22191 -> VPS :8080, GET only) | ok; /api/v1/health 200 same body; catalog/services/protocol/markers, /api/hermes/health, /api/nope, /api/.env all 404 = live 404, same body |

console-static package with `GALION_REAL=1` (restored set at default path), 03:28:30:
lib 57/0; real_arena_bank 3/3, real_board_api_parity 1/1, real_console_rust_serve 1/1,
real_console_static 1/1, real_dist_arena_bank 3/3, real_landing 1/1, real_pairing_e2e 1/1,
real_sprite_health 1/1, real_v2_merge 1/1 (9 real suites, 13/13); serve 6, ui 13,
ui_port1..5 6/8/7/7/9 all ok; 5 `#[ignore]` live-room tests not in the default run.

Gates: `cargo fmt --all --check` ok; `cargo clippy -p console-static --all-targets -- -D warnings`
ok; `scripts/no-python.sh` ok.

## Findings

- **F-349-1** Live :8080 is now the Rust `galion-console-serve-2d846fb8 --dist` (since the
  console cutover). Its `/api/v1/catalog`, `/services`, `/protocol`, `/markers`,
  `/api/hermes/health` and `/api/hermes/history` return 404 (the old Node console served
  200). So the `#[ignore]` live-room tests that compare against the Node API fail on the
  base tip too (checked with this change stashed): real_port2 (`/api/hermes/history` 404),
  real_port3 (live markers 404), real_port4 (live Node `/md/user` 404); real_port5 needs its
  ssh target env; real_merge 1/1 ok. Not changed here. Owner/next slot: either re-point
  these tests at the Rust API shape or retire them now that the Node console is gone.
- The asset set is the pre-Rust-only-rule Node/Vite build; it cannot be rebuilt on the box
  (no Node, by rule). The manifest pins exactly that build.

NO deploy, no VPS change (one read-only `tar -cf -` of the archived set, read-only GETs).
