<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Swarm 8, group A — dependency triage (A2)

Date: 2026-11-15 (Europe/Zagreb, CET, offset `+01:00`).

Repository: `galion-studio/obsidian-open-mind`. Triage base is `main` at `de7480e71ce98191d2ee11f018fa15be6c5a1707` ("Console: bring the connectors screen onto the current shell (#53)").

This pass is read-only on the existing pull requests. Nothing here merges, closes, approves, or pushes to them. The only new branch is `swarm8/deps-batch`.

## How this was checked

GitHub pull-request metadata, check rolls, and failed-job logs were read with `gh` on 2026-11-15. Merge results against current `main` for the older pull requests used `git merge-tree --write-tree` after fetching those head SHAs. No existing head was checked out and no existing branch was updated.

Local builds used a toolchain installed for this run:

- Image default: `rustc 1.83.0` / `cargo 1.83.0`.
- Runs below: `rustup toolchain install stable` → `rustc 1.100.0 (0ef1bc386 2026-11-09)`, plus the `clippy` and `rustfmt` components.

`contrib/grokbot/mcp/vps-mcp` is `edition = "2024"`, and `pageant` 0.2.5 declares `rust-version = "1.89"`, so the 1.83 toolchain cannot build that package. The 1.100.0 runs are the ones reported as local results.

`cargo tree -i miniz_oxide` on this branch shows a single workspace user: `galion-backup`. `pageant` is a dependency of `russh`, and its transport modules are `#[cfg(windows)]`.

## Dependabot pull requests

Class is one of `safe-to-merge`, `needs-code-change`, or `risky-hold`. "In batch" means the bump is applied on `swarm8/deps-batch`.

