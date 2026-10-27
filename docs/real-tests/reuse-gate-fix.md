<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 267 reuse-gate-fix: publish blocker B10 (REUSE gate 7)

Branch `swarm/reuse-gate-fix`, base main-staging `7c95348b`. Times Europe/Zagreb (CET, +01:00).

## Finding: B10 is already fixed on the base

B10 (docs/launch/PUBLISH-BLOCKERS-20261027.md, from CALL-RECORD L806) says
REUSE gate 7 fails on integration-5 `0d68f46` with 7 files. main-staging
`7c95348b` ("REUSE: CC0-1.0 text, string-SPDX overrides, vendored candle-core
annotation + .license sidecars") already fixes all 7. This branch adds no
licence change. It adds a regression guard so the fix stays in place.

| check (2026-10-27) | tree | result |
|---|---|---|
| 18:23 `comply lint --lines` over `git archive 0d68f46` (negative control) | integration-5 | FAIL, 7 files: bench-banking axes.rs, bench-scoreboard lib.rs (SPDX lines inside string fixtures), vendor/candle-core/tests/{fortran_tensor_3d.pth,test.npy,test.npz,test.pt,test_with_key.pt} (no licence/copyright) |
| 18:23 `comply lint` over `git archive 7c95348b` | main-staging | PASS 2063 total, 1996 passed, 0 failed, 67 skipped |
| 18:24:50-18:24:58 `scripts/public-export.sh --allow-missing-tools` (GALION_TOOLS_BIN=/workspace/swarm/tools/bin) | 7c95348b | 0 FAIL, 1 WARN; gate 7 PASS 2063/1996/0/67; gate 8 cargo deny PASS; gates 6/9 SKIP (gitleaks, cargo-cyclonedx not on box); gate 10 WARN 70 IPv4 + 1 quick-tunnel |
| GitHub Actions launch-gates run 39692890186, job "public export + gates", 2026-10-22 01:22 | 7c95348b | gate 7 PASS 2063/1996/0/67; job FAILED on gate 6 gitleaks (4 findings, redacted), not REUSE |
| after this branch's commit, `comply lint` over `git archive HEAD` | swarm/reuse-gate-fix | PASS 2064 total, 1997 passed, 0 failed, 67 skipped (+1 = the new test file) |

comply 0.1.1 (Rust, MIT OR Apache-2.0) at /workspace/swarm/tools/bin/comply. The FSFE `reuse` tool is Python and is not run (Rust-only rule).

## New test: crates/galion-realtest/tests/real_reuse_gate.rs

* `reuse_b10_static` (always on): the 2 string-fixture files are in a
  `precedence = "override"` block of REUSE.toml; all 5 candle-core sidecars
  exist with licence and copyright lines; every licence id in the sidecars and
  REUSE.toml has `LICENSES/<id>.txt`.
* `real_reuse_lint_head` (`GALION_REAL=1`): runs real `comply` over
  `git archive HEAD` in the system temp dir (same as gate 7), expects 0
  problems and "0 failed". Negative control (a): a tiny tree with one
  header-less file must fail on that file only. Negative control (b): HEAD
  tree with the B10 fix undone (test.npy.license deleted, bench-banking
  override removed) must fail on exactly those 2 files.

Run (18:27-18:28):

```
GALION_REAL=1 GALION_TOOLS_BIN=/workspace/swarm/tools/bin cargo-slot test -j 2 -p galion-realtest --test real_reuse_gate -- --nocapture
test reuse_b10_static ... ok
comply lint --lines HEAD: ok=true problems=0
comply lint HEAD: 2064 total, 1997 passed, 0 failed, 67 skipped
negative (a): ok=false  bad.rs: no license identifier / no copyright notice
negative (b): ok=false  crates/bench-banking/src/axes.rs: invalid SPDX License Expression ... ; vendor/candle-core/tests/test.npy: no license identifier / no copyright notice
test real_reuse_lint_head ... ok
test result: ok. 2 passed; 0 failed
```

No ports used (the row assigns 21370-21379; not needed). No new dependencies.

## Live context (VPS, read-only)

2026-10-27 18:27 (+01:00), over ssh with GET only: nerve 127.0.0.1:8670
`/healthz` ok, `/state` cycle 3, score 63/D, verdict "degraded: 3 open
incident(s) (0 critical...)"; cortex 127.0.0.1:8674 GET / 200. REUSE is a
repository licence gate, so nothing is deployed and the live services don't
change. The probe only records the live state.

## Gates

Box, 2026-10-27 18:28-18:33 (+01:00), target /workspace/swarm/target/reuse-gate-fix: `cargo fmt --all -- --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok; `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` 375 suites, 1974 passed, 0 failed, 18 ignored; `scripts/no-python.sh` ok. Sandbox tests are not run on the box (sandbox_vps_only), and this change does not touch the sandbox. Only one test file changed, with no runtime code, so the musl/android/armv7 cross builds were not re-run.

## Open / owner

* B10 can be closed against main-staging 7c95348b. It stays open on main
  13a296c until main-staging is merged.
* The remaining CI blocker for the public-export job is gate 6 (gitleaks,
  4 findings on 7c95348b). That's a separate blocker and is not changed here.
