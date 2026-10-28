<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Land order with the gateway size fixes (wave-4 slot 373), 2026-10-28 07:22-07:55 CET (+01:00)

Throwaway branch `swarm/land-order-gateway-size` off `swarm/land-order-rehearsal` ebc1a63b
(slot 359: tree of fc481390 -> 7091f76b -> 5aa31927 + e158400a on the main-staging snapshot
3f015986). Slot 362 found that this land tree lacked the hermes-gateway size fixes a0d099c2
(slot 329) and 16bfc28d (slot 353). They are rehearsed here on top.
**swarm/main-staging and main were NOT updated. Nothing deployed. No VPS change.**

## Order rehearsed

| step | merged | result | rehearsal commit |
|---|---|---|---|
| 1-6 + fix | slot 359 land (fc481390 -> 7091f76b -> 5aa31927 + e158400a) | as slot 359 | ebc1a63b |
| 7 | a0d099c2 hermes-gateway-2mib (329) | **2 conflicts**: `.cargo/config.toml` (keep the `[alias] xtask` block from the land AND add `[target.x86_64-unknown-linux-musl] linker = "rust-lld"`), `scripts/size-budget.sh` (drop the tagged 2.5 MiB gateway case, as a0d099c2 intends) | 2d594592 |
| 8 | 16bfc28d size-budget-gateway-fix (353; carries 942c3d95 size-budget.tsv, 7427f96e release-pair-check, 124a2124) | **3 add/add conflicts**: `crates/galion-realtest/tests/real_release_artifacts.rs`, `docs/real-tests/release-artifacts-refresh.md`, `docs/release/artifacts-refresh-20261027.md`. HEAD's copies are byte-identical to 124a2124 (= 7091f76b content), so 16bfc28d's side is a pure forward change -> take theirs | 272abc42 |
| fix | F-373-1 (below) | 2 test files | 49f214b9 |

Code tree at 49f214b9: 87a71adc (docs-only commit on top).

## Findings

- **F-373-1 (land breaks a test; fixed here).** `galion-feature-pin` (from fc481390, slot 337)
  pinned the old policy: `release_budgets_match_size_budget_script` required
  `HERMES_GATEWAY_LIMIT=$((5 * 1024 * 1024 / 2))` in size-budget.sh, and RELEASE_BINS gave
  hermes-gateway a 2.5 MiB budget. After a0d099c2 + 16bfc28d the script reads
  config/release/size-budget.tsv (2 MiB for every binary), so the workspace run had 1 failure
  (`size-budget.sh LIMIT changed`). Fix: RELEASE_BINS hermes-gateway = SIZE_LIMIT (2 MiB),
  `HERMES_GATEWAY_LIMIT` removed, the test now parses the budget table (default row = 2 MiB,
  every RELEASE_BINS budget = its table row, no gateway exception in the script).
  Negative control: setting the table's hermes-gateway row to 2621440 makes the test FAIL
  (`left: 2097152 right: 2621440`). **Any real land of a0d099c2/16bfc28d after fc481390 must
  carry 49f214b9** (or the same change), otherwise CI fails.
- **S-373-1 (size, the goal).** On the full land tree hermes-gateway (x86_64 musl release,
  14-bin `-p` set) is **1,948,240 B, 148,912 B under 2 MiB**, stripped, with `.relr.dyn`
  (slot 359 tree without the fixes: 2,202,384 B, over 2 MiB). Byte-identical solo, pair and
  full set. Live VPS gateway is still the 2,732,248 B `hermes-gateway-b7fbbc78` without RELR
  (F-353-1, unchanged; this branch deploys nothing).
- **S-373-2 (nerve headroom is the tight one now).** openmind-nerve 2,017,768 B = 79,384 B under
  2 MiB on this tree. Next growth of the nerve should watch this (slot 368 RELR/ICF would help).
- **F-373-2 (test flake, not from this land).** In the second full workspace run (07:38-07:52)
  `openmind-nerve` tests/evt_mode.rs `paid_slot_against_stub_respects_caps` failed and
  `event_mode_end_to_end` hung 12 min (blocked opening its journal FIFO for write after the
  nerve child was gone; killed by me). The suite uses fixed ports 20761/20762 shared by every
  slot; other agents were running workspace tests at the same time. Re-run alone 07:53: 3/3 ok
  in 4.1 s; it also passed in the first full run (07:27-07:30). Suggest per-slot port env and a
  non-blocking FIFO open (O_NONBLOCK / timeout) for that test.
- **E-373-1 (environment).** `cargo check --target aarch64-linux-android` fails for crates with
  bundled SQLite (desk-hermes, hermes-gateway, openmind-memory, galion-backup) because the box
  has no NDK `aarch64-linux-android-clang` (libsqlite3-sys build script ToolNotFound). Not code;
  the other 9 release crates check ok; aarch64 musl and armv7 musl workspace checks pass.

## Gates (box, CARGO_TARGET_DIR target/land-order-gateway-size)

| gate | time CET | result |
|---|---|---|
| `cargo fmt --all --check` | 07:25, 07:31 | ok |
| clippy `--workspace --all-targets` excl. openmind-sandbox, cgroup-detect, `-D warnings` | 07:26, 07:38 (after fix) | rc 0 |
| clippy gaia-cortex `--all-targets --features llm-advisor -D warnings` | 07:27 | rc 0 |
| ws test excl. sandbox, cgroup-detect, hermes-tools, `--no-fail-fast`, run 1 (before fix) | 07:27-07:30 | 422 suites, 2217 passed, **1 failed (F-373-1)**, 18 ignored |
| galion-feature-pin release_feature_unify after fix | 07:31 | 5/5; negative control FAIL as expected |
| ws test run 2 (after fix) | 07:38-07:52 | 421 suites, 2215 passed, 1 failed + 1 hung (F-373-2 evt_mode, shared ports), 18 ignored |
| openmind-nerve evt_mode alone | 07:53 | 3/3 ok (4.1 s) |
| `scripts/no-python.sh` | 07:27 | ok |
| `scripts/spdx-scan.sh` | 07:27 | 814 scanned, 0 missing |
| repo-check `--root .` | 07:27 | PASS 0 errors, 86 warnings |
| x86_64 musl `--release --workspace` + `scripts/size-budget.sh --skip-build` | 07:26 | ok (43 binaries), hermes-gateway 1,948,112 B |
| real_release_repro (GALION_REAL, x86_64 musl, --locked --offline, -j 4) | 07:31-07:36 | **19 compared, 0 differ, 0 over budget/unstripped** (293 s) |
| cargo check aarch64-unknown-linux-musl / armv7-unknown-linux-musleabihf (ws) | 07:52 | ok / ok |
| cargo check aarch64-linux-android | 07:52-07:55 | 9 release crates ok, 4 SQLite crates E-373-1 |

Real tests: see docs/real-tests/land-order-gateway-size.md. Sandbox tests not run
(sandbox_vps_only; nothing here touches the sandbox). aarch64 byte proof not run.

## Recommended real land (owner)

- **D-373-1** land fc481390 -> 7091f76b -> 5aa31927 -> e158400a -> a0d099c2 -> 16bfc28d ->
  49f214b9 (F-373-1). Conflicts at a0d099c2 (2) and 16bfc28d (3, take theirs) are mechanical and
  recorded above. Alternative: land this branch's tree as one merge.
- **D-373-2** carry F-373-1 with the gateway fixes (otherwise CI fails on release_feature_unify).
- **D-373-3** gateway deploy (F-353-1: live 2.73 MB) stays a separate owner-approved step.
