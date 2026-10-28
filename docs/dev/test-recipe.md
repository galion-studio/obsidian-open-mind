<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Default workspace test recipe: `cargo xtask test`

Wave-4 slot 335 (ws-test-recipe-exclude), owner item D-320-2, 2026-10-28 CET (+01:00).

## Why
`openmind-sandbox`, `hermes-tools` and `cgroup-detect` run real sandboxes or
probe the sandbox host in their tests. Plain `cargo test --workspace`:

- fails on hosts without the kernel features (live VPS has no Landlock:
  hermes-tools `sandboxed.rs` 2/8, F-320-1), and
- runs sandboxes on the swarm box, which RULES.md `sandbox_vps_only` forbids
  (slot-294 slip).

So every gate carried a hand-written `--exclude openmind-sandbox --exclude cgroup-detect [--exclude hermes-tools]`.

## Use
```
cargo xtask test                       # default: auto
cargo xtask test -- --locked -- -q     # args after -- go to cargo test
cargo xtask test --dry-run             # print the decision and the cargo line only
cargo xtask probe                      # host facts + per-crate verdict
```
`cargo xtask` is an alias in `.cargo/config.toml` (`run --quiet -p xtask --`).
On the swarm box use `cargo-slot xtask test -- -j 2`.

| Mode | Flag / env | Sandbox crates |
|---|---|---|
| auto (default) | none, `GALION_SANDBOX_TESTS=auto` | each one runs only if the host has its features and is not sandbox-VPS-only |
| force (explicit opt-in) | `--sandbox`, `GALION_SANDBOX_TESTS=1` | all three, even where features are missing (refused on a sandbox-VPS-only host, exit 3) |
| require (CI) | `--sandbox=require` | like auto, but exit 3 instead of excluding |
| off | `--no-sandbox`, `GALION_SANDBOX_TESTS=0` | none |

A flag beats the env. `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` (the hermes-tools
override, D-320-1) also lifts the Landlock need for hermes-tools.

## What is probed (files only: no fork, no unshare, no syscall probe, nothing written)

| Fact | Source |
|---|---|
| landlock | `landlock` in `/sys/kernel/security/lsm` (unreadable = no) |
| userns | `/proc/sys/user/max_user_namespaces` > 0 and (euid 0, or `kernel.unprivileged_userns_clone` != 0 and `kernel.apparmor_restrict_unprivileged_userns` == 0; absent knobs = allowed) |
| cgroup2 | a `cgroup2` line in `/proc/self/mounts` |
| euid | `/proc/self/status` `Uid:` field 2 |
| sandbox-VPS-only | `/workspace/swarm/RULES.md` contains `sandbox_vps_only`, or `GALION_SANDBOX_VPS_ONLY=1` |

| Crate | Needs |
|---|---|
| openmind-sandbox | Landlock or userns |
| hermes-tools | Landlock (or the override) and userns |
| cgroup-detect | cgroup2 |

`GALION_XTASK_HOST_ROOT=<dir>` reads these files below `<dir>` instead of `/`
(fixtures in `crates/xtask/tests/fixtures/`). Such a run is a simulation and
never starts cargo, so the guard cannot be bypassed that way.

## Where it is wired
- `.github/workflows/self-eval.yml` test job: after the userns sysctl step,
  `xtask probe` then `xtask test --sandbox=require -- --locked`. On
  ubuntu-24.04 (Landlock on, userns after the sysctl step, cgroup2) all three
  crates run; if the runner loses a feature the job goes red instead of
  silently dropping the suites.
- `scripts/self-check.sh` full mode: `cargo run -p xtask -- test -- -j 2`.
- RULES.md still says `--exclude openmind-sandbox --exclude cgroup-detect` by
  hand; that text is swarm-owned (not in this repo) and was not changed
  (owner item D-335-3).

## Mode reaches the tests (F-335-1, slot 346)
xtask runs `cargo test` with `GALION_SANDBOX_TESTS=<auto|force|require|off>`
(printed as `xtask: test env ...`). `hermes-tools/tests/sandboxed.rs` reads it
(`tests/common/sandbox_gate.rs`):

| Mode | `CI` set | host lacks Landlock (no override) / userns / git,dd,install |
|---|---|---|
| auto (or unset) | SKIP (sandbox_vps_only belt-and-suspenders) | SKIP only if neither Landlock nor userns, or tools missing (old rule) |
| force | runs | SKIP as in auto |
| require | runs | test FAILS (panic naming what is missing) |

So CI (`--sandbox=require`) now really runs the sandboxed suite instead of
printing SKIP. Proof: docs/real-tests/hermes-sandboxed-require.md.

## Not covered
- `real_default_512_mib_cap_stops_fs_write` and
  `real_partial_copy_back_on_a_full_host_fs_is_reported` keep their own opt-in
  gates (`GALION_REAL=1`, `HERMES_TOOLS_SMALL_FS`) and still SKIP in CI under
  require (F-346-1).
- Clippy/build still compile the three crates everywhere (compiling is not
  running a sandbox).
