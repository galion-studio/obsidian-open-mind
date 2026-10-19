<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# cgroup v2 delegation (detect only)

- DESK-ID: openmind.B29
- Measured: 2026-10-19T16:18:27+02:00 (Europe/Zagreb, TZ=Europe/Zagreb date -Iseconds)
- Host: ip50
- Kernel: 6.8.12-43-pve
- Container: lxc (pid 1 environ has container=lxc)
- Repo: /opt/galion/openmind
- Crate: crates/cgroup-detect
- Read only. No cgroup was created. No file under /sys/fs/cgroup was written. crates/openmind-sandbox was not edited.

## Verdict

Per-run cgroups are possible.

The calling process is in `/user.slice/user-0.slice/session-54592.scope`. That scope already has controllers `cpu memory pids` (it can limit itself) but its `cgroup.subtree_control` is empty, so a child would not receive them. The scope is populated (5 processes, cgroup.events populated 1). This ticket does not write `cgroup.subtree_control`. The kernel no-internal-process rule would reject enabling domain controllers in a cgroup that still has processes. A child of the session scope is therefore not a usable per-run limit cgroup.

The nearest ancestor that already delegates `cpu`, `memory`, and `pids` is `/user.slice/user-0.slice`. Its `cgroup.subtree_control` is `cpu memory pids`, its type is domain, it has no processes of its own, `cgroup.max.depth` and `cgroup.max.descendants` are `max`, and the directory is writable by uid 0. A child created there would show those three controllers in `cgroup.controllers`, the same way this session scope already does. The detector does not create that child.

`system.slice` is also writable and already delegates the same three controllers. It is not a candidate. Nothing under `system.slice` is a candidate, including a service that already has `cgroup.subtree_control` set. Production units stay out of this probe.

## What was read

Filesystem: `stat -fc %T /sys/fs/cgroup` is `cgroup2fs`. Mount (from /proc/self/mountinfo): `/sys/fs/cgroup` type cgroup2, options `rw,nosuid,nodev,noexec,relatime`. No cgroup v1 mount. /proc/1/cgroup is `0::/init.scope`. /proc/self/cgroup is `0::/user.slice/user-0.slice/session-54592.scope`. Uid line is `Uid:	0	0	0	0` (fsuid 0).

| cgroup | cgroup.controllers | cgroup.subtree_control | type | procs | role |
| --- | --- | --- | --- | --- | --- |
| /user.slice/user-0.slice/session-54592.scope | cpu memory pids | (empty) | domain | 5 | self; not a per-run parent |
| /user.slice/user-0.slice | cpu memory pids | cpu memory pids | domain | 0 | nearest per-run parent |
| /user.slice | cpu memory pids | cpu memory pids | domain | 0 | also delegated; farther |
| / | cpuset cpu io memory hugetlb pids rdma misc | cpu memory pids | domain | 0 | container root; also delegated; farther |
| /system.slice | cpu memory pids | cpu memory pids | domain | 0 | refused by policy |
| /system.slice/hermes-rs.service | cpu memory pids | (empty) | domain | 1 | Delegate=no; populated |
| /system.slice/galion-one.service | cpu memory pids | (empty) | domain | 1 | Delegate=no; populated |
| /user.slice/user-0.slice/user@0.service | cpu memory pids | cpu memory pids | domain | 0 | Delegate=yes; not an ancestor of this shell |

`systemctl show -p Delegate -p ControlGroup` (read only, no daemon-reload): hermes.service Delegate=no and ControlGroup empty (no /sys/fs/cgroup/system.slice/hermes.service directory); hermes-rs.service Delegate=no; galion-one.service Delegate=no; user@0.service Delegate=yes. `cgroup.events` on the session scope is `populated 1` and `frozen 0`. Limits on the session itself are unset: memory.max `max`, cpu.max `max 100000`, pids.max `max`. Writing those would cap the whole session, not one run. They were not written.

Root `cgroup.stat` at this read: `nr_descendants 45 nr_dying_descendants 496 `. That counter was only read.

## How the crate decides

`cgroup_detect::detect` reads `/proc/self/status`, `/proc/self/cgroup`, `/proc/self/mountinfo`, and the cgroup files for this process and its ancestors. It returns per-run possible only when all of the following hold:

- The hierarchy is unified cgroup v2 (one `0::` line, a cgroup2 mount, no cgroup v1 mount).
- The cgroup2 mount is writable.
- Some ancestor (the process cgroup itself, or a parent, walking toward `/`) has type `domain`, is not frozen, has directory write permission for this fsuid (uid 0 counts), has room under `cgroup.max.depth` and `cgroup.max.descendants`, and already lists `cpu`, `memory`, and `pids` in `cgroup.subtree_control`.
- That ancestor's path is not `/system.slice` and does not start with `/system.slice/`.

The nearest such ancestor wins. On this measurement the sentence is: per-run cgroup possible under /user.slice/user-0.slice: subtree already delegates cpu, memory, and pids.

Possible means the kernel already delegated controllers to that parent. It does not mean a cgroup was created, and it does not mean a later ticket should mkdir inside a login session. A dedicated unit with Delegate=yes is the right place for real per-run limits. This ticket does not add that unit.

## Test

`crates/cgroup-detect/tests/detect.rs` only reads `/proc` and `/sys`. It checks the live report against those files and checks that the candidate is not under `system.slice`. Unit tests in the library cover the policy with in-memory snapshots (session scope rejected, system.slice rejected, root used only when it is outside system.slice and already delegates). They do not create directories.

Checked with `cargo fmt -p cgroup-detect`, `nice -n 19 cargo clippy -j 2 -p cgroup-detect --all-targets -- -D warnings`, and `nice -n 19 cargo test -j 2 -p cgroup-detect`. `CARGO_TARGET_DIR=/tmp/om-b29`.

## Not done

- No mkdir under /sys/fs/cgroup, including not under system.slice.
- No write to cgroup.subtree_control, cgroup.procs, memory.max, cpu.max, or pids.max.
- No edit to crates/openmind-sandbox.
- No systemd enable, start, restart, or daemon-reload.
- No Python.
