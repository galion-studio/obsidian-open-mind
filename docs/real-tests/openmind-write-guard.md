<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: openmind write guard (slot 419)

Box, 2026-10-28 18:08–18:25 CET (+01:00), branch `swarm/openmind-write-guard`.
Simulated root only: `<sim>` = `target/openmind-write-guard/tmp/sim-419-<pid>`
with `<sim>/opt/galion/openmind` a real git work tree checked out on
`swarm/overnight-loop-20261027` (the F-391-3 shape) and `<sim>/srv/origin.git`
as the remote. The real `/opt/galion/openmind` was not read or touched on the
box or the VPS. No ports used (git over local paths); assigned range
22890-22899 stayed free.

## 1. galion-report-push, release binary, GALION_REAL=1

```
GALION_REAL=1 cargo-slot test -j 2 --release -p galion-report-push --test real_guard -- --nocapture
attack clone = live tree                  exit 3 | galion-report-push: refused: clone dir <sim>/opt/galion/openmind is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack clone = live subdir                exit 3 | galion-report-push: refused: clone dir <sim>/opt/galion/openmind/docs is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack clone = live/../openmind           exit 3 | galion-report-push: refused: clone dir <sim>/opt/galion/openmind/../openmind is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack clone = symlink to live            exit 3 | galion-report-push: refused: clone dir <sim>/link-to-live is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack clone = new dir under symlink      exit 3 | galion-report-push: refused: clone dir <sim>/link-to-live/new-clone is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack clone = linked worktree of live    exit 3 | galion-report-push: refused: git dir <sim>/opt/galion/openmind/.git/worktrees/linked-wt is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack clone = alternates into live       exit 3 | galion-report-push: refused: alternates <sim>/opt/galion/openmind/.git/objects is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack clone = gitfile into live          exit 3 | galion-report-push: refused: git dir <sim>/opt/galion/openmind/.git is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack GIT_DIR into live                  exit 3 | galion-report-push: refused: GIT_DIR <sim>/opt/galion/openmind/.git is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack GIT_WORK_TREE into live            exit 3 | galion-report-push: refused: GIT_WORK_TREE <sim>/opt/galion/openmind is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack GIT_ALTERNATE_OBJECT_DIRECTORIES   exit 3 | galion-report-push: refused: GIT_ALTERNATE_OBJECT_DIRECTORIES <sim>/opt/galion/openmind/.git/objects is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack cwd inside live                    exit 3 | galion-report-push: refused: cwd <sim>/opt/galion/openmind/docs is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack remote = live tree                 exit 3 | galion-report-push: refused: remote <sim>/opt/galion/openmind is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack remote = file:// live              exit 3 | galion-report-push: refused: remote <sim>/opt/galion/openmind/.git is inside protected tree <sim>/opt/galion/openmind (D-391-2: no git writes there; use your own clone)
attack report source in live              exit 3
check real paths exit 3:
refused  /opt/galion/openmind
refused  /opt/galion/openmind/.git
ok       /opt/galion/swarm-reports-git
publish #1 exit 0 42 ms: committed swarm/overnight-loop-sim 4ff4178ca9dbc76963003bc71099646b996478c9 in <sim>/opt/galion/swarm-reports-git (pushed)
origin tip 4ff4178ca9dbc76963003bc71099646b996478c9: Grok Bot <grokbot@galion.studio>|overnight: loop log (slot 419 sim) (2026-10-28T18:11:47+01:00)
publish #3 exit 0: committed swarm/overnight-loop-sim 6393fc8f8f9fb421fa9933546da763f03d2026f3 in <sim>/opt/galion/swarm-reports-git (pushed)
RESULT refused 16 attacks; sim live tree 65 entries unchanged; HEAD ref: refs/heads/swarm/overnight-loop-20261027; publish #1 42 ms; own clone <sim>/opt/galion/swarm-reports-git
test real_openmind_write_guard_sim_root ... ok
```

After every attack and all publishes, the sim live tree was fingerprinted
again (path, size, mtime ns, content hash of every entry incl. `.git`):
65 entries, 0 changed, HEAD still `ref: refs/heads/swarm/overnight-loop-20261027`.
Happy path checks: commit author `Grok Bot <grokbot@galion.studio>`, message
carries a `+01:00` Zagreb stamp, file content exact; re-run with same content
= "nothing to commit"; another clone pushes in between -> our run builds on
top (rev-list count 4: main + run1 + other + run2, fast-forward, nothing
lost); a dirty own clone (modified README + stray file) does not leak (the
commit touches only `swarm/reports/overnight-loop-log.md`); commit without
`--push` leaves origin unchanged; origin `main` untouched (1 commit). Usage
refusals exit 2: branches `main`, `refs/heads/main`, `swarm/a..b`; DEST
`../x`, `.git/hooks/pre-commit`.

