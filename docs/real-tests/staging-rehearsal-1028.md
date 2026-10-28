<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real checks behind docs/release/staging-rehearsal-20261028.md (slot 407)

All times Europe/Zagreb, CET +01:00. Box = swarm build box (loopback only).
VPS = read-only probes (systemctl is-active, sha256sum, GET on /healthz,
:8674/health, :8080/console). Nothing restarted, deployed or chmod-ed on the
VPS. No secrets read or printed. Disclosure: at 18:42:37 one probe wrote its
curl output to `/tmp/.r407s` on the VPS (33 694 B); it was deleted at
18:42:45. No other VPS write.

Gates per step: `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace --exclude
openmind-sandbox --exclude cgroup-detect --no-fail-fast` (GALION_REAL unset;
sandbox crates are VPS-only), `scripts/no-python.sh`. Logs on the box:
`/workspace/swarm/logs/r407/`.

## Phase 1 (onto old main-staging 98466ab9) - superseded by phase 2

S0 2025/0/18, S1 2038/0, S2 2048/0, S3 2051/0, S4 2122/2 -> fix 2124/0,
S5 2278/9 -> fix 2287/0/18 (427 suites). Tip 3f44b649 conflicts with main
7ba85b60 in 9 files: not a land candidate.

## Phase 2 (onto main 7ba85b60, tree a6f6c163 rebuilt and verified blob by blob)

| Step | Commit | fmt | clippy | tests pass/fail/ign (suites) | no-python | Cargo.lock after build |
|---|---|---|---|---|---|---|
| main | 6966c8da | ok | ok | 2241/0/18 (430) | ok | clean |
| +388 | cee8d86f | ok | ok | 2253/0/18 (433) | ok | clean |
| +389 | 621b6e2d | ok | ok | 2273/**1**/18 (436) | ok | re-sorted (F-407-7) |
| +386 | 86bf3b88 | ok | ok | 2284/**1**/18 (437) | ok | re-sorted (F-407-7) |
| +383 | 4d4a9934 | ok | ok | 2287/**1**/18 (437) | ok | clean |
| +387 | 7cdf8ad4 | ok | ok | 2319/**1**/18 (443) | ok | clean |
| F-407-4 | 913eca57 | ok | ok | **2320/0/18 (443)** | ok | clean |

The one failure from +389 on is `gaia-core every_workspace_crate_is_mapped`
(new crate galion-survival unmapped); F-407-4 maps it. Also on 913eca57:
gaia self-check ok, xtask corpus lib 4/0 + replay_corpus 4/0, spdx-scan 832
files 0 missing.

## Phase 2 real tests (GALION_REAL=1 per command, box loopback, tip 913eca57)

| Crate / test | Ports | Window | Result |
|---|---|---|---|
| openmind-memory real_freshness | 22774 | 18:55:00-18:55:27 | ok 2/0 |
| openmind-memory real_local_time | 22775 | 18:55:27-18:55:32 | ok 1/0 |
| console-static real_landing | base 22770 (+2, +3) | 18:55:32-18:55:47 | ok 1/0 |
| console-static real_survival_refresh | 22590-22597 (live VPS leg off) | 18:55:47-18:56:29 | ok 1/0 (36 s) |
| openmind-nerve real_cortex_t42 | 20620-20624 | 18:56:29-18:57:11 | ok 1/0 (15 s) |
| openmind-nerve real_replay_corpus | 20630-20639 | 18:57:11-18:57:18 | ok 3/0 |
| gaia-cortex real_handoff_exclude | 22570-22579, nerve = release build of this tree | 18:57:41-18:59:30 | **FAILED** (F-407-6) |

real_handoff_exclude evidence (18:59:30): off phase 5 test tickets, control
rate-limited; on phase (`noloop*,sbxtest*`) 0 test tickets, 10/10 test
escalations excluded, 5 test circuits open, control unit R4 handed to
Hermes, control R2 marked `duplicate` -> 1 control ticket, the test expects 2.
The exclusion itself works; the assertion is tied to the old nerve 9d1a4f3
ticket rules. Owner decision D-407-4. The same failure showed in phase 1.

Not run: nerve real_cross_target (heavy musl pin build; the size-budget
cross-check `budget_matches_size_budget_tsv` runs in the normal gate and
passed), sandbox crates (VPS only, not run in this throwaway rehearsal).
