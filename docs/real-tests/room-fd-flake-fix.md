<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life tests: room fd flake fix (swarm slot 350, ports 22200-22209)

- Written: 2026-10-28T06:00:29+01:00 (Europe/Zagreb)
- Branch: `swarm/room-fd-flake-fix` off main-staging 38a07700 (snapshot 3f015986).
- Box only for cargo tests. VPS: read-only nerve probe. No deploy, no live change.

## What was wrong

1. `room` lib test `default_and_fixture_do_not_open_a_socket` counted sockets in
   `/proc/self/fd`. That list is process-wide, and the `http` tests in the same
   binary bind and connect at the same time, so in full workspace runs it failed
   now and then (slots 332, 334). Before the fix: 27/200 fails.
2. Seen while testing (full ws run 10/20, first batch): `galion-boot`
   `secrets_restore_rules` failed with ETXTBSY. A parallel `fork` can inherit the
   write fd of the freshly written `fake-age` script until it `exec`s.

## Fix (test code only, no product code changed)

- 9eeb86ba: the probe test re-runs itself in a child process
  (`--exact --test-threads=1`, env `ROOM_FD_PROBE_CHILD=1`); the parent checks
  the child passed and ran exactly one test. Std sockets are `O_CLOEXEC`, so the
  child starts clean.
- 4b7130f1: `fake_age()` waits (bounded ~2 s) until the script can be executed,
  retrying only on ETXTBSY.

## Evidence (box, Europe/Zagreb)

| run | what | result |
|---|---|---|
| 04:08-04:34 | 20 full ws runs, room fix only | room probe 20/20 ok; 1 boot ETXTBSY (run 10) -> fix 2 |
| 04:45-05:12 | 20 full ws runs, both fixes | room 20/20 ok, boot 20/20 ok; runs 18+19 failed only on openmind-nerve `idle_is_quiet_and_burst_escalates_once` (separate flake, fixed by slot 360 cde9c5e1) |
| 05:48-05:58 (r1) | 5 full ws runs (`--exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`) | 5/5 rc=0, 2004 passed / 0 failed / 18 ignored each |
| r1 | room lib test binary x300 | 0 fails |
| r1 | galion-boot `tests/boot` binary x200 | 0 fails |
| slot 360 | ws with 350 + 360 temp-merged | 4/4 clean (2005/0/18) |

Gates (r1): `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -D warnings` ok;
`scripts/no-python.sh` ok; CJK scan ok; repo-check (musl release) PASS, 0 errors, 83 warnings (same as base).

Rule note: the first 40 runs (before r1) did not exclude `hermes-tools`, so its
`tests/sandboxed.rs` (10 Landlock tests) ran on the box. That is a sandbox_vps_only
slip; r1 runs excluded it. No sandbox code was changed here.

Not run: `GALION_REAL=1 real_room` (hard-coded to slot-08 ports 19080-19088, outside
this slot's range; see refill row 363 real-test-port-env). The change does not touch it.

## VPS (read-only, 2026-10-28 05:48 CET)

- Nerve 127.0.0.1:8670 /state: 200, cycle 55, 16 checks healthy (others: degraded/failing/loading/absent, unrelated).
- Cortex :8672 not listening (not deployed); observed through the nerve.

## Owner

- D-350-1: land 9eeb86ba + 4b7130f1 into main-staging together with slot 360 (9dceed0a + cde9c5e1) so full ws CI runs stop flaking.
