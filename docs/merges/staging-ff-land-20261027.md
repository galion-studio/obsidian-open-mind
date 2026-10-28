<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->

# Staging FF land — 2026-10-27 Europe/Zagreb (CET +01:00)

Landed mergefix branches onto `swarm/main-staging` per
`docs/merges/staging-ff-plan-20261027.md` (6fc280f4) and
`docs/merges/staging-ff-mergechecks-20261027.md` (20cbdcc8).
Slot **staging-ff-land** / Mate t245u. **Staging only** — PR #12 / `main` untouched.

## Tips

| | SHA |
|---|---|
| Old tip | `7c95348b` |
| Tip after four merges | `709c9f8dd2f250a427b74cd7bfebaccd06a3bcaa` (`709c9f8d`) |
| Land doc commit | tip of `swarm/main-staging` containing this file |

## Order executed

| # | Branch | Tip | Method | Result |
|---|--------|-----|--------|--------|
| 1 | swarm/serde-pin-mergefix | 41373013 | `--ff-only` | FF → HEAD=41373013 |
| 2 | swarm/shared-tar-mergefix | 4eca4b71 | `--no-ff` | merge `4b24f4c4` (pair tree was 74f589e7 CLEAN) |
| 3 | swarm/memory-schema-mergefix | 1919ca84 | `--no-ff` | merge `beb19712`; marker scan 0; merge-tree CLEAN |
| 4 | swarm/no-openrouter-mergefix | 4b835a61 | `--no-ff` | merge `709c9f8d`; merge-tree CLEAN |
| 5 | (this doc) | — | commit | tip of branch |

Base was still `7c95348b` (not moved) so plan FF vs `--no-ff` logic held: only serde-pin could true-FF.

## Gates (box, `CARGO_TARGET_DIR=/workspace/swarm/target/staging-ff-land`)

| Gate | Result |
|---|---|
| cargo fmt --check | PASS |
| cargo clippy --workspace --all-targets -j4 --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings | PASS (candle-core vendor deprecated warnings only) |
| cargo test --workspace -j4 --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools | 377 suites, 2002 passed / 1 failed / 18 ignored |
| hades-gate simd_free curve25519_backend_is_serial | FAIL only when RUSTFLAGS env replaced `.cargo/config.toml` serial cfg; re-run without override **PASS** (pre-existing env note, not a merge break; hades-gate untouched by land) |
| scripts/no-python.sh | PASS |
| repo-check | PASS 0 err / 83 warn (2115 files) |
| Landed crates recheck (galion-tar, openmind-memory, repo-check, one-set-model, openmind-nerve lib+no_openrouter, galion-backup/deploy/gaia-core lib) | all PASS |

## Scope

- No merge of PR #12 to `main`.
- No live cortex / nerve / console deploy.
- No CF DNS / HADES. No VPS IP in tree. Secrets not printed.
- Owner decisions still open: D-241-1/2 (no-openrouter), D-242-1 (galion-tar ownership).
