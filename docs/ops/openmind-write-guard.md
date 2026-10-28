<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind write guard (D-391-2): own-clone report push + repo-check rule

Slot 419 `openmind-write-guard` (wave 4), 2026-10-28 18:02–18:25 CET (+01:00).
Branch `swarm/openmind-write-guard` off `swarm/openmind-pin-drift` 1fe15a63.
**Staged only.** Nothing was installed, applied or changed on the VPS; the
live tree `/opt/galion/openmind` was not read or touched (a simulated root on
the box stands in for it). Evidence: `docs/real-tests/openmind-write-guard.md`.

## Why

D-391-2 (approved 2026-10-28 16:05 CET): ban git writes in the live working
copy. F-391-1: the 2026-10-27 19:42 CET overnight report push used the live tree
as its clone (checkout of `swarm/overnight-loop-20261027`, commit, reset,
commit). F-391-3: the tree still tracks that `swarm/*` branch. A rule in
RULES.md alone did not stop it; this branch adds three layers:

1. **`galion-report-push`** (new crate, std only + `zagreb-time`): the report
   commit/push tool, with the guard in front of every git call. It always
   works in its OWN clone (default `/opt/galion/swarm-reports-git`).
2. **repo-check `openmind` check**: flags scripts, units, code and docs that
   `cd` / `git -C` / `GIT_DIR=` into the live tree. Git write subcommands are
   errors (CI fails), cd and read-only git are warnings.
3. **Owner runbook** `docs/ops/openmind-detach-runbook.md`: detach the live
   tree from the `swarm/*` branch, plus an optional on-box tripwire (doc only).

## 1. galion-report-push

```
galion-report-push publish --remote URL --branch swarm/X --file SRC:DEST [--file ...]
                           --message MSG [--clone DIR] [--base REF] [--push]
galion-report-push check PATH...      # exit 0 all allowed, 3 any protected
galion-report-push protected          # list protected roots
exit: 0 ok, 2 usage, 3 refused by the guard, 4 git/io error
```

What the guard refuses (exit 3), each one proven by the real test:

| attack | how it is caught |
|---|---|
| `--clone` = the tree, a subdir, `x/../openmind` | lexical normalisation + component-wise prefix (`openmind-old` is NOT caught) |
| `--clone` = symlink to the tree, or a not-yet-existing dir under such a symlink | longest existing prefix is `canonicalize`d, the rest re-appended |
| `--clone` = a `git worktree` linked to the tree, a dir whose `.git` gitfile points into it, a clone with `objects/info/alternates` into it | after the clone exists, `git rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir` + the alternates file are checked |
| `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES` into the tree | checked on entry, and stripped from every git child anyway |
| cwd inside the tree | refused (git never uses cwd here, but it is the F-391-1 smell) |
| `--remote` = the tree (path or `file://`) | refused; no cloning/fetching from the live tree |
| report `--file` source inside the tree | refused |

Also: only `swarm/*` branches (never `main`); DEST must be a plain relative
path (no `..`, no `/`, no `.git`); push is a plain fast-forward `git push`
(never `--force`); a dirty own clone from an aborted run is reset before the
commit so stray files cannot leak; token-shaped words in git stderr are
masked. The protected list always holds `/opt/galion/openmind`;
`GALION_SIM_ROOT` / `GALION_PROTECT_EXTRA` can only ADD roots (tests).

The tool does not handle credentials. On the VPS it would run with the same
`GIT_ASKPASS` arrangement `remote-push.sh` already uses (not changed here).

Builds: x86_64 musl 512,688 B, aarch64 musl 446,528 B (release, static);
aarch64-android + armv7 musl `cargo check` ok.

## 2. repo-check `openmind`

`repo-check --list` now shows 10 checks. Rule (all text files; a line with
`repo-check: allow` is skipped):

* **error**: `git -C <tree> <write>`, `git --git-dir/--work-tree <tree> <write>`,
  `git clone ... <tree>`, `GIT_DIR=` / `GIT_WORK_TREE=` / `GIT_COMMON_DIR=` set
  to the tree, `cd <tree> && git <write>` on one line, and in `.sh` files a
  `cd <tree>` stays in force on later lines until the next `cd`/`popd`, so
  `git commit` two lines down is an error too (the overnight-push shape).
* **warn**: `cd` / `pushd` / `WorkingDirectory=` / `.current_dir("…")` into the
  tree, and read-only git there (`rev-parse`, `log`, `status`, `remote -v`,
  `branch --show-current`, `config --get`, …; anything not on the read list
  counts as a write).

Result on this branch: **0 errors, 9 openmind warnings** (2,132 files). It found
5 errors on the base tree, all fixed here with an allow marker and reason:
`TEACHER/BOSS.md:75` (already "SUPERSEDED", `git -C … add/commit`),
`TEACHER/LESSON.md:48` (historical lesson, now prefixed SUPERSEDED by
D-391-2), `docs/ops/openmind-pin-drift.md:99` (rejected option B), and
`release/TEACHER/CHECKS.md:17` (`git remote -v` was a false write; fixed in
the rule, it is a read now). The 9 warnings are the real cd/rev-parse mentions
(TEACHER setup lines, the staged `deploy/console-prod.service`
`WorkingDirectory=`, slot 391's read-only evidence).

Limits: a git target held in a shell variable (`git -C "$REPO" commit`) is not
resolved; a cd in a doc code block does not carry to the next line.

## Owner decisions opened

* **D-419-1 — adopt galion-report-push for the overnight report push.**
  (A, recommended) after review/merge, build the static musl binary and switch
  the overnight report step to `galion-report-push publish --clone
  /opt/galion/swarm-reports-git --branch swarm/overnight-loop-YYYYMMDD …` in a
  normal deploy window; (B) keep `push-branch.sh` bundles from the box only
  (no VPS-side clone at all); (C) leave as is (RULES text only).
* **D-419-2 — on-box tripwire for the live tree** (see runbook §4): (A) none,
  rely on tooling + repo-check; (B, recommended) `index.lock` + a
  `reference-transaction` hook via `core.hooksPath`; (C) hook only. Sim test
  on git 2.47: the hook alone aborts every ref update but `checkout` has
  ALREADY rewritten files when it aborts (half state, F-419-1); the
  `index.lock` stops checkout/commit/reset before any file changes. Both are
  bypassable by root (`rm`, `-c core.hooksPath=`), so they are a tripwire,
  not a lock. VPS git is 2.43.0 (hook exists since 2.28; not tested there).
* **D-419-3 — repo-check level for read-only git into the tree**: keep warn
  (recommended) or make it an error too.
* **D-419-4 — TEACHER files**: keep the allow-marked superseded lines
  (recommended, history stays readable) or rewrite those lessons.
