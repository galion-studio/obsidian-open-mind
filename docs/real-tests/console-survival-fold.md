<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-survival-fold (wave 4 slot 425): Console V2 prep, NOT the live cutover

Branch `swarm/console-survival-fold`. Nothing deployed, no live console/unit/permission change.

## What is folded
- Base: console-deploy-candidate **4f763ade** (code 5aa31927 = the console live on :18080 and :8080
  since 2026-10-28 17:29/17:34 CET +01:00).
- Merge **1767bafa** = console-survival-card **1b7e3e8b** (D-408-1 survival card on `/bank`), which
  carries survival-refresh-writer **d25f8479** (D-389-1 in-console refresh, `galion-survival` crate)
  and console-bank-asof **99ac55b5**. Clean merge, 0 conflicts (common base 7888d8fb).
- `crates/*/src` at the branch tip is identical to 1767bafa (later commits: tests, deploy, docs), so
  the deploy binary is `galion-console-serve-1767bafa`.

## Staged drop-in (NOT installed)
`deploy/console-rust-serve/survival-fold/console-rust-primary.service.d/70-survival.conf`
- Applies on top of the live primary unit + the installed `60-admin-store.conf`.
- New as-built mirror of that installed drop-in:
  `deploy/console-rust-serve/live/console-rust-primary.service.d/60-admin-store.conf` (read-only
  `cat` 2026-10-28 21:41 CET +01:00, 1291 B, sha256 fa2266d0eacda459..., byte-identical to the
  slot 343 candidate file).
- Effective argv = live 5aa31927 argv (incl. `--admin-store/--admin-mirror/--arena-root`), binary
  renamed to `-1767bafa`, plus `--survival-file /opt/galion/galion-light/state/survival.json
  --survival-writer /opt/galion/rust/bin/galion-survival --survival-refresh-s 900 --survival-mode
  interval` (D-389-2: interval, 900 s).
- `ReadWritePaths=-/opt/galion/galion-light/state` is ADDED to the admin-store paths.
  `/opt/galion/hermes` stays read-only, so the D-389-3 HOLD-park write to `modes.json` keeps failing
  silently (survival.json is still written).
- Primary only. `console.service` (:8080) must never get `--survival-writer` (one writer per file);
  it may get `--survival-file` alone (status + card only).
- Install = add the file + daemon-reload + restart; rollback = remove it (back to 5aa31927 argv).
- The slot 389 drop-in `deploy/staged/survival-refresh/console-rust-primary-survival.conf` is
  marked SUPERSEDED (old argv without arena/admin flags, `-SHA` placeholder).

## Tests (box, 2026-10-28 21:43-21:52 CET +01:00)
New `crates/console-static/tests/real_survival_fold.rs` (ports 22950-22959):
- `fold_dropin_is_live_argv_plus_survival` (always on): fold argv == live argv + exactly the 8
  survival args; ProtectSystem=strict; ReadWritePaths = live + state/; survival.json and
  eval/survival.txt covered, `modes.json` and the writer binary not; writer under ReadOnlyPaths;
  live today cannot write survival.json; fallback has no `--survival*`; 389 file superseded.
- `real_survival_fold` (GALION_REAL=1, release `galion-console-serve` with the remapped unit argv,
  real release `galion-survival`, empty key file, decoy `OPENROUTER_API_KEY` in the console env):
```
1) refreshed 50 ms after start, no request; file when=2026-10-28T21:44:20+01:00 bytes=1325
2) status interval refreshS 900 runs 1 lastExit 0 stale false; POST 405, foreign host 401,
   40 GETs (/api/v1/survival + /bank) -> runs 1
3) /bank card [fresh] Survival snapshot 2026-10-28 21:44 CET (+01:00) ... 1 runs, 0 failed · interval refresh
4) /api/arena 200 ids 2/2, /arena 200, health 200, /api/v1/bank 404, board unchanged
5) fallback (--survival-file only) writer null, card [fresh], file untouched
live: drop-in == mirror, dir [60-admin-store.conf], survival.json age 27.4 h, :18080 /api/v1/survival 404
test result: ok. 2 passed; 0 failed (13.96 s)
```
Required regressions on the fold tree (GALION_REAL=1, release):
- real_survival_refresh 1/1 (8 legs incl. live mtime 27 h old), 35.1 s.
- real_survival_card 1/1 (all states, live mtime leg stale 27.4 h), 3.4 s.
- real_deploy_candidate 2/2 incl. CONSOLE_CANDIDATE_LIVE=1 (live board 12/12 ids), 2.7 s.
- Also green: real_bank_asof 2/2, real_dist_arena_bank 3/3, real_admin_lockout 1/1,
  real_admin_flags_stage 1/1, real_v2_merge 1/1, real_landing 1/1, real_console_rust_serve 1/1,
  real_pairing_e2e 1/1, real_arena_bank 3/3.

repo-check: base 4f763ade FAILED (2 `hosts` errors: hard-coded VPS IPv4 default in
tests/real_admin_flags_live.rs); fixed here (default ssh alias `slavik-vps`, env override kept) ->
repo-check 900 files 0 errors 58 warnings PASS. real_admin_flags_live itself still asserts the
pre-17:29 state (no drop-ins, binary 2d846fb8), so its live leg is stale since the 5aa31927 install
(F-425-3, not run live here).

