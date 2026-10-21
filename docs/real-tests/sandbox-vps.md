<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Sandbox real test on the VPS (2026-10-21)

Standing rule (Slavik via Mate, 2026-10-21 04:29 +02:00): the sandbox runs only on the VPS. This record is the VPS run. All times are Europe/Zagreb (+02:00). The host is called "the VPS" here, with no address.

## Setup

- Source: `swarm/realscript-port-3` 03967fc. The binaries are static x86_64-unknown-linux-musl, built on the box and never run there:
  - `escape`, the `openmind-sandbox` example (adversarial payload), sha256 `3987d86e…cc39`
  - `sandbox-vps-runner`, sha256 `01640d00…d920`. It is a small Rust runner that does what `crates/openmind-sandbox/tests/real_escape.rs` does: the same attacks, judged two ways (the payload's `RESULT` line and a host-side check). It takes its paths from argv, so it runs from a throwaway dir. It also has `features` (prints `openmind_sandbox::features()` and `landlock_abi()`) and `detect` (prints `cgroup_detect::detect()`). The source stayed outside the repo, at box scratch `/workspace/swarm/scratch/sandbox-vps-runner`, with path dependencies on 03967fc.
- Test dir: `/var/tmp/sandbox-test-20261021-042757` (0755 root; `work/` owned by the test user). `/opt/galion/openmind` was not touched.
- User: a temporary system user `sbxtest` (uid 996, nologin, no home), created 04:27:57 and removed 04:29:11.
- Host: an x86_64 Proxmox LXC container, kernel 6.8.12-pve, cgroup v2, glibc 2.39.
- Cleanup at 04:29:11 +02:00: removed the dir, `userdel sbxtest` (its group went with it), and stopped `sbxtest.slice`. Afterwards: no sbxtest processes, no `sbx*` cgroups, no `/var/tmp/sandbox-test-*`, and no stray `sleep 30` or `sleep 3133`.

## Kernel knobs (04:28:17)

```
kernel.apparmor_restrict_unprivileged_userns = 0
kernel.yama.ptrace_scope = 1
kernel.unprivileged_userns_clone = 1
user.max_user_namespaces = 2147483647
```

As `sbxtest`, `unshare --map-root-user` worked for each of: `-U`, `-U -m`, `-U -n`, `-U -p -f`, `-U -m -n -p -f`.

## (1)+(2) features() as the non-root user

```
uid=996 euid=996
FEATURES Features { landlock_abi: 0, userns: true, userns_full: true, mount_setattr: true, caps: true, android: false }
LANDLOCK_ABI None
```

Root gets the same result. **Landlock is not available** (`landlock_abi() == None`). The network layer is still **Enforced**, through the empty network namespace.

## (1)(2)(4) Adversarial suite as `sbxtest`, no delegated cgroup (04:28:25)

Every run reported the same layer set:

```
LAYERS rlimits=Enforced | env=Enforced | workdir=Enforced | no_new_privs=Enforced | userns=Enforced | net=Enforced | pidns=Enforced | mountns=Enforced | ro_root=Enforced | hide_home=Enforced | proc=NotEnforced("proc mount refused (e.g. container with overmounted /proc); host /proc stays visible") | landlock=NotEnforced("kernel has no landlock") | caps=Enforced | fds=Enforced | cgroup=NotEnforced("no cgroup_parent in policy; no pids.max / memory.max for the tree")
```

RESULT lines (payload) and HOST lines (host-side checks), exactly as printed:

```
uid=996 euid=996
RUN ["tcp", "38591"] status=Exited(0) timed_out=false error=None
RESULT tcp CONTAINED Network unreachable (os error 101)
RESULT tcp-internet CONTAINED Network unreachable (os error 101)
HOST OK net layer Enforced
HOST OK tcp to host loopback listener contained
HOST OK tcp-internet contained
HOST OK host TCP listener received 0 bytes
RUN ["udp", "43357"] status=Exited(0) timed_out=false error=None
RESULT udp CONTAINED Network unreachable (os error 101)
HOST OK udp contained
HOST OK host UDP socket received 0 datagrams
RUN ["ptrace", "4000913"] status=Exited(0) timed_out=false error=None
RESULT ptrace CONTAINED pid 4000913: No such process (os error 3)
HOST OK ptrace of outside bystander contained
RUN ["kill", "4000913"] status=Exited(0) timed_out=false error=None
RESULT kill CONTAINED pid 4000913: No such process (os error 3)
HOST OK kill of outside bystander contained
RUN ["seebystander", "sleep 30"] status=Exited(0) timed_out=false error=None
RESULT seebystander ESCAPED 4000913 -> /bin/sleep 30
HOST FAIL bystander invisible in /proc
HOST OK bystander still alive
RUN ["mount"] status=Exited(0) timed_out=false error=None
RESULT mount CONTAINED mount proc: Operation not permitted (os error 1)
RESULT remount-root CONTAINED remount /: Permission denied (os error 13)
HOST OK mount contained
HOST OK remount-root contained
RUN ["read", "/var/tmp/sandbox-test-20261021-042757/work/a/secretdir/key"] status=Exited(0) timed_out=false error=None
RESULT read CONTAINED /var/tmp/sandbox-test-20261021-042757/work/a/secretdir/key: No such file or directory (os error 2)
HOST OK hidden file read contained
HOST OK hidden marker not leaked
RUN ["write", "/var/tmp/sandbox-test-20261021-042757/work/a/escaped-marker"] status=Exited(0) timed_out=false error=None
RESULT write CONTAINED /var/tmp/sandbox-test-20261021-042757/work/a/escaped-marker: Read-only file system (os error 30)
HOST OK write outside workdir contained
HOST OK no file created outside workdir
RUN ["membomb", "1024"] status=Signaled(11) timed_out=false error=None
HOST OK membomb contained by RLIMIT_AS
NOTE cgroup checks skipped: GALION_REAL_CGROUP_PARENT not a delegated dir
SUMMARY 1 failures: bystander invisible in /proc
exit=1
```

## (3) cgroup-detect, then a delegated run

The first detect ran as `sbxtest` via `runuser` from the root SSH session, inside `/user.slice/user-0.slice/session-*.scope`:

```
per_run_possible: false,
reason: "per-run cgroup not possible: no writable domain ancestor outside system.slice delegates cpu, memory, and pids",
```

Next: `systemd-run --wait --pipe --collect -p User=sbxtest -p Delegate=yes` as a transient service in system.slice. The result was the same `per_run_possible: false`, because detect refuses system.slice on purpose.

The run that worked was a transient service in its own slice: `systemd-run --wait --pipe --collect --slice=sbxtest.slice -p User=sbxtest -p Delegate=yes wrap.sh`. The wrapper (as sbxtest) moved itself into the leaf `sup/` and wrote `+memory +pids +cpu` to the unit cgroup's `cgroup.subtree_control`. Then it ran detect and the full suite with `GALION_REAL_CGROUP_PARENT` set to the unit cgroup (04:28:58, yama ptrace_scope=1):

```
CG=/sys/fs/cgroup/sbxtest.slice/sbxtest-escape-20261021-042757.service subtree_control=cpu memory pids
    candidate_parent: Some(
    per_run_possible: true,
    reason: "per-run cgroup possible under /sbxtest.slice/sbxtest-escape-20261021-042757.service: subtree already delegates cpu, memory, and pids",
        "/sbxtest.slice/sbxtest-escape-20261021-042757.service",
uid=996
uid=996 euid=996
RUN ["tcp", "42037"] status=Exited(0) timed_out=false error=None
RESULT tcp CONTAINED Network unreachable (os error 101)
RESULT tcp-internet CONTAINED Network unreachable (os error 101)
HOST OK net layer Enforced
HOST OK tcp to host loopback listener contained
HOST OK tcp-internet contained
HOST OK host TCP listener received 0 bytes
RUN ["udp", "53356"] status=Exited(0) timed_out=false error=None
RESULT udp CONTAINED Network unreachable (os error 101)
HOST OK udp contained
HOST OK host UDP socket received 0 datagrams
RUN ["ptrace", "4001439"] status=Exited(0) timed_out=false error=None
RESULT ptrace CONTAINED pid 4001439: No such process (os error 3)
HOST OK ptrace of outside bystander contained
RUN ["kill", "4001439"] status=Exited(0) timed_out=false error=None
RESULT kill CONTAINED pid 4001439: No such process (os error 3)
HOST OK kill of outside bystander contained
RUN ["seebystander", "sleep 30"] status=Exited(0) timed_out=false error=None
RESULT seebystander ESCAPED 4001439 -> /bin/sleep 30
HOST FAIL bystander invisible in /proc
HOST OK bystander still alive
RUN ["mount"] status=Exited(0) timed_out=false error=None
RESULT mount CONTAINED mount proc: Operation not permitted (os error 1)
RESULT remount-root CONTAINED remount /: Permission denied (os error 13)
HOST OK mount contained
HOST OK remount-root contained
RUN ["read", "/var/tmp/sandbox-test-20261021-042757/work/b/secretdir/key"] status=Exited(0) timed_out=false error=None
RESULT read CONTAINED /var/tmp/sandbox-test-20261021-042757/work/b/secretdir/key: No such file or directory (os error 2)
HOST OK hidden file read contained
HOST OK hidden marker not leaked
RUN ["write", "/var/tmp/sandbox-test-20261021-042757/work/b/escaped-marker"] status=Exited(0) timed_out=false error=None
RESULT write CONTAINED /var/tmp/sandbox-test-20261021-042757/work/b/escaped-marker: Read-only file system (os error 30)
HOST OK write outside workdir contained
HOST OK no file created outside workdir
NOTE cgroup parent /sys/fs/cgroup/sbxtest.slice/sbxtest-escape-20261021-042757.service
RUN ["forkbomb"] status=Exited(0) timed_out=false error=None
RESULT forkbomb CONTAINED stopped after 22 forks: Resource temporarily unavailable (os error 11)
CGROUP CgroupStats { path: "/sys/fs/cgroup/sbxtest.slice/sbxtest-escape-20261021-042757.service/om-sbx-4001415-0", pids_max_hits: 1, oom_kills: 0, mem_max_hits: 0, mem_peak: 3592192, pids_peak: 24, removed: true }
HOST OK cgroup stats present for forkbomb
HOST OK forkbomb capped by pids.max
HOST OK per-run cgroup removed after forkbomb
RUN ["membomb", "1024"] status=Signaled(9) timed_out=false error=None
CGROUP CgroupStats { path: "/sys/fs/cgroup/sbxtest.slice/sbxtest-escape-20261021-042757.service/om-sbx-4001415-1", pids_max_hits: 0, oom_kills: 3, mem_max_hits: 28, mem_peak: 134217728, pids_peak: 2, removed: true }
HOST OK membomb contained by memory.max
RUN ["setsid-orphan", "3133"] status=Exited(0) timed_out=false error=None
RESULT setsid-orphan CONTAINED spawned detached sleeper (pid in ns 2)
CGROUP CgroupStats { path: "/sys/fs/cgroup/sbxtest.slice/sbxtest-escape-20261021-042757.service/om-sbx-4001415-2", pids_max_hits: 0, oom_kills: 0, mem_max_hits: 0, mem_peak: 675840, pids_peak: 3, removed: true }
HOST OK setsid orphan killed at teardown
SUMMARY 1 failures: bystander invisible in /proc
exit=1
unit_exit=0
```

## Verdict

| layer / check | VPS result |
|---|---|
| net (netns): tcp, tcp-internet, udp | **holds**. All CONTAINED (ENETUNREACH). The host TCP listener got 0 bytes and the host UDP socket got 0 datagrams |
| userns / pidns / mountns / ro_root / hide_home / caps / no_new_privs / fds / rlimits | Enforced |
| ptrace, kill of an outside pid (yama ptrace_scope=1) | CONTAINED (ESRCH: the host pid does not exist in the sandbox pidns) |
| mount, remount-root, read hidden file, write outside workdir | CONTAINED |
| **proc** | **NotEnforced**: "proc mount refused (e.g. container with overmounted /proc); host /proc stays visible". **seebystander ESCAPED**: the payload saw `/bin/sleep 30` outside the sandbox |
| landlock | NotEnforced: the kernel has no Landlock (LXC guest) |
| cgroup per-run (pids.max / memory.max) | Only possible in a Delegate=yes unit in its own slice, with the supervisor moved to a leaf. Then: forkbomb capped at 24 (pids.max), membomb OOM-killed at 128 MiB (memory.max, 3 OOM kills), setsid orphan killed, per-run cgroups removed. From an SSH session, or a plain system.slice unit: not possible (RLIMIT_AS still stopped a single-process membomb, SIGSEGV) |
| setuid-root helper | not run (it would need a setuid-root file on production; skipped) |

**Process isolation does not fully hold on the VPS.** Network isolation holds, and ptrace/kill hold. But the sandboxed process can read the host `/proc`: the process list, command lines, and the `/proc/<pid>/*` files that its uid may read. Nothing that runs untrusted code in this sandbox on the VPS (for example Hermes tools execution) should be deployed until one of these is in place: (a) the LXC config allows a fresh proc mount in the child userns/pidns (Proxmox `features: nesting=1` or similar, a host-side decision), (b) the sandbox refuses to run when the `proc` layer is NotEnforced (`require_*`-style policy), or (c) tools run as a dedicated uid that has nothing to read in other processes' `/proc` (hidepid=2 on the container's /proc, also a host decision).
