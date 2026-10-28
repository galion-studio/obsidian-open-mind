<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# repo-check

Read-only repo self-evaluation lint for OpenMind. Std only plus `zagreb-time`
(for the report timestamp). One command replaces the TEACHER/CHECKS.md
one-liners C6 (Zagreb time), C7 (CJK) and C8 (no Python), and adds the repo
rules that had no tool: MIT LICENSE, SPDX headers, secret shapes, hard-coded
hosts and the UNIVERSAL-CPU.md portability rules.

```sh
CARGO_TARGET_DIR=/workspace/oom-target/side nice -n 19 cargo build -j 1 -p repo-check
repo-check --list                          # checks and default levels
repo-check --warn spdx                     # human report, exit 1 on errors
repo-check --json --copyright "Galion Studio" > self-eval.json
repo-check --only portable,secrets         # subset
```

| check | default | what |
|---|---|---|
| license | error | root LICENSE starts `MIT License`, has `Copyright (c) ` (and `--copyright TEXT` if given) and the MIT grant; crate LICENSE copies are identical |
| spdx | error | `.rs`, `.sh`, `Cargo.toml` under `crates/` and `deploy/` carry `SPDX-License-Identifier: MIT` in the first 5 lines |
| cjk | error | Han / Hiragana / Katakana / Hangul code points (Croatian letters are fine) |
| python | error | `.py`/`.pyc`/`.pyw` files and python shebangs |
| utc | warn | UTC clock reads and `...Z` timestamps in `crates/**/*.rs` outside `zagreb-time` (review list: calls that feed zagreb-time are fine) |
| secrets | error | private-key blocks and token shapes (GitHub, Slack, AWS, Google, GitLab, `sk-` keys); only file:line is printed, never the value |
| hosts | error | public IPv4 literals in `crates/`, `deploy/`, `.github/` (loopback, private, link-local, CGNAT, documentation ranges allowed) |
| portable | error | inline asm, `core::arch`/`std::arch`, `#[target_feature]`, feature-detect macros, `target-cpu`/`target-feature` in TOML/CI, `openssl-sys` in Cargo.lock |
| deps | error | serde_json features pinned once in the root `[workspace.dependencies]` (`preserve_order`); `crates/*/Cargo.toml` may only write `serde_json = { workspace = true }`. Without it `cargo build -p X` and `--workspace` emit different JSON key orders (docs/decisions/serde-order-pin.md) |
| perms | error | secret-like files (`link.env`, `.env*`, `*.pem`, `*.key`, `id_*`, names with secret/token/passw/credential) must not be world-writable (error) or group-writable (warn); world-writable dirs need the sticky bit. Metadata only, never opens the file. `--stat-list FILE|-` lints a `find ROOT -printf '%m %y %p\n'` listing from another host (D-318-2, docs/ops/link-env-perm-stage.md). Owner-aware lists (`%m %y %u %g %p`), `--chmod-plan` (TSV or guarded `--sh`, `--rollback`, `--plan-tree`, `--plan-ow-tree` (o-w only, reason `tree-o-w`), `--plan-reasons`) and `--plan-verify` stage and check a chmod plan without changing anything (D-356-3, docs/ops/world-writable-dirs-stage.md, docs/ops/chmod-plan-refresh.md) |

Skipped: `.git`, `target`, `contrib` (vendored), `stage`, `node_modules`,
binaries (NUL byte / non-UTF-8) and files over 2 MB. A line carrying the marker
`repo-check: allow` is skipped by every check except `secrets` and `license`;
use it for a reviewed, cfg-gated arch path that has a portable fallback.

Exit codes: 0 clean, 1 error findings, 2 usage or I/O error.
