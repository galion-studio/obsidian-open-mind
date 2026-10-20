---
tags: [openmind, sandbox, design]
project: galion
status: draft
---
<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Agentic sandbox: CPU-only design (ticket B26)

Written: 2026-10-18T20:15:20+02:00

Goal: run agent-generated shell commands on low-end commercial devices
without a GPU, without root, and without a heavy build. Design only.
Nothing in this document has been enabled anywhere.

## 1 Kernel facts measured here

All probes were read-only, run in this LXC.

| Feature | Probe | Result | Usable |
|---|---|---|---|
| Kernel | `uname -r` | `6.8.12-43-pve` (exit 0) | yes |
| Userns toggle | `cat /proc/sys/kernel/unprivileged_userns_clone` | `1` | yes |
| Userns limit | `cat /proc/sys/user/max_user_namespaces` | `2147483647` | yes |
| Userns test | `unshare --user --map-root-user true` (once) | exit 0, no output | yes |
| LSM list | `cat /sys/kernel/security/lsm` | `lockdown,capability,yama,apparmor` | no landlock |
| Landlock ABI | `cat /sys/kernel/security/landlock/abi_version` | `No such file or directory` (exit 1) | no |
| Seccomp | `grep -E 'Seccomp\|NoNewPrivs' /proc/self/status` | `NoNewPrivs: 0`, `Seccomp: 2`, `Seccomp_filters: 1` | partly (a filter is already inherited; ours would stack) |
| cgroup fs | `stat -fc %T /sys/fs/cgroup` | `cgroup2fs` | read only for us |
| cgroup controllers | `cat /sys/fs/cgroup/cgroup.controllers` | `cpuset cpu io memory hugetlb pids rdma misc` | exist, but need delegation |
| Our cgroup | `cat /proc/self/cgroup` | `0::/system.slice/hermes.service` | not ours to edit |
| Virt | `systemd-detect-virt` | `lxc` | nested limits apply |
| rlimits | `ulimit -a` | nofile 4096, nproc 514144, cpu/as/data unlimited, core 0, stack 8192 KB | yes, settable down |
| CPUs | `nproc` | `8` | n/a |
| Memory | `free -m` | 16384 MB total, 14780 MB available, no swap | n/a |
| Cached crates | `ls ~/.cargo/registry/cache/*/ \| rg -i 'wasmtime\|landlock\|rustix\|seccomp\|libc'` | no match (exit 1) | must fetch later |

Reading the table:
- Landlock is not in the LSM list, so `landlock_create_ruleset` will most
  likely fail with ENOSYS or EOPNOTSUPP here. Not verified by a syscall,
  because the ticket forbids running anything beyond the listed probes.
- User namespaces work for unprivileged users. A stock Debian/Ubuntu
  device may differ: Ubuntu 24.04 restricts them through AppArmor.
- cgroup v2 is mounted, but we run inside `hermes.service`. Creating child
  cgroups would touch production. Not done, not planned for v0.

## 2 Options compared

Cost is for a low-end CPU (2 to 4 slow cores, 2 to 4 GB RAM).

| Mechanism | Protects against | Cost on low-end CPU | Works in this LXC | Works on stock Debian/Ubuntu |
|---|---|---|---|---|
| rlimits (`setrlimit`) | memory (AS), CPU seconds, open files, fork count (NPROC), file size | Zero runtime. Tiny code. | yes | yes, always |
| Cleared env + fixed workdir | secret leaks via env vars, wandering cwd | Zero | yes | yes |
| Wall timeout + kill own child | long runs, hangs | Zero (one wait loop) | yes | yes |
| Landlock | fs reads/writes outside allowed paths; net connect (ABI 4+) | Near zero. A few syscalls at spawn. | no (not in LSM list) | Kernel 5.13+ if enabled. Debian 12 and Ubuntu 22.04+ ship it, but check at run time. |
| seccomp-bpf | dangerous syscalls (mount, ptrace, socket) | Near zero. Filter writing is fiddly. | stacks on existing filter | yes (kernel 3.17+) |
| User namespace | fake root, private view | Low | yes | mostly; blocked by default on Ubuntu 24.04 for unconfined binaries |
| Mount namespace | fs view (pivot_root, bind mounts) | Low, but setup code is long | likely, via userns | same as userns |
| PID namespace | seeing or signalling other processes | Low | likely, via userns | same as userns |
| Net namespace | all network exfiltration (empty netns) | Low | likely, via userns | same as userns |
| cgroups v2 limits | hard memory, CPU share, pid count for a whole tree | Low, but needs a delegated subtree | no (would touch hermes.service tree) | only under systemd delegation or root |
| wasmtime / WASI | everything: memory-safe, no syscalls, caps by preopen | Estimate: cold build 3 to 8 minutes on 8 fast cores, far longer on 2 slow cores; binary adds roughly 10 to 20 MB; RAM use at compile is high. Runtime is fine. | builds, not tested | yes |

