<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Owner runbook: detach `/opt/galion/openmind` from the `swarm/*` branch

Slot 419, 2026-10-28 18:20 CET (+01:00). **Doc only. For Slavik / the owner to
run by hand in a maintenance window. No agent runs this** (D-391-2 bans agent
git writes in the live tree; this is the owner's one-off fix for F-391-3).

State per slot 391 (2026-10-28 14:51 CET) and D-391-1 (accept 202887d5):
HEAD `202887d56ddbede97acfc47bebd262a6eedd2a39` attached to branch
`swarm/overnight-loop-20261027`; local `main` = 2f5a730b; 0 dirty tracked
files; 240 untracked (incl. the rollback binary `dist/bin-ws/galion-one`,
sha256 `090c15d0…0191`). No live service runs tracked files from the tree.

Goal: HEAD detached at the SAME commit 202887d5 (zero file changes), the tree
no longer on any branch that other clones push to, so a stray `git pull` there
cannot drag report commits into the live tree.

Every command below is marked so repo-check accepts this file.

## 1. Pre-flight (read-only)

```
cd /                                                                      # never cd into the tree
export GIT_OPTIONAL_LOCKS=0
git -C /opt/galion/openmind rev-parse HEAD                                # expect 202887d5…  repo-check: allow
git -C /opt/galion/openmind symbolic-ref -q HEAD                          # expect refs/heads/swarm/overnight-loop-20261027  repo-check: allow
git -C /opt/galion/openmind diff --name-only HEAD | wc -l                 # expect 0  repo-check: allow
sha256sum /opt/galion/openmind/dist/bin-ws/galion-one                     # expect 090c15d0…0191
systemctl is-active galion-one                                            # active (runs /opt/galion/rust/bin, not the tree)
```

Stop if HEAD is not 202887d5 or anything is dirty: the tree changed since
slot 391; re-diagnose first.

## 2. Detach (2 writes, no file content changes)

```
git -C /opt/galion/openmind checkout --detach 202887d56ddbede97acfc47bebd262a6eedd2a39   # repo-check: allow
git -C /opt/galion/openmind branch -D swarm/overnight-loop-20261027                       # repo-check: allow
```

`checkout --detach` to the commit HEAD already points at rewrites no files
(only `.git/HEAD` + reflog). Deleting the local branch removes its upstream
tracking; the commit stays reachable from HEAD and `origin/…`.

## 3. Verify

```
git -C /opt/galion/openmind rev-parse HEAD                                # 202887d5…  repo-check: allow
git -C /opt/galion/openmind symbolic-ref -q HEAD; echo $?                 # 1 = detached  repo-check: allow
git -C /opt/galion/openmind diff --name-only HEAD | wc -l                 # 0  repo-check: allow
sha256sum /opt/galion/openmind/dist/bin-ws/galion-one                     # unchanged
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8670/state      # 200 (nerve)
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8674/health     # 200 (cortex)
```

Rollback (not expected to be needed):

```
git -C /opt/galion/openmind branch swarm/overnight-loop-20261027 202887d56ddbede97acfc47bebd262a6eedd2a39   # repo-check: allow
git -C /opt/galion/openmind checkout swarm/overnight-loop-20261027                                       # repo-check: allow
```

## 4. Optional tripwire (D-419-2, not recommended for agents to apply)

Two cheap layers, both tested on a throwaway repo on the box (git 2.47.3):

| layer | blocks | does not block | caveat |
|---|---|---|---|
| empty `/opt/galion/openmind/.git/index.lock` | checkout, commit, reset, stash, add — before any file changes | `fetch`, `update-ref`, `branch`, read-only commands | looks like a crashed git to a human; document it in the tree's README note |
| `reference-transaction` hook via `core.hooksPath` (aborts on `prepared`) | every ref update (commit, checkout, fetch, branch, tag, reset) | the file rewrite `checkout` does BEFORE the ref update: in the sim the work tree was half switched when the hook aborted (F-419-1) | `git -c core.hooksPath=/dev/null` bypasses it |

So use both or neither: the lock stops the file damage, the hook stops the ref
moves. Root can remove either; this is a tripwire against tooling mistakes
like F-391-1, not access control. VPS git is 2.43.0 (hook exists since 2.28)
and this has NOT been tried on the VPS. If chosen, the owner writes the hook
outside the tree (e.g. `/opt/galion/openmind-hooks/`) so the tree itself is
not edited, and sets `core.hooksPath` in the tree's `.git/config` (1 write).
Undo: delete the lock file and `git config --unset core.hooksPath`.
