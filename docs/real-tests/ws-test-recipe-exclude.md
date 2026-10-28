<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: ws-test-recipe-exclude (wave-4 slot 335, D-320-2)

Branch `swarm/ws-test-recipe` off main-staging 38a07700 (box snapshot 3f015986,
tree 4864589f). All times Europe/Zagreb CET (+01:00). No deploy, no live change.
Ports 22050-22059 reserved, none needed (no HTTP server in this change).

## 1. The default recipe on the swarm box (2026-10-28 01:24:49 - 01:27:07)
`cargo-slot xtask test -- -j 2 --no-fail-fast`
```
xtask: host linux=yes landlock=yes userns=yes cgroup2=yes euid=1000 vps_only=yes
xtask: sandbox mode auto
xtask: exclude openmind-sandbox (host is sandbox-VPS-only (sandbox_vps_only))
xtask: exclude hermes-tools (host is sandbox-VPS-only (sandbox_vps_only))
xtask: exclude cgroup-detect (host is sandbox-VPS-only (sandbox_vps_only))
xtask: cargo test --workspace --exclude openmind-sandbox --exclude hermes-tools --exclude cgroup-detect -j 2 --no-fail-fast
```
Result: 381 suites, 2018 passed / 0 failed / 18 ignored, rc 0. `Running` lines
for hermes_tools / openmind_sandbox / cgroup_detect / sandboxed / real_escape: 0.
The box has Landlock+userns+cgroup2, so feature detection alone would have run
the sandbox suites here; the sandbox_vps_only marker is what keeps them out.

The first run (01:22:52-01:24:12) caught a real break: `gaia --test commands
ownership_counts` (crates_total 68 -> 69, system_crates 56 -> 57 with xtask).
Fixed in this branch. `cargo run` releases the build lock before xtask starts
the inner `cargo test` (no deadlock under cargo-slot).

Opt-in refused on the box (GALION_REAL=1 `real_host_dry_run`, 01:2x):
`--sandbox` -> exit 3 `refused: --sandbox=force on a sandbox-VPS-only host`.

## 2. Live VPS, read-only (2026-10-28 01:24:05)
Static x86_64 musl xtask (447,160 B, sha256 05d6bfd3d94bde61...) copied to
`/var/tmp/xtask-probe-20261028-012352`, run with `env -i`, then the folder was
removed (0 xtask procs left). The binary only reads /proc and /sys files; no
cargo on the VPS, `--dry-run`, nothing sandboxed ran.
```
-- root:       host linux=yes landlock=no userns=yes cgroup2=yes euid=0 vps_only=no
-- uid 65534:  host linux=yes landlock=no userns=yes cgroup2=yes euid=65534 vps_only=no
   openmind-sandbox supported / hermes-tools lacks landlock / cgroup-detect supported
-- test --dry-run -- --locked
   include openmind-sandbox, include cgroup-detect, exclude hermes-tools (host lacks landlock)
   cargo test --workspace --exclude hermes-tools --locked                rc=0
-- --sandbox=require: hermes-tools needs landlock on this host           rc=3
-- HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1: all three included, cargo test --workspace   rc=0
```
Matches slot 320 (hermes-tools sandboxed.rs plain 2/8 without Landlock, 10/0 with
the override; openmind-sandbox suite green on the VPS). LSM list
`lockdown,capability,yama,apparmor`, kernel 6.8.12-43-pve.
After: hermes-rs MainPID 255539 NRestarts 0, galion-desk 264677 NRestarts 0
(unchanged since slot 320), nerve 127.0.0.1:8670/state 200. Cortex: not probed.

## 3. CI still runs them where supported
`.github/workflows/self-eval.yml` test job now: userns sysctl step (unchanged),
`xtask probe`, `xtask test --sandbox=require -- --locked`.
- Fixture `gh-ubuntu-2404-sysctl` (synthetic ubuntu-24.04 after the sysctl
  step; Landlock on per landlock-lsm/rust-landlock CI running ABI-7 tests on
  ubuntu-24.04): require -> include all three, `cargo test --workspace --locked`.
- Fixture `gh-ubuntu-2404-default` (AppArmor userns restriction still 1):
  require -> exit 3 `hermes-tools needs userns`; auto -> excludes hermes-tools only.
So CI runs the same set as before (`cargo test --workspace --locked`), and
turns red instead of silently skipping if the runner loses a feature.
Not proven on a real runner: no CI run was triggered (no PR, no
workflow_dispatch from here). First real proof = the next self-eval run on a PR
into main / swarm/main-staging (D-335-2).

## 4. Unit/integration tests
- `xtask` lib 9/0, `tests/recipe.rs` 5/0 (+1 GALION_REAL host test): fixtures
  box / vps / gh-default / gh-sysctl, force/require/off, env vs flag, bad args,
  CARGO pointed at a nonexistent path proves no run reaches cargo.
- gaia-core 30/0 (ownership row), gaia commands 4/0.
- fmt ok; clippy --workspace --all-targets -D warnings ok; no-python ok;
  repo-check 2147 files 0 errors PASS (83 warnings pre-existing).
- x86_64 musl 447,160 B static (no PT_INTERP / DT_NEEDED), aarch64 musl
  396,944 B; aarch64-linux-android and armv7-musleabihf `cargo check` ok.
