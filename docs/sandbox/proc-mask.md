<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind-sandbox: the host /proc gap on LXC and the /proc mask

Wave-4 slot 191 (`swarm/sandbox-seebystander`, base `sandbox-quota-hide-merge`
78c5c49). Times are Europe/Zagreb, CEST +02:00.

## The gap
`real_escape` reported `seebystander ESCAPED` on the VPS: a sandboxed command
listed every process of the container in `/proc`. The sandbox runs the command
as PID 1 of its own PID namespace, but the fresh `proc` mount for that
namespace failed, so the command kept the host `/proc`.

What leaks through the host `/proc` (VPS, measured 14:16, `srp` S10 baseline
and `real_escape` procpeek): 40 pid dirs, 37 other-uid cmdlines, `status`,
`stat` and the `fd` list of every process, services identifiable by name
(cloudflared, galion-nerve-sh, openmind-memory, sshd, systemd, tuner).
`environ` and `maps` of other processes were NOT readable (a same-uid
`environ` canary was not readable either, r2 control). Landlock, which would
have scoped ptrace-style access, is not in the VPS kernel's LSM list
(`lockdown,capability,yama,apparmor`).

## Root cause (proved on the VPS, 13:51-13:53, temp user sbx191)
1. **Kernel**: in a user namespace a new proc mount is refused unless a
   fully visible proc mount already exists (`mount_too_revealing` /
   `mnt_already_visible`). The VPS is a Proxmox LXC (kernel 6.8.12-43-pve)
   whose `/proc` has 8 lxcfs FUSE files mounted over it (cpuinfo, diskstats,
   loadavg, meminfo, slabinfo, stat, swaps, uptime) plus `/proc/sys`,
   `/proc/sysrq-trigger` and `boot_id` overmounts, and there is no clean
   copy at `/dev/.lxc/proc` (Proxmox adds one only with `features: nesting=1`).
   `unshare -Urpfm --mount-proc` -> `mount /proc failed: Permission denied`.
2. `subset=pid` does not help on 6.8: the relaxation for `subset=pid` was
   merged for Linux 7.2 (vfs-7.2-rc1.procfs, "proc: subset=pid: Relax check
   of mount visibility"). Measured: refused on the VPS.
3. **AppArmor** (LXC profile): any mount *on `/proc` itself* is refused, even
   an empty tmpfs or a bind (measured), while mounts on `/proc/1`,
   `/proc/sys`, other paths and a `proc` directory of a copy of the tree are
   allowed.

## The fix (implemented, `crates/openmind-sandbox`)
New `SandboxPolicy::proc_fallback: ProcFallback` and `Outcome::proc_view:
ProcView`. Order in PID 1 of the sandbox (mount + PID namespace):
1. fresh `proc` (unchanged) -> `ProcView::Private`;
2. fresh `proc` with `subset=pid` (Linux >= 7.2 under overmounts) ->
   `ProcView::PidOnly`;
3. `ProcFallback::Mask` (default, also in every `Preset`): a scratch tmpfs
   over the work dir, a recursive bind of `/` into it, the scratch tmpfs
   dropped from the copy, a read-only tmpfs over the copy's `proc` directory,
   `pivot_root(".", ".")` into the copy and the old root detached. Every
   earlier layer (read-only root, hidden paths, work dir bind and work dir
   tmpfs quota) is part of the copy and keeps working; Landlock gets its
   `/proc` rule for the new tmpfs. The mask holds 6 static files: a live clone
   of the host file where allowed, else a snapshot the caller read just before
   the run (VPS: AppArmor refuses the clones, so 6 snapshots). No pid dirs, no
   `/proc/self`. -> `ProcView::Masked { static_files, live_files }`. If a step
   fails before `pivot_root`, every step is undone and the run keeps the host
   /proc with the errno in the `proc` layer; if the old root cannot be
   detached after `pivot_root`, the run fails closed.
4. `ProcFallback::Refuse`: fail the run closed instead of running with the
   host /proc. `ProcFallback::Host`: the old behaviour.

The `proc` layer is `Enforced` for Private, PidOnly and Masked. No new
dependencies (libc only; `open_tree`/`move_mount`/`pivot_root` raw syscalls).

## Trade-off (owner decision D-SB1)
Under the mask a tool cannot read `/proc/self/*` (status, fd, exe,
mountinfo), `/proc/net/*` or `/proc/<pid>`. Seen in our own tests: util-linux
`umount` needs `/proc/self/mountinfo`, the S7 payload counted interfaces from
`/proc/net/dev`, five sandbox tests observe the command through
`/proc/self/status|fd`. Those tests now set `ProcFallback::Host` explicitly
(they test other layers), the S7 payload falls back to `if_indextoname`.
hermes-tools sandboxed 10/10 and srp S3-S7 pass unchanged under the mask.
Rust tools that call `std::env::current_exe()` get an error (cargo falls back
to argv[0]; a cargo build was not run on the VPS: no builds on the VPS).

## Evidence
See `docs/real-tests/sandbox-seebystander.md`.
