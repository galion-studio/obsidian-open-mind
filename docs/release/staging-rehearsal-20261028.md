<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->

# Staging rehearsal 2026-10-28 (wave-4 slot 407, throwaway)

Branch `swarm/staging-rehearsal-1028`. Nothing here updates `main` or
`swarm/main-staging`; nothing is deployed; the VPS was only read.
Times Europe/Zagreb CET (+01:00).

Branches rehearsed (all pushed today):

| Slot | Branch head | What it is | Touches live code? |
|---|---|---|---|
| 388 | `1d7164b4` memory-db-stale-diag | openmind-memory freshness sensor + zagreb-time helper | openmind-memory (not redeployed tonight) |
| 389 | `d25f8479` survival-refresh-writer | galion-survival writer + console-static on-request refresh, staged drop-in (D-389-1, NOT installed) | console-static/console-ui source (live console is 5aa31927, without 389) |
| 386 | `9ebbed3a` nerve-xt-rustlld-pin | `.cargo/config.toml` x86_64 musl `linker = "rust-lld"` + real_cross_target live pin | build config + nerve test only |
| 383 | `26a526c1` console-realtests-rust-routes | `galion_realtest::ports::Block` port-base env for many GALION_REAL tests | tests + dev-deps only |
| 387 | `6e96ee54` cortex-handoff-exclude | gaia-cortex `hermes.exclude_units` + staged drop-ins; brings the cortex lineage incl. live cortex `58a12e61` | gaia-cortex source (live cortex 58a12e6 is an ancestor of 387) |

## Phase 1 (17:19-18:06): onto old main-staging 98466ab9

Order 388 -> 386 -> 383 -> 389 -> 387, full gates after every step
(fmt, clippy `-D warnings`, `cargo test --workspace --exclude openmind-sandbox
--exclude cgroup-detect --no-fail-fast`, no-python, Cargo.lock check).
Box logs: `/workspace/swarm/logs/r407/summary.txt`.

| Step | Head | fmt | clippy | tests pass/fail/ign (suites) | Notes |
|---|---|---|---|---|---|
| S0 base | 98466ab9 | ok | ok | 2025/0/18 (384) | |
| S1 +388 | 5ed67a04 | ok | ok | 2038/0/18 (387) | clean merge |
| S2 +386 | 1fbd6c1c | ok | ok | 2048/0/18 (388) | clean merge |
| S3 +383 | fc5396d0 | ok | ok | 2051/0/18 (388) | clean merge |
| S4 +389 | b5eac9c7 | ok | ok | 2122/**2**/18 (399) | 3 conflicted files; 2 semantic failures |
| S4 fix | ceddad5b | ok | ok | 2124/0/18 (399) | F-407-1 |
| S5 +387 | 1ce5c49f/3aeab4f5 | ok | ok | 2278/**9**/18 (427) | 19 conflict blocks in 8 files; F-407-2 |
| S5 fix | 3f44b649 | ok | ok | 2287/0/18 (427) | F-407-3 (corpus re-bless), carry 9cef9ff7 + e93c1725 |

Phase-1 findings: F-407-1 (port2 fallbacks test expects no SPA fallback;
galion-survival not mapped in gaia-core ownership), F-407-2 (E0063
`Limits.evict_idle` in hermes-rs/openmind-memory after the http.rs union;
cortex crates not mapped), F-407-3 (replay corpus needed the gated
cortex-staging-land-gate 186cd61c re-bless).

## What changed at 18:09: PR #12 merged, main = 7ba85b60

PR #12 merged `swarm/main-staging` 3250d07d into `main` (merge `7ba85b60`,
tree `a6f6c163`). main now already holds 373 + 374 + the cortex-staging lineage
(186cd61c, 9cef9ff7, e93c1725) + console 5aa31927 + PR #13 CI fixes. Live core
stack was already cut over (console 5aa31927, cortex 58a12e6, nerve 9baa450).
So phase 1's base is obsolete: the real target is main.

### Reconstructing main on the box (no GitHub fetch path on the box)

base.git and the box have no copy of 7ba85b60. Slot 407 rebuilt it locally
and proved it equal:

1. `0cfb1599` (pr12-ci-green, local) + no-ff merge of 98466ab9, then the
   patches of `5f365368` and `48744d50` read from the GitHub API
   -> tree `573e73b8`, then `4864589f` = GitHub's trees for those commits.
2. PR #13 merge (98466ab9 + that tip) = tree `4864589f`.
3. Merge 373 `83e573e2` + 374 `d50478db` (both local), patches of
   `811797a9`, `09fb719c`; three files whose conflict resolutions exist only
   on GitHub (`.github/workflows/self-eval.yml`, `scripts/size-budget.sh`,
   `scripts/ucpu-gate.sh`) copied from main via the API and checked by
   `git hash-object` against GitHub's blob ids.
4. Every one of 2391 blobs (path + mode + id) matched GitHub's
   recursive tree listing of `a6f6c163`; `git write-tree` = `a6f6c163`.

Local commit `6966c8da` (branch-local name `recon-main-7ba85b60`) has exactly
main's tree. Different SHA from 7ba85b60, same content.

## Phase 2 (18:36-): onto main 7ba85b60 (tree-equal)

`git merge-tree` of each branch alone onto main:

| Slot | Conflicted files alone onto main |
|---|---|
| 388 1d7164b4 | 0 |
| 389 d25f8479 | 0 |
| 386 9ebbed3a | 2 (.cargo/config.toml, nerve real_cross_target.rs) |
| 383 26a526c1 | 4 (Cargo.lock, real_landing.rs, hermes-gateway Cargo.toml, real_cross_target.rs) |
| 387 6e96ee54 | 3 (replay-corpus corpus.json + expected live-replay200/207) |
| phase-1 tip 3f44b649 | 9 (do NOT land the phase-1 tip; land the branches) |