Gates: `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude
openmind-sandbox --exclude cgroup-detect -- -D warnings` ok; `cargo test --workspace --exclude
openmind-sandbox --exclude cgroup-detect` (GALION_REAL unset; hermes-tools not in this lineage)
152 suites, 751 passed, 0 failed, 7 ignored (21:47-21:48); `scripts/no-python.sh` ok; SPDX ok.

Artifacts (release):
| target | binary | bytes | sha256 (first 16) |
|---|---|---|---|
| x86_64-unknown-linux-musl | galion-console-serve | 1102720 | 2fdc75b8c09dcedc |
| x86_64-unknown-linux-musl | galion-survival | 586416 | 91b8e96773b76f66 |
| aarch64-unknown-linux-musl (rust-lld) | galion-console-serve | 933784 | f3cb1c45cebfe4ea |
| aarch64-unknown-linux-musl (rust-lld) | galion-survival | 514912 | 11522969d3f6443b |
`cargo check` aarch64-linux-android, armv7-linux-androideabi, armv7-unknown-linux-musleabihf ok.
Console binary +73,688 B vs live 5aa31927 (1,029,032 B). The drop-in uses the INSTALLED
galion-survival (594,640 B, S4.7), not the one built here.
musl x86_64 smoke :22952 (fold argv, interval 900): refreshed at start, runs 1, `/bank` card
fresh, RSS 1196 kB, decoy key 0 hits in the file.

## VPS (read-only plus one throwaway run, 2026-10-28 21:41-21:50 CET +01:00)
- `console-rust-primary` / `console` active, NRestarts 0, started 17:30 / 17:34 CET; drop-in dirs
  hold only `60-admin-store.conf`; `:18080` and `:8080` health 200, `/bank` 200,
  `/api/v1/survival` 404 (fold not installed). No survival timer.
- Live `survival.json` still mtime 2026-10-27 18:19:50 CET (1385 B), ~27.5 h stale (F-408-1 stands).
- Nerve `:8670/state` 200 32,230 B, score 68 / D, cycle 18. Cortex `:8674/health` 200.
- Throwaway as `nobody` in `/var/tmp/survival-fold-test-20261028215006` on :22953: the musl fold
  console with the fold argv (temp roots, empty key file) + the LIVE writer
  `/opt/galion/rust/bin/galion-survival`: interval run at start -> temp survival.json
  `when 2026-10-28T21:50:16+01:00`, `measured false` (no key, nothing spent), decoy 0; 30 GET /bank
  -> runs 1; card `fresh`; `/api/arena` 200, health 200, POST 405, foreign host 401; RSS 2988 kB.
  Live survival.json mtime and live `modes.json` hash unchanged; folder removed; 0 listeners.

## 413 console-deploy-candidate-2 (pushed c2522a99 before this start): merge order
- 413 is also built on 4f763ade (plus console-rust-routes-a and its main-staging lineage).
- Rehearsed `git merge` of c2522a99 into this branch (throwaway worktree, not committed): one
  conflicted file, `crates/console-static/src/config.rs`, 5 hunks, all keep-both (survival flags
  next to `--owner-routes/--hermes`); after that console-static + console-ui 42 suites 307/0/8 ign;
  GALION_REAL real_survival_fold 2/2, real_deploy_candidate_2 2/2, real_survival_card 1/1.
- Do NOT install both staged drop-ins on top of each other: 413's replaces `60-admin-store.conf`
  (binary -4e9e9e50 + `--owner-routes --hermes`), and this `70-survival.conf` would then reset
  ExecStart back to -1767bafa WITHOUT the 413 flags. The live leg of real_survival_fold fails
  ("installed drop-in drifted from mirror") if 60-admin-store.conf changes, as a guard.
- Expected order: land 413 c2522a99, then this branch (resolve config.rs keep-both, as rehearsed),
  build ONE binary from the combined tree, and regenerate `70-survival.conf` from the then-installed
  argv (candidate-3). If only one goes live tonight, either branch works alone.

## Findings
- F-425-1: my first read-only view of the live drop-in was filtered by my own `grep -v secret`, which
  hid the `--admin-store` lines; the byte-exact `cat` + sha256 showed the installed file equals the
  slot 343 candidate. The mirror is now checked byte-for-byte by the live leg.
- F-425-2: survival.json is still 27+ h stale on live; nothing refreshes it until D-425-1.
- F-425-3: real_admin_flags_live expects the pre-17:29 live state; needs a refresh to the 5aa31927
  drop-ins (not done here; real_survival_fold carries the current drift check for the primary).

## Owner decisions
- **D-425-1** (covers D-389-1, D-389-2 = interval 900 s, D-408-1): install `70-survival.conf` on
  `console-rust-primary` with `galion-console-serve-1767bafa` (rebuild from 1767bafa, publish with
  `.sha256`). Recommended yes; primary first, fallback stays writer-free.
- **D-425-2** merge order with 413: recommended one combined candidate (413 then 425) and one
  restart, instead of two stacked drop-ins.
- D-389-3 stays HOLD: the modes.json write keeps failing silently by design.
