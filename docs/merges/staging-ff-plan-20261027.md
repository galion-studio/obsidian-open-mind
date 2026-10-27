<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->

# Staging fast-forward / merge PLAN — 2026-10-27 Europe/Zagreb (CET +01:00)

**Plan only. This slot made NO commits to `main-staging` or `main`.** Owner/parent executes.
Source: `docs/merges/staging-ff-mergechecks-20261027.md` on `swarm/staging-ff-mergechecks` 20cbdcc8 (slot 256 DONE 2026-10-27 17:36:43 CET +01:00).
Base: `swarm/main-staging` **7c95348b** (untouched).

> **STAGING ONLY. Do NOT merge PR #12 / do NOT touch `main` (still 13a296c) as part of this plan.**

## 1. Recommended order

| # | Branch | Tip | Method | Mergecheck |
|---|--------|-----|--------|------------|
| 1 | swarm/serde-pin-mergefix | 41373013 | `--ff-only` | FF + CLEAN (tree dc29a948) |
| 2 | swarm/shared-tar-mergefix | 4eca4b71 | **merge** (see caveat) | FF vs 7c95348b + CLEAN (tree 3464a2b9) |
| 3 | swarm/memory-schema-mergefix | 1919ca84 | `--no-ff` merge | NOT-FF, merge-tree CLEAN (f5d2e7f6) |
| 4 | swarm/no-openrouter-mergefix | 4b835a61 | `--no-ff` merge | NOT-FF, merge-tree CLEAN (f841ba72) |

**Caveat found while planning (17:4x CET):** serde-pin and shared-tar are each FF of 7c95348b, but **neither is an ancestor of the other**. Only the first one landed can be a true fast-forward; the second becomes a regular merge. Pairwise `git merge-tree --write-tree 41373013 4eca4b71` is **CLEAN** (tree 74f589e7), so expect no conflicts. Alternative: rebase shared-tar onto serde-pin in its own branch to keep history linear (needs a new slot; not done here).

## 2. Command checklist (run in a dedicated integrator worktree, NOT /opt/galion/openmind)

```
# [ ] 0. setup
git clone -q /workspace/swarm/base.git /workspace/swarm/wt/staging-integrate-20261027
cd /workspace/swarm/wt/staging-integrate-20261027
git config user.name  "<integrator>"; git config user.email "<integrator email>"
git fetch -q origin
git checkout -q -B swarm/main-staging origin/swarm/main-staging
test "$(git rev-parse --short=8 HEAD)" = 7c95348b || echo "STOP: staging moved"

# [ ] 1. serde-pin (true FF)
git merge --ff-only 41373013            # expect HEAD=41373013

# [ ] 2. shared-tar (merge; no longer FF after step 1)
git merge --no-ff 4eca4b71 -m "main-staging: merge swarm/shared-tar-mergefix 4eca4b7"

# [ ] 3. memory-schema (merge)
git merge-tree --write-tree HEAD 1919ca84   # re-check: expect clean
git merge --no-ff 1919ca84 -m "main-staging: merge swarm/memory-schema-mergefix 1919ca8"
git grep -nE '^(<<<<<<<|>>>>>>>)' -- . ':!vendor' || echo "no markers"

# [ ] 4. no-openrouter (merge)
git merge-tree --write-tree HEAD 4b835a61
git merge --no-ff 4b835a61 -m "main-staging: merge swarm/no-openrouter-mergefix 4b835a6"

# [ ] 5. gates (CPU-polite)
export CARGO_TARGET_DIR=/workspace/swarm/target/staging-integrate-20261027
cargo fmt --check
nice -n 10 cargo clippy --all-targets -j 4 -- -D warnings
nice -n 10 cargo test --workspace -j 4 --exclude openmind-sandbox --exclude cgroup-detect
scripts/no-python.sh 2>/dev/null || true

# [ ] 6. push staging (owner decision — RULES: owner merges)
/workspace/swarm/push-branch.sh /workspace/swarm/wt/staging-integrate-20261027 swarm/main-staging
```

## 3. What each tip brings

- **serde-pin 41373013** — pins serde ordering across workspace Cargo.toml + `repo-check` deps_pin rule; adds `real_serde_order` test and decision doc. Slot 240 gates green (1966/0/18; real 3/3).
- **shared-tar 4eca4b71** — new `galion-tar` crate (ustar/GNU read+write), galion-backup and galion-deploy switched to it (−568 lines dup code); `gnu_oracle` 2/2, `real_parity` 26/26.
- **memory-schema 1919ca84** — openmind-memory schema mergefix (slot 239); diverged before later staging history (staging↔tip 472/2, mb 723b46c8); would stage 12 paths.
- **no-openrouter 4b835a61** — removes OpenRouter paths (no paid API); `real_no_openrouter` 2/2; only behind staging by the 7c95348 CI-fix commit; would stage 12 paths.

## 4. Known risks / conflicts

- **memory-schema:** a classic `<<<<<<<` marker grep hit 1 in mergecheck, yet `write-tree` is clean — likely a pre-existing literal (fixture/doc). Inspect the grep hit after step 3 before pushing.
- **no-openrouter:** tip already contains merge 7e2a12e; merge-base is 7e2a12e1, so it re-merges only against 7c95348 CI fixes. Low risk; clean in slot 241 and slot 256.
- **serde-pin ↔ shared-tar:** sibling branches (see caveat); pairwise clean, but `Cargo.lock` / Cargo.toml both touched — rerun clippy+tests after step 2.
- Slot 256's first `--no-ff` "MERGE-CONFLICTS" lines were a mis-log (missing committer identity); corrected re-run = MERGE-OK. Set `user.email` in step 0.
- Combined 4-way tree has NOT been gate-tested yet; step 5 is mandatory.

## 5. Explicit scope

- This is **staging only**. Do **not** merge PR #12, do **not** push or merge to `main`.
- No VPS writes, no cortex/console.service changes, no live service restarts.
