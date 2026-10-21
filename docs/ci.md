<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->
# CI: every workflow step is `cargo xtask <task>`

Branch `swarm/ci-workflow-rust`, base `swarm/integration-2` df42e0f, 2026-10-21 (CEST +02:00).

## Why

Before this change the six workflows carried ~180 lines of inline bash
(no-Python scan, CJK scan, Zagreb-timestamp scan, licence check, scanner
downloads with sha256 checks, release packaging, cosign loop) plus calls into
`scripts/*.sh`. None of it could be run locally the way CI ran it, the tool
pins were copied in two places (gitleaks in `launch-gates.yml` and
`self-eval.yml`), and the owner's direction is Rust only.

Now `crates/xtask` (std + `serde_json` + `zagreb-time`, both already in the
workspace, no new crates) is the one CI entry point. The alias
`cargo xtask = run --locked --quiet -p xtask --` lives in `.cargo/config.toml`.
The YAML keeps only what YAML must do: triggers, permissions, matrix,
checkout, toolchain, artifact upload and the pinned third-party actions.
A unit test (`tasks::tests::workflows_only_call_xtask`) fails if any `run:`
line in `.github/workflows/` is not `cargo xtask ...`, so inline shell can't
creep back in.

`cargo xtask list` prints the same map as the table below; `cargo xtask help` lists every task.

## Inventory: workflow step to task

| Workflow | Job / step | Before | Now |
|---|---|---|---|
| self-eval.yml | fmt | 2 x `cargo fmt --check` | `cargo xtask fmt` |
| self-eval.yml | clippy | `cargo clippy ... -D warnings` | `cargo xtask clippy` |
| self-eval.yml | test | `cargo test --workspace --locked` | `cargo xtask test` |
| self-eval.yml | omnode | clippy + test of edge/openmind-node | `cargo xtask omnode` |
| self-eval.yml | ucpu-tier1 (x86_64, aarch64 musl) | `scripts/install-zig.sh` + `scripts/ucpu-gate.sh grep build static size clippy` | `cargo xtask ucpu --zig grep build static size clippy` |
| self-eval.yml | ucpu-tier2 (armv7, android) | `install-zig.sh` + `ucpu-gate.sh check` | `cargo xtask ucpu --zig check` |
| self-eval.yml | ucpu-qemu | `apt-get qemu-user-static` + `install-zig.sh` + `scripts/real/ucpu.sh` | `cargo xtask ucpu-qemu --apt-qemu` |
| self-eval.yml | repo-check | musl build + 2 runs of repo-check, inline | `cargo xtask self-eval` |
| self-eval.yml | no-python | 15 lines bash | `cargo xtask no-python` (Rust) |
| self-eval.yml | cjk-scan | `grep -P` over tracked files | `cargo xtask cjk` (Rust) |
| self-eval.yml | zagreb-timestamps | `grep -E` over tracked files, warning only | `cargo xtask zagreb-ts` (Rust, still warning only) |
| self-eval.yml | gitleaks | curl + upstream checksums.txt + sudo install | `cargo xtask gitleaks` (pinned sha256, no sudo) |
| self-eval.yml | license | 10 lines bash | `cargo xtask license` (Rust) |
| realtest.yml | box-stack | build 9 packages + run + summary step | `cargo xtask realtest --out target/realtest/ci` (also writes the job summary) |
| launch-gates.yml | public-export | 4 pinned downloads + `cargo install comply` + 2 x fetch + `scripts/public-export.sh` | `cargo xtask launch-gates --out "$RUNNER_TEMP/public" --online` |
| sbom.yml | cargo-sbom | `cargo fetch` + `bash scripts/sbom.sh` (jq) | `cargo xtask sbom` (Rust port, byte-identical report) |
| release.yml | package | 12 lines bash | `cargo xtask package` |
| release.yml | sign | cosign loop in bash | `cargo xtask sign` |
| release.yml | GitHub Release | `gh release create ...` | `cargo xtask gh-release` |
| release.yml | SBOM | `anchore/sbom-action` | unchanged (pinned action) |
| scorecard.yml | analysis | `ossf/scorecard-action` | unchanged (pinned action, no `run:`) |
| dependabot.yml | - | config only | unchanged |

