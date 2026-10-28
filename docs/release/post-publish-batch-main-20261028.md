<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Post-publish test-only batch on top of main (wave-4 slot 421), 2026-10-28 20:39-20:55 CET (+01:00)

D-415-1 prep. PR #12 is merged: GitHub main **7ba85b60** = merge of 13a296ce + swarm/main-staging
3250d07d (publish 2026-10-28). This branch carries the slot 415 test-only batch
(`swarm/post-publish-test-batch` **18d68395**) as one follow-up on top of main.
**main, main-staging and PR #12 were NOT updated. No PR opened. Nothing deployed. No VPS change.**

## How main got onto the box (the gap)

7ba85b60 and its main-staging parents (b4fe2383, 811797a9, 09fb719c, 3250d07d, PR #13 38a07700) are
not in any repo on the box, and GitHub cannot be fetched from the box without credentials. So:

1. Read-only GitHub compare `83e573e2...7ba85b60` (20 commits, 13 files, patches + blob ids).
2. Applied those 13 patches to 83e573e2 and committed **2ce1a08f**. Its tree is
   **a6f6c163101d2e8cedcf4919a6fbf25ef415cd81 = the tree of main 7ba85b60** (all 13 blob ids
   checked one by one). Only the commit id differs (parent 3250d07d is missing on the box).
3. Merged 18d68395 onto it: **96e24ce2**, 0 conflicts, tree 19d5c983.

What main has beyond 373+374 (3beabcb1, the base the batch was tested on) is only 5 files,
38 lines: `.github/workflows/self-eval.yml`, `.gitleaks.toml`, `console-static/tests/deploy_units.rs`
(SPDX split), `gaia-cortex/src/dispatch.rs` (SPDX split in the proposal header, format string kept
literal), `galion-common/tests/http_evict.rs` (miri ignore). **0 files overlap** with the batch's
79 files.

## Land order (owner / parent, after approval)

Either way gives the same tree **19d5c983**:

- **A (merge):** on real main 7ba85b60, `git merge --no-ff swarm/post-publish-batch-main`.
  Rehearsed with `git merge-tree` against throwaway commits that have tree a6f6c163 and main's
  ancestry (merge bases 83e573e2 alone, and 83e573e2 + d50478db criss-cross): 0 conflicts, tree
  19d5c983 both times. The history then holds the duplicate 2ce1a08f reconstruction commit
  (same content as main, empty in effect).
- **B (replay, cleaner history):** on a branch from real main, apply `git diff 2ce1a08f 96e24ce2`
  (the batch only: 79 files, +3899/-842) and commit. Recommended if history should not show 2ce1a08f.

Then re-run the gates in CI (GALION_REAL tests stay out of CI). D-415-2 is kept:
`tsv_budget()` in `openmind-nerve/tests/real_cross_target.rs` is the nerve hard ceiling and
`size_budget_table` asserts `BUDGET <= tsv_budget()`.

## Gates (box, CET +01:00)

| gate | result |
|---|---|
| `cargo fmt --all --check` | ok (20:41) |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok (20:41-20:42) |
| `env -u GALION_REAL cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` x3 | **2232 / 0 failed / 18 ignored, 424 suites, all 3 runs** (20:42-20:45, 20:45-20:47, 20:47-20:48) = slot 415 numbers |
| `scripts/no-python.sh` | ok |
| `scripts/spdx-scan.sh` | 817 scanned, 0 missing |

Sandbox crates are excluded locally (sandbox_vps_only); this branch changes no sandbox code, so no
VPS sandbox run was needed. hermes-tools excluded as in slot 415 (landlock tool runs).

Real-life evidence: `docs/real-tests/post-publish-batch-main.md`.

## Known, not caused by this batch

F-383-5 (`real_enroll` 71/72), F-383-1..10, F-375-1 / D-375-1. Live nerve self_eval 68 D with
tuner-quick failing (slot 420, D-420-1/2).
