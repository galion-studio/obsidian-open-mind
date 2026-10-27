<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->

# Main merge dry-run — 2026-10-27 Europe/Zagreb (CET +01:00)

Publish blocker **B1** / wave-4 slot **274** (Mate t247u).

**Throwaway only.** Did **not** push `main`. Did **not** commit to `main` or `swarm/main-staging`.
Scratch worktree: `/workspace/swarm/wt/main-merge-dryrun-scratch` (local throwaway branch, discarded after gates).
Docs branch: `swarm/main-merge-dryrun` (this file only).

## Path chosen

`staging-ff-land` (Mate t245u) had already landed the four mergefix tips onto
`swarm/main-staging` before this slot ran:

| # | Branch | Tip | Land method (on staging) |
|---|--------|-----|--------------------------|
| 1 | swarm/serde-pin-mergefix | `41373013` | `--ff-only` |
| 2 | swarm/shared-tar-mergefix | `4eca4b71` | `--no-ff` merge `4b24f4c4` |
| 3 | swarm/memory-schema-mergefix | `1919ca84` | `--no-ff` merge `beb19712` |
| 4 | swarm/no-openrouter-mergefix | `4b835a61` | `--no-ff` merge `709c9f8` |

Advanced staging tip at dry-run time: **`98466ab9`**
(`main-staging: staging-ff-land doc`, 2026-10-27 17:46:46 CET +01:00).
All four tips are ancestors of that tip.

Per slot brief: dry-run merge the **advanced tip** onto `main` `13a296c` (simpler
than re-merging the four individually).

## Merge-tree / FF result

| Check | Result |
|-------|--------|
| `main` ancestor of staging tip? | **YES** (498 commits ahead of `13a296c`) |
| `git merge-tree --write-tree 13a296c 98466ab9` | **CLEAN** tree=`86c987908dedea4c0c156aaebe19cbfc8aa722b1` |
| Throwaway merge | **`--ff-only` OK** → scratch HEAD=`98466ab9` |
| Classic conflict markers after FF | n/a (FF; tree identical to staging tip) |

## Full gates (on throwaway FF tree = staging tip `98466ab9`)

| Gate | Result | Notes |
|------|--------|-------|
| `cargo fmt --check` | **PASS** | |
| `cargo clippy --all-targets -- -D warnings` | **PASS** | ~48 s; candle-core deprecation warnings only (vendored) |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` | **PASS** | **2025 passed / 0 failed / 18 ignored** (~219 s) |
| `scripts/no-python.sh` | **PASS** | `no-python: ok` |
| `repo-check` | **PASS** | 2115 files, **0 error(s)**, 83 warning(s) (utc style) |

CPU-polite: `cargo-slot` + `-j 2`, `CARGO_TARGET_DIR=/workspace/swarm/target/main-merge-dryrun`, `nice -n 10`.
Ports **21440–21449** unused (no GALION_REAL needed for this dry-run).

## Blockers for a live main merge

Recorded 2026-10-27 17:52:33 +0100. Dry-run itself is **green*** (FF + all gates above). Remaining publish/main-merge
blockers are **outside** this tree merge (see `docs/launch/PUBLISH-BLOCKERS-20261027.md`
if present), notably:

- Owner must still **approve** and perform the real merge/push of PR #12 / `main`
  (agents must never push `main`).
- Live cortex still on older `bd6fbe6` vs merge lines; mouth 503; CF 403; etc.
  — not re-tested here.
- `swarm/main-staging` tip `98466ab9` may still need its own push from the land
  slot; this dry-run used the land worktree tip object, not a GitHub remote fetch.

## Explicit non-actions

- No push to `main`.
- No commit on `main` or `swarm/main-staging`.
- No live deploy, no CF/DNS/HADES, no secrets printed.
- No Auto-review approval requested.

## SHAs

| Ref | SHA |
|-----|-----|
| main (untouched) | `13a296ce15b5bbd66adc26f6bfae787b2c1b5190` |
| staging tip dry-runned | `98466ab9f33d3bad9b4849b0e202c400b4dbcd00` |
| merge-tree result | `86c987908dedea4c0c156aaebe19cbfc8aa722b1` |
| serde-pin-mergefix | `413730138934e4587e83683d00eea0b81080d7b2` |
| shared-tar-mergefix | `4eca4b713374c8ab4af859cd74c9e37d6eda50b0` |
| memory-schema-mergefix | `1919ca8467ebb1a715f50128c20fd345232e2fce` |
| no-openrouter-mergefix | `4b835a61e1368f810cf8630ed3803fc5f7ddac81` |