Gating is unchanged: release.yml is still tag-only and skipped unless the repo
is public and `RELEASE_ENABLED == 'true'`; scorecard still skips while private;
`zagreb-timestamps` keeps `continue-on-error: true`; nothing publishes.

## What is native Rust and what is still delegated

Native Rust (crates/xtask/src):

- `checks.rs`: no-python, cjk, zagreb-ts, license. Ported 1:1; on this tree
  the Rust zagreb scan returns exactly the same 55 file:line hits as the old
  `grep -E`, and cjk/no-python agree (0 hits).
- `sbom.rs`: the SPDX allowlist judge and SBOM document, replacing
  `scripts/sbom.sh` + jq (deleted). Same report and same JSON on the
  153-package graph (only `generated` differs). One deliberate difference: an
  id that starts with AND/OR/WITH (e.g. `ORACLE`) is one token; jq split it.
  Both reject it; only the reason text differs.
- `tools.rs`: one pin table (gitleaks 8.30.1, cargo-deny 0.20.2,
  cargo-cyclonedx 0.5.9, comply 0.1.1, zig 0.15.2) with sha256 sums,
  replacing `scripts/install-zig.sh` (deleted) and the env blocks in the YAML.
  Downloads go to `$RUNNER_TEMP/galion-tools` (CI) or
  `$CARGO_TARGET_DIR/xtask-tools` (local); nothing is installed system-wide.
- `tasks.rs`: orchestration (cargo invocations, packaging, signing, release).

Shells out to (std `Command`, no shell): `cargo`, `git ls-files`, `curl`,
`sha256sum -c`, `tar`, `cosign`, `gh`, `sudo apt-get` (only `ucpu-qemu --apt-qemu`
on a runner without qemu).

Still bash, called by xtask (follow-up ports, each its own slot):

| Script | Lines | Called by | Note |
|---|---|---|---|
| scripts/ucpu-gate.sh | 270 | `cargo xtask ucpu` | grep/build/static/size/check stages; next port candidate |
| scripts/public-export.sh | 312 | `cargo xtask launch-gates` | 10 gates over a `git archive` tree |
| scripts/real/ucpu.sh | 174 | `cargo xtask ucpu-qemu` | being ported by the realscript-port slots (112-114) |
| scripts/size-budget.sh | 177 | ucpu-gate.sh | |

When those land as Rust, only the xtask function body changes; the YAML stays as is.

## Run it locally

```
cargo xtask ci            # fmt clippy test no-python cjk license zagreb-ts
cargo xtask sbom --check fixture.json
cargo xtask install zig   # prints the bin dir; pinned + sha256-checked
cargo xtask realtest --base-port 19500   # extra args go to galion-realtest stack
```

## Cost

Light jobs (no-python, cjk, license, zagreb-ts, gitleaks) now compile xtask
first: a cold debug build of xtask + chrono/chrono-tz/serde_json took 4.9 s on
the box (`-j 2`). The xtask release binary is static musl, 586 KiB (x86_64) and
514 KiB (aarch64), well inside the 2 MiB budget the ucpu gate applies to every
workspace binary.

## Open items

- `.cargo/config.toml` gets an `[alias]` section. Slot 145 (toolchain-aarch64)
  edits the same file in a different section: textual merge, no conflict in content.
- Not run on GitHub yet (GitHub actions run only after the owner merges / pushes);
  the box runs in docs/real-tests/ci-workflow-rust.md cover every task except
  `realtest` (needs ports 19500-19551, outside this slot's range), `package`,
  `sign` and `gh-release` (release-only, need a tag, cosign OIDC and a token).