Unit tests: 9/9 (guard: live tree/children/`..`/relative, siblings
`openmind-old` and `swarm-reports-git` allowed, env redirects, remotes,
additive roots, lexical; lib: file maps, branches, redaction).

aarch64 musl: unit tests 9/9 under `qemu-aarch64-static`; the binary
`check /opt/galion/openmind/docs /opt/galion/swarm-reports-git` -> refused /
ok, exit 3. The aarch64 real test cannot spawn its aarch64 child without
binfmt on the box (panics at spawn, not a guard result), so the full sim run
is x86_64 only.

## 2. repo-check `openmind`, release binary

```
GALION_REAL=1 cargo-slot test -j 2 --release -p repo-check --test openmind_rule -- --nocapture
error openmind scripts/push-report.sh:3: cd into the live tree, then git with write subcommand `checkout` (D-391-2); use your own clone
error openmind scripts/push-report.sh:3: cd into the live tree, then git with write subcommand `commit` (D-391-2); use your own clone
error openmind scripts/two.sh:1: git -C/--git-dir/--work-tree into the live tree with write subcommand `push` (D-391-2); use your own clone
warn  openmind scripts/ok.md:1: git -C/--git-dir/--work-tree into the live tree, read-only `rev-parse`; set GIT_OPTIONAL_LOCKS=0
warn  openmind scripts/push-report.sh:2: working directory set to the live tree; no git writes there (D-391-2)
repo-check 2026-10-28T18:24:10+01:00: 4 files, 3 error(s), 2 warning(s) -> FAIL
test result: ok. 8 passed; 0 failed
```

Whole tree (2,132 files) on this branch: `0 error(s), 92 warning(s) -> PASS`
(openmind: 0 errors, 9 warnings). On the base 1fe15a63 the new rule gave 5
errors (TEACHER/BOSS.md:75 x2, TEACHER/LESSON.md:48, docs/ops/openmind-pin-drift.md:99,
release/TEACHER/CHECKS.md:17 `git remote -v` = rule bug, fixed). The rule also
caught this branch's own runbook rollback line until it got its allow marker.

## 3. Tripwire experiments (throwaway repo in /tmp, git 2.47.3)

| setup | commit | checkout other branch | checkout --detach | reset --hard | branch / tag / fetch | file content after checkout |
|---|---|---|---|---|---|---|
| `reference-transaction` hook (abort on `prepared`) | 128 | 128 | 128 | 128 | 128 / 128 / 128 | **rewritten** (`f` = main's content, staged M) — half state, F-419-1 |
| empty `.git/index.lock` | 128 | 128 | – | 128 | update-ref 0, fetch 0 | unchanged |
| hook bypass `-c core.hooksPath=/dev/null` | – | 0 | – | – | – | – |

## 4. VPS read-only observation (nerve/cortex, no tree access)

```
2026-10-28 18:11:58 +0100
http://127.0.0.1:8670/state 200 32133B        (nerve; open_incidents 2, first cause "edge")
http://127.0.0.1:8674/health 200 235B         (cortex: mode live, ok true, incidents_open 2, escalations_open 0)
/opt/galion/swarm-reports-git: No such file or directory   (D-419-1 would create it)
git version 2.43.0                             (>= 2.31 needed for rev-parse --path-format)
```

Nothing written on the VPS. Cortex is live; the 2 open incidents are edge
(out of scope here).

## 5. Gates

* `cargo fmt --all --check` ok; `cargo clippy --all-targets -D warnings` on
  galion-report-push, repo-check, gaia-core, gaia: clean.
* `cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect`
  (debug): 389 suites, 2,042 passed, 1 failed, 18 ignored. The failure was
  `gaia --test commands ownership_counts` (crates_total 68 -> 69, system
  crates 56 -> 57: the new crate); count bumped, `-p gaia -p gaia-core` rerun
  all green. gaia-core `every_workspace_crate_is_mapped` needed the new
  ownership row (repo-check / MINERVA, pending Slavik).
* `scripts/no-python.sh` ok; repo-check PASS (0 errors).
* Builds: x86_64 musl 512,688 B, aarch64 musl 446,528 B; aarch64-android and
  armv7 musl `cargo check` ok.
* Not run: sandbox tests (sandbox_vps_only; this branch does not touch the
  sandbox). Pre-existing: `gaia/tests/real_cli.rs:125` still asserts
  `crates_total 63` (GALION_REAL-gated, stale before this branch; not changed).