| PR | Bump | Semver | CI on the PR head | Changelog / breaking notes read | Local on 2026-11-15 | Class | In batch |
|---|---|---|---|---|---|---|---|
| [#2](https://github.com/galion-studio/obsidian-open-mind/pull/2) | `toml` 0.8.23 → 1.2.1+spec-1.1.0 | major | 23 checks. `cargo test` **failure**. Other listed checks succeeded, including clippy, fmt, desktop `build, clippy, test`. Head `56674ad3`, base `746dfc44`. Mergeable. | toml 0.8 → 1.x replaces `toml_edit` / `toml_write` with `toml_parser` / `toml_writer` in the lockfile. | Not rebuilt here. CI log for run `43629559600` job `132552939053`: `galion-feature-pin` test `every_release_bin_alone_has_the_full_set_features` failed, 4 passed / 1 failed. Panic at `crates/galion-feature-pin/tests/release_feature_unify.rs:94`: indexmap features for `link-watch` differ between a solo release build and the full release set (`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, indexmap v2.14.2, full set `default,std`). | needs-code-change | no |
| [#19](https://github.com/galion-studio/obsidian-open-mind/pull/19) | `actions/upload-artifact` 4.6.2 → 7.0.2 | major (4 → 7) | 22 checks. Only `gitleaks secret scan` **failure**. `cargo test`, clippy, and the jobs that upload artifacts succeeded. Head `767f4d83`, base `7ba85b60` (behind current `main`). | v6 requires Node.js 24 and Actions Runner ≥ 2.327.1. v7 adds optional direct (unzipped) upload and moves the package to ESM. Call sites in this repo use the default zip upload. | Not applied on this branch. | risky-hold | no |
| [#20](https://github.com/galion-studio/obsidian-open-mind/pull/20) | `actions/checkout` 4.4.0 → 7.0.1 | major (4 → 7) | 22 checks. Failures: `public export + gates`, `cargo test`, `gitleaks`. Head `36c64dab`, base `463d863b`. | v5/v6 move the action to Node.js 24. v7 blocks checking out a fork pull request from `pull_request_target` and `workflow_run`. Current `main` also pins checkout v4.4.0 in `.github/workflows/openmind-desktop.yml`, which this pull request does not touch. | Not applied. Logs: public-export job `132528891031` fails `cargo deny` advisories on `Cargo.toml` (bans, licenses, sources ok). `cargo test` job `132528891819` fails `pre_cutover_backup_overlay_manifest_verify_and_data_delegation` at `crates/galion-deploy/tests/real_backup.rs:162` with the message `build the examples (plain cargo test -p galion-deploy)`. That test is green on current `main` (see main CI below). | risky-hold | no |
| [#21](https://github.com/galion-studio/obsidian-open-mind/pull/21) | `rusqlite` 0.32.1 → 0.41.1 | major (0.32 → 0.41) | 22 checks. Failures include clippy, `cargo test`, box scenarios, ucpu tier-1/tier-2, qemu, gitleaks. Head `ddcd7fa3`, base `7ba85b60`. | 0.41.0 release notes include edition 2024, `column_names` / `columns` returning iterators, MSRV 1.88, bundled SQLite 3.54.0. | Not rebuilt here. Clippy log job `124771426302` (rustc 1.99.1): `error[E0599]` no method `prepare_cached` on `MutexGuard<Connection>` and on `Transaction` in `openmind-memory` (6 errors). Call sites on `main` are in `crates/openmind-memory/src/store.rs`. | needs-code-change | no |
| [#22](https://github.com/galion-studio/obsidian-open-mind/pull/22) | `candle-nn` 0.9.2 → 0.11.0 | minor on 0.x (0.9 → 0.11) | 22 checks. Failures: clippy, `cargo test`, both ucpu tier-1 builds, both tier-2 checks, gitleaks. Head `00b5551f`, base `7ba85b60`. Lock adds `candle-core` 0.11.1 beside the workspace patch. | `docs/security/candle-zip-fix.md` and the root `Cargo.toml` patch comment: no candle release in 0.9.2..0.11.0 requires `zip >= 9`. `main` vendors `candle-core` 0.9.2 for that fix. The 0.11.0 lock in the pull request moves `zip` to 9.2.0 for the crates.io candle-core, which the path patch does not use. | Not rebuilt here. `cargo test` log job `124771461258`: `candle_core::Error` vs `candle_core::error::Error`, and `candle_nn::Linear` / `LayerNorm` / `Embedding` `forward` trait bounds fail against the vendored 0.9.2 core. | risky-hold | no |
| [#23](https://github.com/galion-studio/obsidian-open-mind/pull/23) | `getrandom` 0.2.17 → 0.3.4 (direct dep in `hermes-rs`) | major | 22 checks. Failures: clippy, `cargo test`, ucpu tier-1/tier-2, qemu, gitleaks. Head `d8382a2a`, base `7ba85b60`. | 0.3 changelog (from the pull request body): `wasm_js` backend default when the feature is on, `wasi` replaced by `wasip2`, `rustc-dep-of-std` removed. The 0.2 function `getrandom::getrandom` is gone. | Not rebuilt here. `cargo test` log job `124771508750`: `error[E0425]` cannot find function `getrandom` in crate `getrandom`, `hermes-rs` (2 errors). On `main` the calls are `crates/hermes-rs/src/enroll/mod.rs` lines 348 and 502. The workspace lock already contains `getrandom` 0.3.4 as a separate copy from the 0.2.17 direct dependency. | needs-code-change | no |
| [#24](https://github.com/galion-studio/obsidian-open-mind/pull/24) | `miniz_oxide` 0.8.9 → 0.9.1 | minor | 22 checks. Only `gitleaks` **failure**. `cargo test` and clippy succeeded on that head. Head `42c5b7f6`, base `7ba85b60`. | 0.9.0 changelog: "Version bump due to a minor API break" (enums `non_exhaustive`, MSRV 1.60). 0.9.1 is bug fixes (Huffman trees, wasm stack). `crates/galion-backup/src/gzip.rs` matches `MZStatus::StreamEnd` and `MZError::Buf` with wildcards. | Rebuilt on current `main` with the other batch bumps. `cargo test -p galion-backup --locked`: lib 8, `fake_tree` 3, `real_parity` 2, `real_perf` 2, all passed. `cargo clippy -p galion-backup --all-targets --locked -- -D warnings` passed. | safe-to-merge | yes |
| [#26](https://github.com/galion-studio/obsidian-open-mind/pull/26) | `pageant` 0.2.4 → 0.2.5 in `contrib/grokbot/mcp/vps-mcp` | patch | 22 checks. Only `gitleaks` **failure**. Workspace `cargo test` succeeded. That package is in the workspace `exclude` list, so those workspace tests do not compile it. Head `e88b3626`, base `7ba85b60`. | No changelog file in the published crate. `.cargo_vcs_info.json` sha `183a22e8f8f490fa09b6eb7bd044ef99d8c4414f` is russh commit message `v0.65.0` (2026-10-25). Crate `rust-version` is 1.89. Transport modules are `#[cfg(windows)]`. | `cargo test --manifest-path contrib/grokbot/mcp/vps-mcp/Cargo.toml --locked` on Linux: 3 passed (`policy::tests::readonly`, `policy::tests::classes`, `config::tests::parses_shell_style`). Clippy `-D warnings` on this package failed with two `collapsible_if` lints in `src/config.rs:23` and `src/main.rs:307`. Those lines are unchanged by this batch. Windows Pageant backends were not compiled on this host. | safe-to-merge | yes |
| [#30](https://github.com/galion-studio/obsidian-open-mind/pull/30) | `russh` 0.64.1 → 0.65.1 in the same excluded package | minor | 22 checks. Failures: `public export + gates`, `cargo test`, `gitleaks`. Head `02408836`, base `7ba85b60`. | 0.65.0 release notes: breaking change on the **server** handler (`auth_publickey_offered` / `auth_publickey` take new types). 0.65.1: agent lock is rechecked after confirmation, and client `kex_done` runs only after host-key verification. `vps-mcp` uses `russh::client` (`src/ssh.rs`). | Same `vps-mcp` test command as #26: 3 passed, russh 0.65.1 compiled. The pull request's `cargo test` failure is the same `galion-deploy` example test as #20 (job `132238147342`), on the stale base. | safe-to-merge | yes |
| [#54](https://github.com/galion-studio/obsidian-open-mind/pull/54) | `sigstore/cosign-installer` 3.9.1 → 4.1.3 | major | 26 checks, all success or skipped. `mergeStateStatus` CLEAN. Head `5c93243b`, base is current `main` `de7480e7`. | Installer v4 is required to install Cosign v3. v4.0.0 notes that `cosign sign-blob` on Cosign v3 requires `--bundle`. `release.yml` already passes `--bundle`. v4.1.3 bumps the default Cosign to 3.1.4. | Not applied. `release.yml` runs only on `v*` tags, so the green pull-request checks do not execute this step. Signing was not run locally (that path writes a public Rekor entry). | risky-hold | no |
| [#55](https://github.com/galion-studio/obsidian-open-mind/pull/55) | `github/codeql-action/upload-sarif` 3.39.0 → 4.39.3 | major | 26 checks, all success or skipped. CLEAN. Head `2723db1a`, base current `main`. | v4.39.x notes read from the pull request body are about the analyze/init action (bundle 2.27.4, TRAP cache cleanup removal, macOS 27 traced analysis). This repo calls only `upload-sarif` with `sarif_file`, `continue-on-error: true`. | Not applied. `scorecard.yml` does not run on `pull_request` (push to `main`, schedule, `workflow_dispatch`, `branch_protection_rule`). The #54/#55 check list does not include `Scorecard analysis`, so the upgraded step did not run. | risky-hold | no |
| [#56](https://github.com/galion-studio/obsidian-open-mind/pull/56) | `anchore/sbom-action` 0.24.0 → 0.24.3 | patch | 26 checks, all success or skipped. CLEAN. Head `75e0df0a`, base current `main`. | v0.24.2 notes: eslint bump and workflow/makefile chores. v0.24.3 notes: npm dependency bumps. Annotated tag `v0.24.3` (`fffec3d851af758d56fee099cf3fbf8aa8779843`) peels to commit `66cbf4bc1f1c0d2edc94016e65bc221b6bb0ad6c`, message `chore(deps): update Syft to latest release (#735)`. | Pin updated in `.github/workflows/release.yml`. Desktop and backup tests below still pass. The SBOM step itself runs only on a `v*` tag, so it was not executed. | safe-to-merge | yes |

### Gitleaks failures on the older heads

On #24 the gitleaks job (run `41280592038`, job `124771535769`, 2026-11-01, `fetch-depth: 0`) ended with `leaks found: 5`. The same single gitleaks failure is the only red check on #19 and #26, and it is also red on #20, #21, #22, #23, #28, and #30. Current `main` `de7480e7` has `gitleaks secret scan` **success**. Those red scans are the stale bases, and this batch does not carry them forward.

### Current `main` CI

Check runs on `de7480e71ce98191d2ee11f018fa15be6c5a1707` that completed as success include: `build, clippy, test`, `build and test (x86_64-pc-windows-msvc)`, `build and test (aarch64-apple-darwin)`, `aarch64 build and test (qemu-user)`, `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`, gitleaks, license, repo-check, ucpu tier-1/tier-2, qemu, miri short shards, and `Scorecard analysis`. One miri matrix entry is skipped.

## What the batch contains

Applied on top of `de7480e7`:

| Source PR | Change on this branch |
|---|---|
| #24 | `crates/galion-backup/Cargo.toml` `miniz_oxide` `0.8` → `0.9`. Root `Cargo.lock` `miniz_oxide` 0.8.9 → 0.9.1 (checksum `b63fbc4a50860e98e7b2aa7804ded1db5cbc3aff9193adaff57a6931bf7c4b4c`). |
| #30 | `contrib/grokbot/mcp/vps-mcp` `russh` 0.64.1 → 0.65.1, lockfile updated with `cargo update -p russh --precise 0.65.1`. |
| #26 | Same lockfile, `pageant` 0.2.4 → 0.2.5 via `cargo update -p pageant --precise 0.2.5`. |
| #56 | `.github/workflows/release.yml` `anchore/sbom-action` pin `e22c389904149dbc22b58101806040fa8d37a610` (v0.24.0) → `66cbf4bc1f1c0d2edc94016e65bc221b6bb0ad6c` (v0.24.3). |

No major bump is in this branch. #54 and #55 are the majors whose pull-request checks are green; their changed steps do not run on pull requests, and they stay out of the batch.

### Local commands and results (rustc 1.100.0)

Baseline on `main` before the edits, one command:

`cargo test -p openmind-desktop -p galion-backup --locked`

- `galion-backup`: 8 lib + 3 `fake_tree` + 2 `real_parity` + 2 `real_perf`, passed.
- `openmind-desktop`: 43 lib tests, passed.

After the four bumps together:

| Command | Result |
|---|---|
| `cargo test -p galion-backup --locked` | passed (same 8 / 3 / 2 / 2) |
| `cargo clippy -p galion-backup --all-targets --locked -- -D warnings` | passed |
| `cargo test -p openmind-desktop --locked` | passed (43) |
| `cargo clippy -p openmind-desktop --all-targets --locked -- -D warnings` | passed |
| `cargo test --manifest-path contrib/grokbot/mcp/vps-mcp/Cargo.toml --locked` | passed (3) |
| `cargo clippy --manifest-path contrib/grokbot/mcp/vps-mcp/Cargo.toml --all-targets --locked -- -D warnings` | failed, exit 101, two pre-existing `collapsible_if` lints (`src/config.rs:23`, `src/main.rs:307`) |
| `cargo fmt --all -- --check` | passed |

Not run locally: `cargo test --workspace`, `cargo clippy --workspace`, miri, ucpu/qemu, `cargo deny`, gitleaks, the tag-only release job, and a rebuild of #2 / #21 / #22 / #23. Those four are classified from the CI logs named above.

## Older pull requests

| PR | Title | Against current `main` | Recorded CI | Recommendation |
|---|---|---|---|---|
| [#7](https://github.com/galion-studio/obsidian-open-mind/pull/7) | brand: list the 9 feature icons in `BRAND.md` and `STYLE-GUIDE.md` | Still absent. `brand/BRAND.md` section 7 describes the icons and does not list the nine files. `brand/STYLE-GUIDE.md` has no "Feature icons" section. `git merge-tree --write-tree main 959790ac` exited 0 (clean). GitHub: `mergeable=MERGEABLE`, `mergeStateStatus=CLEAN`. Head `959790ac`, base `317d1d40`. | 12 checks, all success. That set is smaller than current `main` (no desktop Windows/macOS/aarch64 jobs on this head). | **keep**. Docs-only, still missing, merges clean. Rebase only if branch protection starts requiring the newer desktop jobs. |
| [#14](https://github.com/galion-studio/obsidian-open-mind/pull/14) | hermes-offhost: snapshot, heartbeat, and resume when the VPS is off | Feature is absent: no `crates/hermes-rs/src/offhost.rs` on `main`. GitHub: `mergeable=CONFLICTING`, `mergeStateStatus=DIRTY`. `git merge-tree` exited 1. Conflicts: `crates/hermes-gateway/src/main.rs`, `crates/hermes-rs/src/api.rs`. `lib.rs` auto-merged. Head `1e0f427b`, base `98466ab9`. | No check runs on the head. | **rebase**. The behavior is still unimplemented. Resolve the two conflicts and re-run CI before any merge. |
| [#15](https://github.com/galion-studio/obsidian-open-mind/pull/15) | openmind-mcp-stack: real Rust MCP server (memory + read-only vault) | `main` still has the stub: `crates/openmind-mcp-stack/src/lib.rs` serves only `stack_status` and opens no socket. `docs/mcp-notes.md` says the notes crate does not change that stub. `git merge-tree` exited 1 with one conflict: `docs/crate-index.md`. Head `498244ec`, base `7ba85b60`. | 22 checks, all failure (including fmt, clippy, `cargo test`, license, no-CJK). | **rebase**. The real memory/vault server is not on `main`. The code side auto-merged; the crate index conflicts, and the recorded CI is red on the stale base. |
| [#16](https://github.com/galion-studio/obsidian-open-mind/pull/16) | memory integration 2026-10-29 (stale-diag, Hermes memory client, session scope, nerve freshness) | Not on `main`. `crates/hermes-rs/src/` has no `memory.rs`. No `memory_freshness.rs` under `crates/openmind-nerve`. `git merge-tree` exited 0 (clean tree). Head `5c7bad5f`, base `7ba85b60`. This merged tree was not compiled here. | 22 checks, all failure. | **rebase**. Still relevant, and git can merge it, but the head CI is entirely red. Re-run CI on current `main` before merging. |
| [#17](https://github.com/galion-studio/obsidian-open-mind/pull/17) | teaching: sandbox gate first (SOUL vault/memory + cortex-escalation) | `deploy/staged/hermes-teaching/` is absent on `main`. `git merge-tree` exited 0. Head `9dad0a7a`, base `7ba85b60`. | Run `40880733693`: every job failed (`cargo test`, `no CJK characters`, license, fmt, clippy, ucpu, gitleaks, and the rest). `gh run view --log-failed` for the no-CJK job returned an empty log, so the failure text was not captured. | **rebase**. The teaching files are still absent. Refresh CI on current `main` before merging. |
| [#28](https://github.com/galion-studio/obsidian-open-mind/pull/28) | save: G.A.L.I.O.N. secrets-guard gitignore | `git merge-tree` exited 0, so the patch would apply. The pull request body says "Save branch only — not a merge request." Current `.gitignore` already ignores `.env`, `.env.*`, `target/`, `node_modules/`, `*.wav`, `*.mp3`, `*.safetensors`, `*.bin`, `*.onnx`, `*.pt`, `*.py`. The added block repeats several of those and places another `*.pt` after `!vendor/candle-core/tests/*.pt`, which would ignore the vendored fixtures again. `*secrets*` would also match `galion-secrets` after the existing un-ignore rules. Head `10e3d478`, base `7ba85b60`. Only gitleaks failed (same stale full-history scan). GitHub reported the pull request mergeable. | 22 checks, gitleaks failure only. | **close**. Keep the branch as a save snapshot. A narrower follow-up can add any key patterns that are still only under `stack/**`. |

## Recommendations for Maciej

1. Review and merge `swarm8/deps-batch` if its CI matches the local results: `miniz_oxide` 0.9.1, `russh` 0.65.1, `pageant` 0.2.5, and `anchore/sbom-action` 0.24.3. That replaces #24, #26, #30, and #56.
2. Leave #2, #21, and #23 open until someone changes call sites (`galion-feature-pin` indexmap features, `prepare_cached` in `openmind-memory`, `getrandom::getrandom` in `hermes-rs`).
3. Leave #22 open. `candle-nn` 0.11.0 does not compile against vendored `candle-core` 0.9.2, and `docs/security/candle-zip-fix.md` still says to wait for a candle release that requires `zip >= 9`.
4. Leave #19 and #20 open until they are rebased onto current `main`. Both are multi-major action bumps (Node.js 24; checkout v7's fork-PR block). #20 does not update `openmind-desktop.yml`. Their red `cargo test` / `cargo deny` results are from stale bases whose `main` successors are green.
5. Leave #54 and #55 open until the tag-only release workflow and a scorecard run have actually executed the new pins. Pull-request CI on current `main` is green and does not run those steps. Installer v4 switches the default Cosign to v3; `release.yml` already passes `--bundle`.
6. Older work: keep #7, rebase #14, #15, #16, and #17, and close #28. Those are recommendations only; this pass did not close or rebase them.
7. Close draft #29. Do not merge `main` into it. Reason in the next section.
8. Root `LICENSE` on this branch is a byte copy of `galion-operating-system` `main` `LICENSE` (1257 bytes, fetched 2026-11-15). The same bytes replace every project `LICENSE` that was previously identical to the old root file (67 copies besides the root: `crates/*/LICENSE`, `edge/openmind-node/LICENSE`, `release/repo/LICENSE`, `release/obsidian-open-mind-kit/LICENSE`, `docs/swarm/galion-os-staging/LICENSE`, `contrib/grokbot/LICENSE`). Third-party licences under `vendor/candle-core` and `stack/agents/personas/crew-skills` were left as they were. The license job and `repo-check --copyright` now expect `Copyright (c) 2026 Galion Labs`, because the old gate required `Copyright (c) 3000 Galion / galion-studio` and a first line of `MIT License`.

## Draft #29

[#29](https://github.com/galion-studio/obsidian-open-mind/pull/29) is a draft, `galion.app: USD/EUR geo pricing, Galion Labs footer, Research rename`. Head `00f2d443`, base `swarm/galion-one-ask-anything-20261101` (`46c6889c`), not `main`. GitHub reported it mergeable against that base. Its CI rollup on the head is four miri checks (three success, `miri full` skipped).

[galion-operating-system#16](https://github.com/galion-studio/galion-operating-system/pull/16) merged at `2026-11-15T00:39:02Z` (`b9517e92`). That is the live galion.app site. It already shows USD for the United States, Canada, Australia, New Zealand, and everywhere outside UN M49 Europe, and EUR for Europe including GB and IE. Plus (Galion One) is $20 or €20 per month. Ultra (Galion Research) is $220 or €220 per month. The footer uses the same four lines as that repo's `LICENSE`. The product name on the site is Galion Research.

This repo's `main` does not contain `crates/galion-one/src/home.rs` or `crates/galion-one/home/index.html`. Those files exist on the swarm branch #29 targets. Merging `main` into that branch would not put the marketing pages on `main`, and the live site copy is already in the other repository.

One difference: #29's body prices Galion Research as a one-time $220 / €220. The merged site prices it per month. The merged site is the one that is live.

Recommendation: **close**. Not rebased, not merged with `main`, and not closed from this pass.
