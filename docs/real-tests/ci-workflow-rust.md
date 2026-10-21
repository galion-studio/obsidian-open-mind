<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->
# Real-life tests: ci-workflow-rust (cargo xtask)

Box runs, 2026-10-21 02:55-03:05 CEST (+02:00), worktree on swarm/integration-2 df42e0f + this branch.
`CARGO_TARGET_DIR=/workspace/swarm/target/ci-workflow-rust`, builds through the wave-4 `cargo-slot` queue, `-j 2`.
No VPS access was needed (CI only). No secrets read or printed; gitleaks runs with `--redact`.

## Unit tests

`cargo test -p xtask --locked`: 11 passed, 0 failed. Includes
`workflows_only_call_xtask` (every `run:` line in .github/workflows is `cargo xtask ...`),
`inventory_tasks_are_dispatched`, `pins_are_complete`, the SPDX judge table,
and `gates_catch_planted_findings` (throw-away git repo with a `.py` file, a
python shebang, a CJK line, a UTC stamp and a changed crate LICENSE: the
gates report 2 / 1 / warn-only / 1 findings; the clean repo passes).

## Parity with the bash they replace (same tree)

| Gate | Old (bash) | New (cargo xtask) | Result |
|---|---|---|---|
| zagreb-ts | `grep -InE ...Z\b`: 55 hits | 55 warnings | file:line:text lists identical (`diff` empty) |
| cjk | `grep -InP` ranges: 0 | `ok: no CJK characters` | same |
| no-python | `git ls-files '*.py' ...`: 0 | `ok: no Python files, no python shebangs` | same |
| license | inline bash | `ok: MIT (53 crate LICENSE copies identical)` | pass |
| sbom | `bash scripts/sbom.sh`: `sbom: ok (153 packages)` | `cargo xtask sbom`: `sbom: ok (153 packages)` | report text identical except the time line; `jq -S 'del(.generated)'` of both JSON files identical |
| sbom --check (GPL fixture) | exit 1, `FAIL dep x 1 GPL-3.0-only disallowed` | exit 1, same row | same |

Timings (wall, warm build): no-python 0.69 s, cjk 0.73 s, license 0.04 s, zagreb-ts 0.31 s, sbom 0.44 s.
Cold xtask debug build: 4.9 s.

## Tool installs (network, pinned sha256)

`cargo xtask install gitleaks cargo-deny cargo-cyclonedx zig`: 8.45 s, every
`sha256sum -c` printed `OK` (gitleaks 8.30.1, cargo-deny 0.20.2,
cargo-cyclonedx 0.5.9, zig 0.15.2). Second run: 0.05 s (idempotent).
`comply 0.1.1` was built by `cargo install --locked` inside the launch-gates run.

## Task runs

- `cargo xtask gitleaks`: 293 commits scanned, 9.02 MB in 519 ms, `no leaks found`, exit 0.
- `cargo xtask ucpu --zig grep`: `ucpu-gate: RESULT ok`, exit 0.
- `cargo xtask fmt`: workspace + edge/openmind-node clean, exit 0.
- `cargo xtask self-eval`: repo-check (static musl, rust-lld) `1124 files, 0 error(s), 34 warning(s) -> PASS`, self-eval.json written.
- `cargo xtask launch-gates --out <target>/public --online` (the full launch-gates job):
  gates 1-7 and 9 PASS (1118 files exported; REUSE 1043 passed / 0 failed / 58 skipped;
  CycloneDX 150 components), 10 WARN (22 public IPv4 literals, 1 quick-tunnel host: owner decides),
  **8 cargo deny FAIL: advisories** on the root workspace, pre-existing on integration-2:
  RUSTSEC-2026-0367/0368/0369 (zip, via candle; slot 109 candle-zip-fix) and
  RUSTSEC-2024-0436 (paste unmaintained). Not caused by this branch.
- Portability: `xtask` release builds static for x86_64 musl (599 864 B) and aarch64 musl
  (526 480 B), no PT_INTERP; `cargo check` passes for armv7-unknown-linux-musleabihf and
  aarch64-linux-android; the aarch64 binary runs under qemu-aarch64 (`xtask list`).

## Not run here

- `cargo xtask realtest`: the stack needs ports 19500-19551; this slot owns 19850-19859.
  The task passes extra args through (`--base-port N`), so it can run in any free range.
- `package`, `sign`, `gh-release`: release-only (need a `v*` tag, cosign OIDC and a GitHub token).
- `clippy`/`test` over the whole workspace and `omnode`: the same cargo commands as before,
  wrapped; this branch only adds the xtask crate (clippy + tests green for it).
