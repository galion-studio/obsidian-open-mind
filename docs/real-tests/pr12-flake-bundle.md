<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# PR #12 flake bundle (wave-4 slot 374, pr12-flake-bundle)

Written 2026-10-28 07:55 CET (+01:00). Branch `swarm/pr12-flake-bundle` off
`swarm/pr12-ci-fix` b721a689 (PR #12 head tree + slot 360 queue/nerve
fixes). One candidate branch for owner decisions D-350-1 + D-360-1. Nothing
was pushed to the PR #12 branch, main or main-staging. Nothing was deployed.
Test-only changes; no product code beyond what slot 360 already carried.

## 1. What is in the bundle

| commit | origin | fix |
|---|---|---|
| 9dceed0a | slot 360 (in base) | openmind-queue `Handle::stop` waits for in-flight connection threads; refusals_persist WouldBlock flake |
| cde9c5e1 | slot 360 (in base) | openmind-nerve `idle_is_quiet_and_burst_escalates_once` holds the hub lock during the burst |
| 3011c042 | cherry-pick of slot 350 9eeb86ba | room no-socket fd probe runs in its own single-test child process |
| 278706fd | cherry-pick of slot 350 4b7130f1 | galion-boot `secrets_restore_rules` waits out ETXTBSY on fake-age |
| 5d03a5ce | new, slot 374 | gaia `heal_loop` runtime_budget / stop_file timing asserts get slack (F-374-1) |

Both cherry-picks applied cleanly (diff vs base: crates/room/src/lib.rs +37,
crates/galion-boot/tests/boot.rs +17/-1).

## 2. F-374-1: new flake found (gaia heal_loop)

Full ws run 1 on 278706fd (07:24-07:25 CET, cold binaries, a release build
running beside it) failed two gaia `tests/heal_loop.rs` tests together:
`runtime_budget_stops_the_loop` (2 ticks, assert wanted 3..=5) and
`stop_file_ends_a_sleeping_loop_within_a_slice` (stop seen >350 ms after the
file was written). Both are wall-clock asserts; they failed in the same
second, which points at a process-wide stall (scheduler or cold page-in),
not at loop logic. Reproduction attempts on the old binary: 0/60 serial,
0/150 (6 in parallel), 0/20 at nice 10 against 10 busy loops. Old-head runs
2-6 were clean.

Fix 5d03a5ce (test only): runtime elapsed upper bound 1600 -> 2500 ms, ticks
3..=5 -> 2..=5, stop lag 350 -> 1500 ms (still 20x under the 30 s interval,
so the slice watch is still what is tested). After: heal_loop 0/100 (5 in
parallel). Owner may drop this commit if CI never shows it (D-374-1).

## 3. Full workspace runs (box)

Command: `nice -n 10 cargo test --workspace --no-fail-fast --exclude
openmind-sandbox --exclude cgroup-detect --exclude hermes-tools -j 4`
(sandbox_vps_only respected; hermes-tools excluded so no Landlock test ran
on the box).

Final head (5d03a5ce + this doc), 15 runs 07:32-07:54 CET:

| runs | result |
|---|---|
| 1-3 | 378 suites, 2005 passed / 0 failed / 18 ignored |
| 4 | 1 fail: openmind-queue `stop_releases_lock` bind 127.0.0.1:22300 AddrInUse; my own targeted stress of that same binary was running at the same time (self-inflicted, not a code flake) |
| 5 | 1 fail: galion-deploy `systemd_backend` bind 127.0.0.1:19394 AddrInUse; slot 373 started its own full ws run in wt/land-order-gateway-size at 07:37:51, same fixed port (cross-agent collision) |
| 6-15 | 10 consecutive clean runs, each 378 suites, 2005/0/18 |

So 10 consecutive full ws runs with 0 flakes (runs 6-15). Pre-fix head
278706fd: run 1 red (F-374-1), runs 2-6 clean 2005/0/18.

Targeted stress on final head (box, 07:37-07:40 CET): room lib 0/100,
galion-boot secrets_restore_rules 0/50, openmind-queue refusals_persist
0/200, stop_releases_lock 0/50, openmind-nerve idle_is_quiet 0/50.

## 4. Gates

- `cargo fmt --all --check`: ok
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox
  --exclude cgroup-detect -- -D warnings`: ok (gaia re-checked after 5d03a5ce)
- `scripts/no-python.sh`: ok; CJK scan (CI regex): ok
- repo-check (musl release, `--copyright "3000 Galion / galion-studio"`):
  PASS, 2120 files, 0 errors, 83 warnings (same as base)

## 5. VPS (read-only)

2026-10-28 07:28 CET: nerve 127.0.0.1:8670 `/state` 200; cortex
127.0.0.1:8674 `/health` 200. No VPS writes.

## 6. Open items for the owner

- D-374-1: keep or drop 5d03a5ce (heal_loop slack).
- F-374-2: fixed loopback ports in tests (22300, 19394, 20311, ...) collide
  when two agents run full ws on one box at the same time. Not a CI problem
  (isolated runners); a port-0 sweep would make box runs safe.
- Land order: this branch can replace the separate 350 + 360 land steps.
