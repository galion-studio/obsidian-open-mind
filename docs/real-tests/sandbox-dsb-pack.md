<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: staged hermes-tools preset (slot 305, sandbox-dsb-pack)

Test: `crates/hermes-tools/tests/staged_preset.rs`. It builds `exec::policy` from the exact
`Environment=` lines of `deploy/staged/hermes-rs.service.d/coding-tools.conf`. Only the worktree,
state and toolchain paths are remapped to a temp dir.
- 6 policy tests start no sandbox and run anywhere.
- 2 `real_staged_*` tests start openmind-sandbox. They need `GALION_REAL=1 GALION_SANDBOX_ON_VPS=1`; without them they print SKIP and pass vacuously (sandbox_vps_only).

## Box (2026-10-27 ~21:04 CET +01:00, policy layer only)
`cargo-slot test -j 2 -p hermes-tools --test staged_preset -- --nocapture`: **8 passed, 0 failed**. The 2 real tests were vacuous (SKIP). No sandbox was started on the box.

## VPS (2026-10-27 21:05:20–21:05:59 CET +01:00)
- Binary: a static x86_64-musl release test binary built on the box and copied to `/var/tmp/sandbox-test-20261027-210518`, together with the 2 staged deploy files.
- How it ran: as uid/gid 65534 (`setpriv --reuid=65534 --regid=65534 --clear-groups`, `env -i`). No temp user was created and no cgroup was used.
- Baseline before and after: hermes-rs MainPID 255539 / NRestarts 0, and galion-desk 264677 / 0, both unchanged. Nerve `/state` returned 200 both times.

Run 1 at 21:05:43 (full output: `scratch/dsb305-vps-run1.txt`): **8 passed, 0 failed, 0.72 s**

| Probe | Fs | Git | Cargo |
|---|---|---|---|
| status | Exited(0) | Exited(0) | Exited(0) |
| proc_view | Masked{6,0} | Masked{6,0} | Masked{6,0} |
| real link.env (`[ -r ]` only) | HIDDEN | HIDDEN | HIDDEN |
| entries in /opt/galion | 0 | 0 | 0 |
| planted 0666 canary, hidden | HIDDEN, content not in stdout | same | same |
| /proc pid dirs, /proc/self, /proc/1/cmdline | 0, no, no | 0, no, no | 0, no, no |
| /proc files | cpuinfo, loadavg, meminfo, stat, uptime, version | same | same |

- **Control:** the same canary with no hide entry → `CANARY=READABLE`. So the probe is sensitive, and HIDDEN really comes from the hide layer.
- **Layers:** rlimits, env, workdir, workdir_quota, no_new_privs, userns, net, pidns, mountns, ro_root, hide_home, proc, caps and fds were Enforced. landlock was NotEnforced ("kernel has no landlock"). cgroup was NotEnforced (no cgroup_parent; a plain user has no delegate).
- **Host fact:** `link.env` mode 666 (metadata only).
- **Workdir quota** (Fs kind, `HERMES_RS_CODING_WORKDIR_MB=16`, the same mechanism as the staged 512 MiB):
  - `WROTE=64` × 256 KiB = 16 MiB, then ENOSPC. `df` showed 16384/16384 kB, 100%.
  - Quota report: limit_bytes 16777216, limit_inodes 1024, used 16777216, copied_out and out_complete true, errno 0.
  - The host worktree held 16777216 bytes after copy back, so the cap held on the host too.

Runs 2 and 3 (21:05:57 and 21:05:58): **8 passed, 0 failed** each (0.71 s and 0.49 s).

Run 4 at 21:08:30 used the final test source (the MemoryMax check now parses the drop-in, for clippy): **8 passed, 0 failed, 0.67 s**. Cleanup: folder removed, 0 mounts, 0 processes, and hermes-rs MainPID 255539 / NRestarts 0 unchanged.

Cleanup at 21:05:59: 0 leftover processes, 0 `sandbox-test-` mounts, folder removed.

## Not tested (honest gaps)
- euid 0 (D-SB4). Not run on the live host on purpose.
- The delegated cgroup path (`HERMES_RS_CODING_CGROUP=delegate`).
- `subset=pid` (needs kernel ≥ 7.2). Nesting (D-SB2).
