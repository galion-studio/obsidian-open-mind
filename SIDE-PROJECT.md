<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->
# Side project: `repo-check` - repo self-evaluation lint binary

Started 2026-10-18T21:01:42+02:00 (Europe/Zagreb), branch `ws/side`.

## Why
TEACHER/CHECKS.md defines C6 (Zagreb time), C7 (CJK scan) and C8 (no Python) as
ad-hoc `rg`/`find` one-liners, and the plan's Phase 1 lists SPDX headers and a
self-check (B42-B44) with no tool behind them. There is no CI workflow yet. One
small std-only Rust binary turns these into one repeatable command that CI
(`.github/workflows/self-eval.yml`) and the lead can run. Not part of any
Rust rewrite workstream (Galion One / desk / tuner, survival, memory, Hermes),
omnode, the PWA or the brand pack.

## Scope
New crate `crates/repo-check` (std only, zero dependencies), binary `repo-check`:
- `license`: root LICENSE exists and starts with "MIT License" + a Copyright line.
- `spdx`: every `.rs` under `crates/` has `SPDX-License-Identifier: MIT` in its first lines.
- `cjk`: no Han/Hiragana/Katakana/Hangul characters in tracked text files (C7).
- `python`: no `.py` files outside `contrib/`, `target/`, `stage/` (C8).
- `utc-print`: no `Z`-suffixed RFC 3339 timestamp literals / `Utc::now()` outside zagreb-time (C6, report).
- `secrets`: no private-key blocks, `ghp_`/`github_pat_`/`sk-`/AWS-key shaped tokens (never prints the match, only file:line).
- `hosts`: no hard-coded public IPv4 literals in crates/ and deploy/ (loopback/private/doc ranges allowed).
- Output: human text, or `--json` with a Zagreb timestamp; exit 1 on any error-level finding.
- `--only <check>` and `--warn <check>` flags; existing gaps (missing SPDX) can be run as warn.
- Draft workflow `.github/workflows/self-eval.yml` that builds and runs it.

## Done criteria
- `cargo test -p repo-check`, `cargo clippy -p repo-check -- -D warnings`, `cargo fmt --check` pass.
- Runs on this repo, findings reported; release binary size recorded (< 2 MB).
- No other crates touched (SPDX gaps in other crates are reported, not fixed).
