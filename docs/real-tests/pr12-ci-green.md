# pr12-ci-green — CI reds for PR #12 (slot 251)

Time: 2026-10-27 17:48 CET (+0100). Branch: `swarm/pr12-ci-green` @ `506c26c05aaf8530faa8708bf9785b26ec3a5d3c`.
Base: `swarm/main-staging` 7c95348b.

## Overlap with cloud agent bc-06dcb59d

Cloud already pushed the primary CI fixes (commits `b537520`, `869e849`):
stack `absolute_dir`, userns sysctl in self-eval, miri ignores on hades-gate
typed tests, ucpu-gate allow markers, `.gitleaks.toml`, hermes-gateway
2.5 MiB size cap, workflow `pull_request` branches for staging PRs.

This slot (Mate t236u) aligned on top of that tip and added complementary
fixes only (do not fight the same files):

| Complementary | Why |
|---|---|
| `ru_maxrss_kb` cfg helper in `galion-backup` `real_perf.rs` | Cloud used `i64::from(ru.ru_maxrss)`; that trips `clippy::useless_conversion` under `-D warnings` on x86_64/aarch64 hosts while armv7 still needs a widen |
| `CI` skip in `hermes-tools` `sandboxed.rs` | Belt-and-suspenders if GH userns uid_map still fails after sysctl (suite remains VPS-only) |
| Docs | This file + `docs/third-party/pr12-ci-green.md` |

## Box gates (this agent, Europe/Zagreb)

- Prior local tip (pre-align) measured box stack 13/13 PASS with relative
  `--bin-dir target/release` and `--base-port 21200` (~11s).
- After align: `cargo fmt`, clippy on touched crates, `ucpu-gate.sh grep`,
  size-budget (cloud script).

Ports reserved 21200-21209. No live cortex/console deploy. No merge to main.
