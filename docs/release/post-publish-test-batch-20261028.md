<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Post-publish test-only batch: land order (wave-4 slot 415), 2026-10-28 18:02-18:25 CET (+01:00)

Publish-path step 3 (`docs/release/PUBLISH-PATH-20261028.md`, slot 392 0d4568bd): land 373 + 374,
then fold the test-only branches 375, 376, 383, 386, 390 as a **separate batch after the publish**.
This branch rehearses that whole batch in one throwaway tree.
**swarm/main-staging, main and PR #12 were NOT updated. Nothing deployed. No VPS change.**

Branch `swarm/post-publish-test-batch`, base **83e573e2** (slot 373 `swarm/land-order-gateway-size`).

## Order rehearsed (first-parent chain)

| step | merged | conflicts and resolution | commit | tree |
|---|---|---|---|---|
| 1 | 374 pr12-flake-bundle **d50478db** | none | 3beabcb1 | 76ece348 (= slot 392 `git merge-tree` result) |
| 2 | 375 real-ports-batch-2 **7c3de266** (brings 363 0dfc3712..0d545118) | `console-static/tests/real_landing.rs`: 375's `ports::Block` (`LANDING_REAL_PORT_BASE`) kept, default base = 373's slot-254 block **21230** (375 had 19400), 373's two-stylesheet asserts kept. `openmind-nerve/tests/real_cross_target.rs`: 373's `budget()` from `config/release/size-budget.tsv` kept, 375's `const BUDGET` dropped; 375's `NERVE_XT_PORT_BASE` block + `NERVE_XT_LIVE_PIN` kept, 373's `PORT_SVC/PORT_DEAD` dropped | ad78d771 | 3e90be7f |
| 3 | 376 nerve-xt-pin-refresh **52aedbc7** | `real_cross_target.rs`: 376's pin modes (budget / live / aa77864), LIVE_* = 9d1a4f3 and `SIZE_BUDGET` warn table taken; 373's table lookup kept as `tsv_budget()`: hard check is `size <= tsv_budget()`, and `size_budget_table` now asserts `BUDGET <= tsv_budget()` so the two tables cannot drift | a8e36a97 | 9518a064 |
| 4 | 383 real-ports-batch-3 **26a526c1** | `hermes-gateway/Cargo.toml` + `Cargo.lock`: union of 373's `rusqlite` dev-dep (real_size_2mib) and 383's `galion-realtest` dev-dep (`cargo metadata --locked` ok) | 187ba0df | 3ff4d1b7 |
| 5 | 386 nerve-xt-rustlld-pin **9ebbed3a** | `.cargo/config.toml`: **373's copy unchanged** (x86_64 musl rust-lld line from slot 329); 386's duplicate `[target.x86_64-unknown-linux-musl]` block and its comment dropped (D-376-2 / D-386-1). `real_cross_target.rs` merged clean | 66544869 | 372559a0 |
| 6 | 390 nerve-evt-mode-port **403f7179** | none | 3aea9e38 | 2562f78a |
| 7 | `cargo fmt` of two merge lines | - | 487c5797 | d8ff6869 |

All six source tips are ancestors of the branch head (`git merge-base --is-ancestor`, checked).
Slot 392 expected conflicts in 386 (2 files) and 390 (2 files) against 373+374; in this order the
386 conflict shrank to `.cargo/config.toml` only and 390 merged clean, because 375/376 had already
resolved the shared `real_cross_target.rs` / port-env commits (363 lineage).

## What the batch changes (vs 3beabcb1 = 373+374)

Test files, docs, `[dev-dependencies] galion-realtest` (workspace path crate) in 9 crates, and the
test-helper crate `galion-realtest` itself (`ports.rs`). **No production `src/` file changes, no
registry crate added, `.cargo/config.toml` byte-identical to 373.** Release binaries do not change:
openmind-nerve x86_64 musl from this tree = **2,017,768 B**, the same size slot 373 recorded.

## Gates (box, 2026-10-28 CET +01:00)

| gate | result |
|---|---|
| `cargo fmt --all --check` | ok (after 487c5797) |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok (only the known vendor/candle-core deprecation warnings, not workspace lints) |
| `env -u GALION_REAL cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` x3 | **2232 / 0 failed / 18 ignored, 424 suites, all 3 runs** (18:07-18:11, 18:11-18:13, 18:13-18:15) |
| `scripts/no-python.sh` | ok |
| `scripts/spdx-scan.sh` | 817 scanned, 0 missing |

Real-life evidence: `docs/real-tests/post-publish-test-batch.md`.

## How to land it after the publish

1. After 373 then 374 are on main-staging (publish-path step 3) and PR #12 is merged (step 5),
   merge this branch (or replay steps 2-6 with the same resolutions) onto main-staging.
2. Re-run the gates above in CI; GALION_REAL tests stay out of CI.
3. Known, not caused by this batch: F-383-5 (`real_enroll` 71/72, "rejected B: claim refused"),
   F-383-1..10 golden/port findings, F-375-1 / D-375-1 console Node-era routes.
