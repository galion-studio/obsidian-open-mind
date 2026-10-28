<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: ci-miri-schedule (D-M1), 2026-10-21 +02:00

Owner decision D-M1 (Mate, 2026-10-21 17:04 +02:00): a short Miri run of about 3 minutes on every push,
the full run of about 25 minutes on a nightly schedule. Branch `swarm/ci-miri-schedule`, base
`swarm/sec-miri-pure` 91353ba. No GitHub settings changed.

## Workflow `.github/workflows/miri.yml`

- Toolchain `nightly-2026-10-20` (the box's nightly: manifest date 2026-10-20, rustc 07e526db9
  2026-10-19, miri 0.1.0 - the one sec-miri-pure ran on), components `miri, rust-src`.
- `miri-short`, on every push (any branch) and on manual dispatch: pass 1 (isolation, Stacked Borrows) on
  galion-common, hades-gate, openmind-schema minus the two slow galion-common targets `nist` (FIPS B.3,
  one million `a`) and `time_vs_chrono` (`--exact --skip` of their six tests). Split into three parallel
  matrix shards, because one serial run is ~5 min on the box:
  `-p galion-common --lib` | `-p galion-common --test *` | `-p hades-gate -p openmind-schema`
  (`set -f` so `*` reaches cargo as its target glob). Timeout 15 min each.
- `miri-full`, cron `17 0 * * *` (02:17 +02:00 summer, 01:17 +01:00 winter) and manual dispatch with
  `full: true`: the whole pass 1, nothing skipped. Timeout 60 min.
- Guard: `crates/galion-realtest/tests/miri_workflow.rs` (4/0): triggers, SPDX, same dated nightly in both
  jobs, short only on push / full only on schedule, timeouts, the three shards cover exactly the three
  crates, the skip list equals the test functions of `nist.rs` + `time_vs_chrono.rs` and each name is unique
  across the three crates (so `--exact --skip` hides nothing else), negative controls.
- `actionlint 1.7.7` (downloaded to /tmp, not committed): no findings.

## Box timings (`CARGO_TARGET_DIR=/workspace/swarm/target/ci-miri-schedule`, `-j 2`, `+nightly`)

| run | command | start | wall | result |
|---|---|---|---|---|
| short, serial, cold build | `cargo +nightly miri test -j 2 --locked -p galion-common -p hades-gate -p openmind-schema -- <skip>` | 17:06:28 | 316 s | 23 suites 56/0/36 ignored |
| short, serial, warm | same | 17:11:44 | 293 s | 23 suites 56/0/36 |
| shard common-lib, warm | `-p galion-common --lib` | 17:17-17:30 | 150 s | 21/0/1 |
| shard common-tests (http_fuzz is the cost) | `-p galion-common --test http_fuzz` | same | 66 s | |
| shard gate-schema | `-p hades-gate --lib` 58 s + `--test redteam` 27 s + `-p openmind-schema` 5 s | same | ~90 s | |
| three shards at once on one box (load 17-18 from other agents' release builds) | the three shard commands | 17:29:58 | 277 / 472 / 578 s | 21/0/1, 5/0/8, 30/0/27 |
| full | `cargo +nightly miri test -j 2 --locked -p galion-common -p hades-gate -p openmind-schema` | 17:16:37 | 5106 s (load 15-29, see below) | 23 suites 62/0/36 = short + the 6 slow tests |

Short: on a quiet box the longest shard is ~150 s plus build, i.e. ~3 min wall per push on three runners
(the serial run is ~5 min). The "all three at once" row ran the shards on one 8-vCPU box with load
17-18 and is not representative of three separate runners.

Full: started 17:16:37 +02:00 while the box load rose to 15-29 (other agents' release builds; Miri got
~28 % of a core). It finished 18:41:43 +02:00, exit 0, 62 passed / 0 failed / 36 ignored, 0 UB (nist 3/0 with Miri clock
1369 s vs 421 s quiet; time_vs_chrono 3/0 after ~68 min starved). The 85 min wall is contention, not the job. sec-miri-pure measured the
same pass 1 at about 25 min wall on a quiet box (galion-common nist 421 s, time_vs_chrono 1108 s), which is
the nightly budget; the job timeout (60 min) leaves 2x headroom.

## Other gates

`cargo fmt --all -- --check` ok; `cargo clippy -p galion-realtest --all-targets -- -D warnings` ok;
`cargo test -p galion-realtest` 5 suites 16/0/0; `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` missing 0.
Workflow-only change plus one test file, so no workspace-wide run.

## Open for the owner

- D-MS-1: re-time the full job on a quiet box or the first nightly run; if it exceeds ~30 min, move
  `time_vs_chrono` to a weekly job.
- D-MS-2: the short job costs three runners per push (about 3 min each) instead of one (about 5 min).
