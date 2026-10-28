<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# PR #12 CI check (wave-4 slot 360, pr12-ci-fix)

Written 2026-10-28 05:40 CET (+01:00). Branch `swarm/pr12-ci-fix` off the
local snapshot 3f015986, whose tree 4864589f is byte-identical to the PR #12
head on GitHub (`swarm/main-staging` 38a07700, the PR #13 merge). Nothing was
pushed to the PR branch, main or main-staging. Nothing was deployed.

## 1. GitHub checks on the PR head (read-only, GitHub API)

All 21 check runs on 38a07700 are finished (2026-10-27 20:03-20:12 CET):
20 success, 1 skipped on purpose.

| Workflow | Checks | Result |
|---|---|---|
| self-eval | fmt, clippy -D, cargo test, omnode, ucpu tier-1 x86_64 + aarch64, ucpu tier-2 armv7 + android, ucpu qemu, repo-check, no Python, no CJK, licence (MIT), Zagreb timestamps, gitleaks | success |
| realtest | box stack scenarios | success |
| launch-gates | public export + gates | success |
| miri (push) | 3 short miri jobs | success |
| miri (push) | miri full (pass 1, nightly) | skipped: runs only on `schedule` or `workflow_dispatch` with `full` (miri.yml line 74) |

The last red runs were on 98466ab9 (2026-10-27 17:47 CET). PR #13 fixed them.
"Zagreb timestamps" is warning-only: 203 warnings, mostly test fixtures
(128 in crates/openmind-nerve, 12 in zagreb-time, 11 in openmind-memory).

So no check is red on GitHub right now. The work below fixes two timing
flakes in the `cargo test` job. Neither failed on GitHub's last run, but both
fail on a busy runner, so the job could turn red without a code change.

## 2. Local repeat of the CI jobs on the PR head tree (box)

| Job | Command | Result |
|---|---|---|
| fmt | `cargo fmt --all -- --check` + edge/openmind-node | ok |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | ok |
| omnode | clippy -D + test on edge/openmind-node | ok |
| no Python | `scripts/no-python.sh` and `git ls-files '*.py'` | ok, 0 files |
| no CJK | the CI grep | ok |
| repo-check | musl release `repo-check --copyright ...` | PASS, 0 errors, 83 warnings |
| cargo test | workspace without openmind-sandbox, cgroup-detect, hermes-tools (sandbox_vps_only) | **flaky**, see below |

`cargo test` on the unchanged head: run 1 failed fast in
`openmind-queue --test refusals_persist` (WouldBlock: the data dir "is in use
by another process"). Run 2 (`--no-fail-fast`, after fix 1) had 2004 passed
and 1 failed: the room `default_and_fixture_do_not_open_a_socket` flake,
which slot 350 owns. A later run hit a third flake, in openmind-nerve
`idle_is_quiet_and_burst_escalates_once` (wakes 9 > 8).

## 3. Fix 1: openmind-queue `Handle::stop` frees the store lock (9dceed0a)

Cause: each connection thread holds an `Arc<Shared>`, and the store's `flock`
lives inside it. The thread answers the client before it drops that Arc.
`stop()` joined only the acceptor and ticker threads. So an in-process
restart on the same dir could race the last connection thread and get
WouldBlock.

Fix: after the joins, `stop()` waits until `Arc::strong_count == 1`, polling
every 2 ms, for at most 12 s (`STOP_DRAIN`; connection I/O times out after
10 s). The production binary (`openmind-queue serve`) uses `join()`, not
`stop()`, so live behaviour does not change.

| Measurement | Before | After |
|---|---|---|
| `refusals_persist` test binary, 1000 serial runs | 13/1000 failed | 0/1000 |
| new `tests/stop_releases_lock.rs` (500 start/3 GET/stop/Store::open rounds per run, port 22300) | 11/40 runs failed | 0/100 |
| GALION_REAL=1 release real tests: real_auth, real_binary_survives_sigkill, real_refusal_persist x2 | n/a | 4/4 pass (05:25 CET) |

## 4. Fix 2: openmind-nerve burst test holds the hub lock (cde9c5e1)

Cause: the test sends 31 events and expects at most 8 loop wakes. On a busy
box the loop thread can drain between sends, which gives one wake per few
events (a full workspace run saw 9).

Fix: the test holds `hub.lock()` while it sends. The loop blocks on its first
publish, the rest queue up, and the whole burst becomes exactly one drained
wake. The test now asserts `wakes_by.journal == 1` (stricter), and keeps the
old 2..=8 total bound. Only test code changed.

| Measurement | Before | After |
|---|---|---|
| that test alone, 6 parallel workers | 1/72 failed (wakes 9) | 0/96 with the stricter assert |
| openmind-nerve lib | | 217/0 |

## 5. Combined runs (this branch + slot 350 room-fd-flake-fix 4b7130f1, local temp merge only)

Full workspace `cargo test --locked --no-fail-fast` (excluding the sandbox
crates as above):
- Before fix 2: 4 runs. 3 clean (378 suites, 2005/0/18), 1 failed on the
  nerve flake.
- After fix 2: 4/4 clean, each 378 suites, 2005 passed, 0 failed, 18 ignored
  (05:35-05:39 CET).

Clippy -D on the workspace stayed clean after both fixes. openmind-queue also
builds for x86_64 + aarch64 musl release and passes `cargo check` for
aarch64-linux-android + armv7-musleabihf.

## 6. Which checks would go green

All 21 are green today. With this branch plus slot 350's branch, the box
reproduction of `cargo test` is free of the three known flakes (queue lock,
nerve burst, room fd). The box cannot run hermes-tools or the sandbox crates
(sandbox_vps_only), but CI runs them and they passed on 38a07700.

## 7. Live VPS (read-only, 2026-10-28 05:20 CET)

- Nerve 127.0.0.1:8670 /state: 200, mode live, cycle 53.
- Cortex :8672 not listening (not live). Observation used the nerve.
- POSEIDON `openmind-queue.service`: active, MainPID 294, NRestarts 0, up since
  2026-10-22 23:46:47 CET. GET /stats 200.
- The live binary uses `join()`, so fix 1 does not change its runtime path.
  Nothing was written on the VPS.

## Owner decisions

- D-360-1: land 9dceed0a + cde9c5e1 together with slot 350 (9eeb86ba,
  4b7130f1) into main-staging, so PR #12's `cargo test` stops depending on
  scheduler luck.
- No new dependencies, no third-party code.
