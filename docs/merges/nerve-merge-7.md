<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-merge-7 (wave-4 slot 411), 2026-10-28 CET (+01:00)

Next nerve deploy candidate (D-396-2 + D-393-1 stamp), relative to the live
nerve `openmind-nerve-9baa450` (deployed 2026-10-28 17:40:13 CET) and GitHub
main `7ba85b60` (PR #12 merged 18:09 CET).

`swarm/nerve-merge-7` = `swarm/nerve-version-commit` eaa5d0ac (stamp, slot 393; on live 9baa450c)
+ `--no-ff` merge `swarm/nerve-edge-overcount` 3e2d8a42 (F-380-1, slot 394) -> d4b76d24, 0 conflicts
+ `--no-ff` merge `swarm/nerve-console-blindspot-test` 607e44fe (F-313-2 guard, slot 332) -> a5979a96.
Box + VPS read-only. **Not deployed**; no live file, unit or service changed.

## Merges

| Commit | What | Conflicts |
|---|---|---|
| d4b76d24 | merge 3e2d8a42 (logscan expire recount, real_edge_overcount) | 0 |
| a5979a96 | merge 607e44fe (not an ancestor: `merge-base --is-ancestor` = no; base 12956b45) + slot 411 test edits | 2, kept ours: `evt/journal.rs` (doc comment only), `tests/real_console_deep.rs` (ours = REAL_NERVE_PORT_BASE superset from nerve-merge-6) |

Slot 411 edits to `tests/real_console_blindspot.rs`: port block via
`REAL_NERVE_PORT_BASE` (default 22020 unchanged), and the F-396-1 repro:
with `Host: console.galion.app` every room (/console /node /hermes /status) on
the blind 2d846fb8 console must be 503 while loopback is 200, before the nerve
verdict is checked; the fixed console must not 503 them.

## Delta vs live 9baa450c (nerve src)

`build_info.rs` (new, stamp), `lib.rs` +1, `logscan.rs` +134/-3 (F-380-1),
`main.rs`, `server.rs` (stamp in `--version` and `/health`). Tests: version_stamp,
real_build_stamp, real_edge_overcount, real_console_blindspot. No dependency or
Cargo.lock change. The unstamped Z build of 9baa450c on the box is
**bit-identical** to the live artifact (sha256 8c9d7a5f..., 1,479,456 B).

## Relative to main 7ba85b60

GitHub compare 9baa450c...7ba85b60 = diverged (main 452 ahead, 54 behind): main
does not carry the nerve-merge chain (no `CONSOLE_DEEP` in main's
`crates/openmind-nerve/src/config.rs`). Main itself is not on the box; proxy =
slot 415 `3beabcb1` (373 83e573e2 + 374 d50478db, contains main's nerve commits
cde9c5e1, 16bfc28d, 9eaeadd1, 272abc42). `git merge-tree` vs that proxy:
**11 conflicted files for the candidate, the same 11 for live 9baa450c** (Cargo.lock,
desk-hermes/galion-deploy/galion-one/openmind-nerve Cargo.toml, galion-health main.rs,
nerve config.rs + evt/journal.rs, openmind-queue server.rs, repo-check README + lib.rs).
So this candidate adds no new main-land conflict; landing the nerve line on main
is a separate merge (owner D-411-2).