Order used and proposed: **388 -> 389 -> 386 -> 383 -> 387**
(the two clean ones first, then the build-config pin, then the test-port
sweep, then the cortex lineage last because it is the largest and carries the
replay corpus).

| Step | Commit | Conflicts and resolution |
|---|---|---|
| +388 | cee8d86f | clean |
| +389 | 621b6e2d | clean (Cargo.lock auto-merged) |
| +386 | 86bf3b88 | `.cargo/config.toml`: union of the slot-329 and slot-386 comments over the same `[target.x86_64-unknown-linux-musl] linker = "rust-lld"` + `[alias] xtask`. `real_cross_target.rs`: 386 side (live pin + `SIZE_BUDGET` table); main's 16bfc28d intent (budget from `config/release/size-budget.tsv`) kept as new test `budget_matches_size_budget_tsv` |
| +383 | 4d4a9934 | `real_landing.rs`: 383's `ports::Block` kept with default base = main's slot-254 block 21230 (unset env still binds 21232/21233); hermes-gateway dev-deps union (main `rusqlite 0.32` + 383 `galion-realtest`); Cargo.lock regenerated offline, 0 registry changes |
| +387 | 7cdf8ad4 | replay corpus: main side kept; byte-identical to 186cd61c (the phase-1 F-407-3 re-bless is already on main) |
| F-407-4 | 913eca57 | galion-survival (from 389) mapped in gaia-core ownership (Survival, pending Slavik) + counts 73/61 -> 74/62 |

F-407-2 (evict_idle, cortex crates mapped) is NOT needed on main: main got
those with the cortex-staging lineage.

Gate results: see `docs/real-tests/staging-rehearsal-1028.md`.

## Live vs main after this land order

| Service | Live binary (VPS read 18:49) | In main 7ba85b60? | In main + 5 (this rehearsal)? |
|---|---|---|---|
| console | galion-console-serve-5aa31927 | yes | yes; 389 adds console-static survival refresh (not live until D-389-1 + a console redeploy) |
| cortex | gaia-cortex-58a12e6-llm (sha256 7ca6512b..., 1 147 680 B) | **no** | yes (58a12e61 is an ancestor of 387) |
| nerve | openmind-nerve-9baa450 (sha256 8c9d7a5f..., 1 479 456 B) | **no** | **no**: 9baa450c (nerve-merge-6, slot 333) is a separate lineage; `merge-tree` of it onto this rehearsal tip lists 13 files. Slot 411 nerve-merge-7 owns that land. |

F-407-6: 387's `real_handoff_exclude` fails on this tree (also in phase 1):
the exclusion works (0 test tickets, 10/10 escalations excluded, control
unit handed to Hermes), but the test expects 2 control tickets (R4 + R2) and
the nerve in this tree marks the R2 one `duplicate`, so there is 1. 387
passed it against nerve 9d1a4f3. See D-407-4.

F-407-7: after +389 (and still after +386) `Cargo.lock` is re-sorted by
cargo on build (git auto-merge put `galion-survival` out of order; 0 version
or registry changes). The +383 step regenerates it offline and it stays
clean from there; when landing, run `cargo metadata --offline` (or any
build) after 389 and commit the re-sorted lock.

So landing 387 makes main hold the live cortex source; main still lacks the
live nerve source until the 9baa450c lineage lands (slot 411).

F-407-5: 386's `NERVE_XT_PIN=live` default still pins nerve 9d1a4f3
(d0c70b87..., 1 458 760 B). That nerve is no longer live (9baa450 since
17:40 CET). The pin test is still a valid reproducibility proof for
9d1a4f3, but its "live" name is now stale: re-pin to 9baa450c
(8c9d7a5f..., 1 479 456 B) once the 9baa450c lineage is on the branch
(it needs that commit in the repo for `git archive`).

## Proposed land order (owner decision D-407-1)

Onto a fresh staging branch off main 7ba85b60 (or `swarm/main-staging`
only if it can move forward to main without a reset; never a force-push):

1. 388 `1d7164b4` (clean)
2. 389 `d25f8479` (clean) + F-407-4 ownership row (913eca57's diff)
3. 386 `9ebbed3a` with the 86bf3b88 resolution
4. 383 `26a526c1` with the 4d4a9934 resolution
5. 387 `6e96ee54` with the 7cdf8ad4 resolution (take main's corpus)
6. then slot 411's live-nerve lineage, and re-pin F-407-5.

Simplest mechanical path: land this branch's phase-2 commits
`cee8d86f..913eca57` (they sit on a tree-equal copy of main; replay them on
the real 7ba85b60 with `git cherry-pick -m 1` per merge, or redo the merges
with the same resolutions; the resolutions are small and listed above).

## Owner decisions

- D-407-1: approve the land order above onto a staging branch off main 7ba85b60 (PR to main after CI).
- D-407-2: who lands the live-nerve lineage 9baa450c (slot 411) so main == live for all three core services.
- D-407-3: re-pin 386's live nerve test to 9baa450c after D-407-2 (F-407-5).
- D-407-4: fix 387's real_handoff_exclude control assertion (F-407-6): with the nerve in main + 5 the control unit's R2 escalation is de-duplicated against its R4 ticket, so 1 control ticket, not 2. Either assert `>= 1` (handed) or pin the test to a nerve with the old per-rule tickets.
- Carried: D-389-1 survival refresh drop-in + console redeploy (not done here).
