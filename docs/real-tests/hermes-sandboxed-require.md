<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: hermes-sandboxed-require (wave-4 slot 346, F-335-1)

Branch `swarm/hermes-sandboxed-require` off ws-test-recipe 132a5e9a. All times
Europe/Zagreb CET (+01:00). No deploy, no live change. Ports 22160-22169
reserved, none needed.

## Change
- `cargo xtask test` sets `GALION_SANDBOX_TESTS=<mode>` on the inner
  `cargo test` and prints `xtask: test env ...` (`xtask::child_env`).
- `hermes-tools/tests/sandboxed.rs` `skip()` now uses the pure decision in
  `tests/common/sandbox_gate.rs`: auto keeps the `CI` skip and the old
  feature skip; force lifts only the `CI` skip; **require never skips** and
  panics naming what the host lacks (Landlock unless
  `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`, userns, git/dd/install), mirroring
  `xtask::missing("hermes-tools")`. The `CI` skip is decided before
  `openmind_sandbox::features()` so nothing is probed when skipping.
- New pure test `hermes-tools/tests/sandbox_gate.rs` (decision table, probes
  nothing, safe on any host).

## 1. Swarm box (2026-10-28 03:27:08), sandbox_vps_only
```
xtask: host linux=yes landlock=yes userns=yes cgroup2=yes euid=1000 vps_only=yes
test --sandbox=require        -> refused: --sandbox=require on a sandbox-VPS-only host   rc=3
CI=1 test --sandbox=require   -> refused (same)                                           rc=3
test --dry-run (auto)         -> exclude all three, cargo test --workspace --exclude ... -j 2
                                 xtask: test env GALION_SANDBOX_TESTS=auto                rc=0
```
On the box only pure tests ran: xtask lib 10/0 (+ child_env_names_the_mode),
xtask tests/recipe.rs 5/0 (now also asserts the test env line for
require/auto/off), hermes-tools sandbox_gate 5/0. `sandboxed.rs` was compiled,
never run here. fmt ok; clippy -p xtask -p hermes-tools --all-targets -D warnings ok.

## 2. Live VPS as uid 65534 (2026-10-28 03:28:20 - 03:28:35)
Static x86_64 musl test binaries (sandboxed sha256 97e425e1341c0b0b..., sandbox_gate,
xtask) copied to `/var/tmp/sandbox-test-346-<ts>` (owned 65534), each run via
`setpriv --reuid=65534 --regid=65534 --clear-groups env -i` with
HOME=TMPDIR=<dir>/tmp. Kernel 6.8.12-43-pve, LSM
lockdown,capability,yama,apparmor (no Landlock).

| Case | Env | Result |
|---|---|---|
| A xtask probe | - | landlock=no userns=yes euid=65534; hermes-tools lacks landlock |
| B xtask test --sandbox=require --dry-run | - | `hermes-tools needs landlock on this host` rc=3 |
| C xtask test --dry-run | - | exclude hermes-tools, `test env GALION_SANDBOX_TESTS=auto` rc=0 |
| D sandbox_gate | - | 5 passed / 0 failed |
| E sandboxed | CI=1 (auto) | 10 "passed", every test prints SKIP (8 `CI=set, GALION_SANDBOX_TESTS auto`, 2 own GALION_REAL/SMALL_FS gates) rc=0 — old behaviour kept |
| F sandboxed | CI=1, require | **FAILED 2 passed / 8 failed**, all 8 `host lacks landlock (require never skips ...)` rc=101 |
| G sandboxed | require, no CI | FAILED 2/8 rc=101 |
| H sandboxed | CI=1, require, ALLOW_NO_LANDLOCK=1 | 10 passed / 0 failed (1.78 s): the suite really ran under CI=1 |
| I sandboxed | auto, no CI | FAILED 2/8 (pre-existing F-320-1 without Landlock, unchanged) |
| J sandboxed | CI=1, force, ALLOW_NO_LANDLOCK=1 | 10 passed / 0 failed |

The 2 that pass under require are `real_default_512_mib_cap_stops_fs_write`
and `real_partial_copy_back_on_a_full_host_fs_is_reported`: they return early
on their own opt-in gates (GALION_REAL=1, HERMES_TOOLS_SMALL_FS) before
`skip()` (F-346-1, left as is).

Cleanup 03:28:35: folder removed, 0 uid-65534 test processes left. After:
hermes-rs MainPID 255539 NRestarts 0, galion-desk 264677 NRestarts 0 (same as
slot 335), nerve 127.0.0.1:8670/state 200. Cortex: not probed.

## Owner items
- D-346-1: once a real GitHub self-eval run happens (D-335-2), check the
  hermes-tools `sandboxed` lines show `ok` without SKIP; if the runner cannot
  write uid_map the job now goes red (intended).
- F-346-1: the two `real_*` tests still SKIP in CI under require (they need
  GALION_REAL=1 / a small fs); decide whether require should also cover them.