Note on wasmtime: figures are public-knowledge estimates, not measured.
Also, WASI cannot run arbitrary shell commands. It only runs code compiled
to wasm. So it fits plugins, not `bash -c`.

## 3 Threat model

Attacker: a model that writes a bad command by mistake or by prompt
injection. Not a determined kernel exploit author.

| Threat | v0 layer that helps | What it stops | What it does not stop |
|---|---|---|---|
| Filesystem escape (read or write outside workdir) | fixed workdir; landlock if present | With landlock: reads and writes outside allowed lists. | Without landlock: nothing. `cat /etc/passwd` or `rm ../x` still work. Workdir is only a start point. |
| Secret reads (files) | landlock fs_read allowlist | Reads of files outside the list. | Without landlock: any file the user can read. |
| Secret reads (env) | env cleared, allowlist only | Env-borne tokens. | Secrets already in the workdir. |
| Network exfiltration | landlock net rules (ABI 4+ only); v1 net namespace | v1: all sockets. | v0 on kernels without landlock net: nothing. `net: false` is then advisory and must be reported as not enforced. |
| Fork bomb | `RLIMIT_NPROC` | Caps processes for the whole user, so also hits our own siblings. | NPROC is per real user, not per tree. Needs a dedicated uid or v1 pid cgroup to be exact. |
| Memory exhaustion | `RLIMIT_AS` | One process address space. | Many processes each under the cap. Mmap tricks that count differently. |
| CPU exhaustion | `RLIMIT_CPU` plus wall timeout | CPU seconds per process; total wall time. | Many short children each under the cap (wall timeout still ends it). |
| Disk fill | `RLIMIT_FSIZE` | Single file size. | Many files. Needs quota or fs_write limits. |
| Long runs | wall timeout, SIGKILL to own child | Hangs. | Children that escaped the process group: use `setsid` and kill the group. |
| Fd exhaustion | `RLIMIT_NOFILE` | Open files per process. | n/a |
| Kernel exploits, ptrace, mount | nothing in v0 | n/a | v1 seccomp reduces surface. |

Important: v0 is a safety net against accidents and sloppy commands. It is
not a boundary against a hostile program. Docs and logs must say so.

## 4 Decision: layered v0

Pick, in order of build effort:
1. v0: rlimits + cleared env + fixed workdir + wall timeout (kill own child
   only) + landlock when the kernel offers it. If landlock is missing, run
   anyway and report `landlock: not_enforced` in the outcome.
2. v1: seccomp-bpf filter, then user + mount + pid + net namespaces where
   the host allows them. Net namespace gives real `net: false`.
3. v2: wasmtime/WASI only for untrusted plugins shipped as wasm. Not for
   shell commands.

Reasons:
- v0 needs only `libc` or `rustix`. It builds in seconds on a weak CPU.
- rlimits, env, timeout work on every Linux device, with no privilege.
- Landlock needs no root and no namespaces, so it works where userns is
  blocked. It is optional, so absence cannot break the build or the run.
- Namespaces and seccomp work here but vary across distros. Their setup
  code is long and error-prone. Defer until v0 is tested.
- cgroups need delegation we do not have. Skip until a device installer
  can supply a delegated subtree.
