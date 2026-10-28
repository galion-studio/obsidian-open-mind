<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: hermes-gateway deploy candidate vs live b7fbbc78 (slot 416)

Branch `swarm/hermes-gateway-candidate`, base 83e573e2. All times Europe/Zagreb, CET (+01:00), 2026-10-28.
Recipe and runbook: `docs/release/hermes-gateway-candidate-20261028.md`. Nothing was deployed.

Test: `crates/hermes-gateway/tests/real_candidate_parity.rs` (needs `GALION_REAL=1`). It starts the candidate
and the live binary side by side on 127.0.0.1:22860 (live) / 22861 (candidate), and 22862/22863 for the
coding-on pair, with a throwaway home, a mock provider and no real API key, then checks every port is closed.

| part | what it checks |
|---|---|
| P1 | sha256 + size of both; candidate <= 2 MiB; candidate sha == the sha in the staged drop-in; live sha == c5432a88… |
| P2 | `--check-config` on the live unit shape: same parsed config line; worktree under `/var/lib`: live exit 0 (blind spot), candidate exit 2 |
| P3 | same status + scrubbed body on `/health`, `/v1/health`, `/health/detailed`, `/v1/models` (auth and no auth), `/v1/capabilities`, `/api/sessions`, `/api/jobs`, a 404 route; same chat reply |
| P4 | tools list with coding off (live shape): mock `tool:<name>` reaches only `clock`, `delegate_task`; no "coding: tools on" |
| P5 | VPS only (`HERMES_PARITY_SANDBOX=1`, refused on the box): coding on with a git worktree, same "tools on" list, same reachable tools, same sandboxed preflight notes, sandboxed `fs_read` returns the file |

## Runs

| time (CET) | where | binaries | result |
|---|---|---|---|
| 18:34:01 | box | candidate t-cand-gcc vs `/workspace/swarm/artifacts/t291u/hermes-gateway-b7fbbc78` | fail: P2 compare included the bind port (test bug, fixed: same port for both, `--check-config` never binds) |
| 18:34:21 | box | same | fail: `/health/detailed` has `pid` (test bug, fixed: scrubbed) |
| 18:34:36 | box | same | **pass** P1–P4 (P5 skipped on the box by design); ports 22860-22869 free after |
| 18:35:25 | VPS, `/var/tmp/sandbox-test-416-20261028183525`, as `nobody`, `env -i` | candidate copy vs `/opt/galion/rust/bin/hermes-gateway-b7fbbc78` (read and run only) | fail at P5: preflight notes differ only in the per-binary worktree path (test bug, fixed: path prefix normalised). P1–P4 passed |
| 18:36:32 | VPS, same folder | same | **pass** P1–P5 in 0.94 s; ports free; no `nobody` process left |
| 18:36:50 | VPS | — | throwaway folder removed; hermes-rs active, MainPID 255539, NRestarts 0, ExecStart b7fbbc78, drop-ins unchanged (50/60/70/90), `:8642/health` 200, nerve `:8670/state` 200, gaia-cortex active |

VPS P5 facts: landlock_abi=0, userns=true, so both binaries say "WARNING: no Landlock; relying on the mount
namespace only" (run with `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`); both report "tools on: fs_read, fs_write, git,
cargo"; with no `HERMES_RS_CODING_TOOLCHAIN` the cargo tool refuses every call (same in both). Live
`--check-config` prints no coding line; the candidate prints `coding=on worktree=… on_error=exit` (the 657a9988 fold).

## Reproducibility

| time (CET) | build | sha256 |
|---|---|---|
| 18:17:14 | `git archive 989958be`, `GALION_BUILD_COMMIT=989958be…`, `SOURCE_DATE_EPOCH=1793206972` | 0f7bb471…8ac8 (1,952,592 B) |
| 18:37:54 | branch checkout 80c37b12 (other path), `GALION_BUILD_COMMIT=0000…`, `SOURCE_DATE_EPOCH=1`, fresh target dir | 0f7bb471…8ac8 (same) |

Live b7fbbc78 rebuilt five ways from its recorded recipe: no match (F-416-1, see the release doc).

## Gates (box, 18:38–18:39 CET)

`cargo fmt --all --check` ok; `cargo clippy --workspace --exclude openmind-sandbox --exclude cgroup-detect
--all-targets --locked -- -D warnings` ok; `cargo test -p hermes-gateway -p hermes-tools` ok (all suites,
incl. `staged_candidate` 3/3); `scripts/no-python.sh` ok; SPDX headers on every new file.
