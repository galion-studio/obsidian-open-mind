<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-deploy-candidate (wave 4 slot 343) — single console deploy candidate

Branch `swarm/console-deploy-candidate`. NOT the live cutover, nothing deployed, no VPS change.

## The one deploy candidate
- **Code SHA: `5aa31927`** (5aa31927825b60d81cc2e63283cc7755d50115de) = merge of
  console-dist-arena-bank **7888d8fb** (D-336-1 `--arena-root`, `--data-upstream`) onto
  console-v2-admin-fold **c7cebb28** (console-v2-merge-2 cd1204a0 + console-admin-lockout-fix
  dfa120c6 = slot 313 admin flags + **F-313-1 lockout fix**). Clean merge, no conflicts
  (common base cd1204a0; 7888d8fb only touched console-static api/arena/config/server + docs).
- Later commits on this branch add tests, staged drop-ins and docs only; `crates/` source
  is identical to 5aa31927, so the binary is named after it.
- Supersedes as deploy candidates: 9c0f9d1a (no lockout fix, F-313-1), 49b31b3f / dfa120c6 /
  c7cebb28 (no arena reader), 7888d8fb (no F-313-1 fix).
- Staged (not installed) drop-ins, same keys as the slot 313 drop-ins plus one flag pair:
  `deploy/console-rust-serve/candidate/console.service.d/60-admin-store.conf` (:8080) and
  `deploy/console-rust-serve/candidate/console-rust-primary.service.d/60-admin-store.conf`
  (:18080): binary `/opt/galion/rust/bin/galion-console-serve-5aa31927`, `--admin-store`,
  `--admin-mirror`, `ReadWritePaths` unchanged, **+ `--arena-root /workspace/arena`**.
  No `--data-upstream` (D-336-2 bank reader undecided -> `/api/v1/bank` honest 404).

## Artifacts (box, 2026-10-28 ~02:35 CET +01:00, release, from 5aa31927 tree)
| target | binary | bytes | sha256 (first 16) |
|---|---|---|---|
| x86_64-unknown-linux-musl | galion-console-serve | 1037184 | 8c531a3a42582ac8 |
| x86_64-unknown-linux-musl | galion-console-static | 1037184 | 1a2be33f5135120f |
| aarch64-unknown-linux-musl (rust-lld) | galion-console-serve | 874424 | e23d01258ef5a160 |
`cargo check` aarch64-linux-android, armv7-linux-androideabi, armv7-unknown-linux-musleabihf: ok.
musl x86_64 smoke (port 22134): `/api/v1/health` 200, `/api/arena` 200 (missing root), RSS ~1.1 MB.
(Size vs 7888d8fb static: 1033088 -> 1037184 B, +4 KiB for the lockout fix.)
The deploy step must rebuild from 5aa31927 (or a later tip with the same crates/ tree) and
publish `galion-console-serve-5aa31927` + `.sha256` next to `-2d846fb8`; not done here.

## Gates (box, 2026-10-28 02:26-02:40 CET +01:00)
- `cargo fmt --all --check` ok.
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  147 suites, 717 passed, 0 failed, 7 ignored (hermes-tools excluded as in slot 336: sandbox_vps_only).
- console-static + console-ui: 254 passed, 0 failed, 7 ignored (252 before + 2 new in real_deploy_candidate; real leg skips without GALION_REAL).
- `scripts/no-python.sh` ok. No new deps.

## Real tests (GALION_REAL=1, release, box)
New `crates/console-static/tests/real_deploy_candidate.rs` (ports 22130-22139): one process
started with the EXACT argv of live `console.service` + candidate drop-in (paths/ports remapped):
```
candidate_dropins_are_the_admin_dropins_plus_arena ... ok   (argv = slot 313 argv + --arena-root; RWPaths same; primary = fallback binary)
B: /api/arena 200 357 B 0 ms ids 3/3, /arena 200 14513 B, POST/PUT/DELETE 405, board unchanged, /api/v1/bank 404
B: remote /api/arena no cookie 401, paired cookie 200, foreign host 401; C: 10 board routes 200, / ok
A: 5x401 -> 429 wrong+right identical, lockedUntil persisted, hashes only; arena + paired cookie still 200 during lock
A2: RO store right/wrong 503 identical, then [503, 503, 503, 429], store unchanged
D: no arena root -> health 200, /api/arena 200 (empty board JSON, no path leak), /arena 200
live: board.jsonl 12 lines, 12 ids   (READ-ONLY ssh cat of VPS /workspace/arena/board.jsonl)
live: /api/arena 200 4039 B 0 ms ids 12/12, /arena 200 17198 B, POST/PUT/DELETE 405, board unchanged, /api/v1/bank 404
test result: ok. 2 passed; 0 failed   (2.69 s)
```
Regression on the same tree: real_dist_arena_bank 3/3 (incl. CONSOLE_ARENA_LIVE=1 live leg:
live :18080 `/api/arena` + `/api/v1/bank` 404, box `/api/arena` from live board 200 ids 12/12),
real_admin_flags_stage 1/1, real_admin_lockout 1/1, real_v2_merge 1/1, real_arena_bank 3/3,
real_landing 1/1, real_board_api_parity 1/1, real_console_rust_serve 1/1, real_pairing_e2e 1/1,
real_sprite_health 1/1, real_merge 1/1 (ssh -N GET-only forward), real_admin_flags_live 1/1
(CONSOLE_UNIT_LIVE=1, read-only). real_console_static FAILS for the known environment reason
only (`/workspace/swarm/artifacts/console-static/out`, the old Node build, is not on the box).

## VPS read-only (2026-10-28 02:27-02:37 CET +01:00)
- `console.service` active; `:18080` and `:8080` `/api/arena` 404, `/api/v1/bank` 404 (gap the
  candidate closes for arena; bank waits on D-336-2).
- real_admin_flags_live: both unit bodies == mirrors (1230 B / 1047 B), NRestarts 0, no
  drop-ins installed, live binary still 2d846fb8, `galion-console-serve-5aa31927` absent;
  remote `/api/v1/admin` + board routes 404 on both ports, `/console` 503 (gap the drop-in closes).
- `/workspace/arena`: root-owned, board.jsonl 4152 B (world-readable), no champions/.
- Nerve `:8670/state` 200 37109 B: console (:8080) `healthy`, restarts_1h 0, rss 712 kB.
  Cortex not consulted (nerve used). Temp file /tmp/.s343n on the VPS created and removed.

## Owner decisions (for the cutover, not taken here)
- **D-343-1** Approve 5aa31927 as THE console deploy candidate: install the two candidate
  drop-ins (instead of the slot 313 ones) + binary `galion-console-serve-5aa31927`, primary
  :18080 first, fallback :8080 after a soak. Rollback = remove drop-in + daemon-reload + restart
  (back to 2d846fb8 argv).
- **D-336-2** still open: bank reader. Until then `/bank` is the honest empty state.
- **D-336-3** Arena Breed/Promote/Stop stay 405 until Slavik approves the paid run.

## Open issues
- The 9c0f9d1a binary name inside the slot 313 drop-ins is obsolete; keep those files only
  as history (their tests still pin them).
- Nerve cycle ~8 min (N-332-1) means a post-deploy health flip shows up slowly; watch the
  unit directly during the cutover.