- wasmtime is the heaviest to build and cannot run shell. Defer.

## 5 What is enabled where

- Here (this LXC, this pod): nothing. No sandbox code exists, nothing is
  wired into any agent, no cgroup was created, no namespace was kept, no
  setting was changed. The single `unshare` probe made an empty namespace
  and exited.
- Devices: nothing. B27 will add a library crate only. Wiring it into any
  tool runner is a separate ticket and needs approval.

## 6 B27 spec and tests

Crate `openmind-sandbox` v0, Rust, MIT headers on every file. Deps: `libc`
or `rustix` only, plus optional `landlock` crate behind a cargo feature
(`landlock`, on by default, failure at run time is non-fatal). No Python.
No tokio. Build with `nice -n 19 cargo build -j 2`.

```rust
pub struct SandboxPolicy {
    pub workdir: PathBuf,
    pub env_allowlist: Vec<String>,   // names copied from parent env
    pub wall_timeout: Duration,
    pub cpu_secs: u64,                // RLIMIT_CPU
    pub mem_bytes: u64,               // RLIMIT_AS
    pub nofile: u64,                  // RLIMIT_NOFILE
    pub net: bool,                    // false = deny; enforced only if supported
    pub fs_read: Vec<PathBuf>,
    pub fs_write: Vec<PathBuf>,
}
pub fn run(cmd: &[String], policy: &SandboxPolicy) -> Outcome;
```

Outcome carries: exit code or signal, `timed_out: bool`, stdout and stderr
(capped, e.g. 1 MiB each), wall time, and an `enforced` report:
`{ rlimits: bool, env: bool, workdir: bool, landlock: Enforced|NotSupported|Failed(reason), net: Enforced|NotEnforced }`.

Behavior:
- Spawn with `Command`, `env_clear()`, then add only allowlisted names.
- `pre_exec` (async-signal-safe calls only): `setsid`, `setrlimit` for CPU,
  AS, NOFILE, plus fixed `RLIMIT_NPROC`-style and `RLIMIT_CORE=0`; `chdir`
  to workdir; `PR_SET_NO_NEW_PRIVS`; then landlock restrict if ABI present.
- Parent polls `try_wait` at 50 ms to 100 ms until `wall_timeout`, then
  `killpg` on the child's own process group, then reaps. Never kill any pid
  not returned by our own spawn. No `pkill`, no name matching.
- stdin is `/dev/null`. Output read on threads or non-blocking, capped.
- If landlock is unavailable or `net` cannot be enforced, run anyway and say
  so in `enforced`. A caller may set `require_enforced` later (v1).

Tests (all run in a temp dir under the scratch area, no network, no root):
1. `echo hi` returns exit 0, stdout `hi`.
2. Env: parent sets `SECRET_TEST=x`; `env` output lacks it; an allowlisted
   var is present.
3. Workdir: `pwd` equals policy workdir.
4. Timeout: `sleep 30` with 1 s wall returns `timed_out`, took under 3 s,
   child gone (check pid with `kill -0`).
5. Own child only: a second unrelated `sleep` started by the test is still
   alive after the timeout kill.
6. CPU: busy loop with `cpu_secs = 1` ends by SIGXCPU/SIGKILL.
7. Memory: allocate more than `mem_bytes` fails (nonzero exit or signal).
8. Nofile: opening more than `nofile` files fails.
9. Group kill: `sh -c 'sleep 30 & wait'` leaves no `sleep` child after timeout.
10. Capped output: a command printing 10 MiB returns at most the cap.
11. Landlock present (skip with a printed reason if not): reading a path
    outside `fs_read` fails with EACCES; writing outside `fs_write` fails.
12. Landlock absent: `run` still works and `enforced.landlock` is
    `NotSupported`. On this LXC this is the expected path.
13. Missing workdir returns an error outcome, no panic.
14. Source scan: no Python files, no `pkill`/`killall`, MIT headers present.

Out of scope for B27: seccomp, namespaces, cgroups, wasmtime, any wiring
into Hermes or other services.
