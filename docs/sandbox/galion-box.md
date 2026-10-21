<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-box — Galion OS sandbox launcher prototype (plan L3)

Written 2026-10-21 06:55 CEST (+02:00), wave-4 slot 144. Prototype only: nothing is installed,
deployed or flashed anywhere. No phone was touched.

## What it is

A Rust binary (~1,500 lines incl. unit tests; ~600-line real test) (`crates/galion-box`, one dependency: `libc`) that runs a command in a
private Linux sandbox **without root**, so it can live beside the phone's normal apps (plan goal
*Coexist*). It is the "namespaces" half of the plan's L3 "proot / chroot" item.

```
caller (host namespaces, supervises + timeout)
  └─ init (pid 1): clone(NEWUSER|NEWNS|NEWPID|NEWNET|NEWUTS|NEWIPC), own id maps,
       │           mounts, pivot_root, reaps
       └─ payload: limits, no caps, no_new_privs, clean env, execve
```

| Layer | What galion-box does | Plan goal |
|---|---|---|
| Identity | One uid/gid mapped: payload is uid 1000 inside, the caller's own uid outside. `--uid 0` refused; launcher refuses to run as root. | Secure |
| Filesystem | pivot_root into `--rootfs`; old root detached. Only the rootfs, a 64 MiB private `/tmp`, a tmpfs `/dev` (null, zero, full, random, urandom) and explicit `--bind-ro` paths exist. `--ro-root` makes `/` read-only. | Secure, Private |
| /proc | Fresh proc for the new pid namespace; if the kernel refuses (LXC on the VPS does), an **empty read-only tmpfs** instead. The host `/proc` is never exposed. | Private |
| Processes | New pid namespace: payload is pid 2, sees no host process. Timeout or caller death kills the whole namespace (PDEATHSIG + pid-1 semantics). | Coexist |
| Network | New, empty net namespace. Default: nothing up (default-deny). `--net lo`: only the sandbox's own loopback. | Secure, Anonymous |
| Privileges | Capability bounding set + ambient cleared, `PR_SET_NO_NEW_PRIVS`, exec as non-zero uid so CapEff = 0. | Secure |
| Limits | `nice` 10 by default (0..19 allowed, never above the host UI), RLIMIT_NOFILE 256, RLIMIT_CORE 0, optional RLIMIT_NPROC / RLIMIT_AS. | Coexist |
| Env | Host env never inherited: PATH, HOME, LANG, GALION_BOX_PROC + explicit `--env`. | Private, Anonymous |
| Fail closed | If namespaces cannot be created (or an emulator ignores the flags: `getpid() != 1`), exit 125 and **never** run the payload unsandboxed. | Secure |

Subcommands: `probe` (read-only report + verdict), `plan` (validate + print steps, creates nothing),
`run`, and payload helpers used by the real tests (`inside-check`, `spawn`, `alloc`, `sleep`, `exit`).

## Findings from building it (real, 2026-10-21 CEST)

1. **qemu-user cannot host a namespace sandbox.** Under `qemu-aarch64` (10.0.13) the guest's
   `clone(CLONE_NEW*)` returns EINVAL from qemu itself, and `unshare(CLONE_NEWUSER)` returns EINVAL
   because qemu's RCU helper thread makes every guest process multi-threaded. So "tested under
   qemu-aarch64" can only cover: unit tests, CLI/policy, probe, refusals and the fail-closed path.
   The namespace behaviour itself was proven on the VPS with the x86_64 build of the same code.
   Real aarch64 namespace proof needs real ARM hardware (the phone in Phase 3, or an ARM VM).
2. **`probe` now detects this** (`threads=2` at start → `verdict=emulated`), and `run` fails closed.
3. **clone() not unshare().** Creating the namespaces with a fork-like `clone()` works from a
   multi-threaded caller (an Android app process is always multi-threaded); unshare(CLONE_NEWUSER)
   does not.
4. **LXC refuses classic read-only remounts.** `MS_REMOUNT|MS_BIND|MS_RDONLY` → EACCES on the VPS;
   `mount_setattr(MOUNT_ATTR_RDONLY)` (Linux 5.12+) works. Launcher uses mount_setattr and falls back
   to the remount on ENOSYS (older Android kernels).
5. **LXC refuses a fresh proc mount** in the user namespace (same as the openmind-sandbox run at
   04:27 CEST, where host `/proc` was then visible). galion-box hides `/proc` behind an empty tmpfs
   instead: `proc_pids=0` inside, so the "seebystander" escape class does not apply.

## Phone reality (why this is only half of L3)

Many Android kernels ship with `CONFIG_USER_NS=n`, and app seccomp policies vary. `galion-box probe`
on the handset (Phase 1 gate) answers this in one command. Options if it says `no-userns` are owner
decisions (below).

## Owner decisions (Slavik) — not decided here

| # | Decision | Options | Evidence / recommendation |
|---|---|---|---|
| D-S1 | Fallback when the phone has no user namespaces | (a) root mode: same code without NEWUSER, using the rooted phone (plan L0) via `su`; (b) ship upstream `proot` (GPL-2.0, exec-only, separate binary — licence policy currently forbids GPL); (c) write a Rust ptrace path-translator (weeks of work, slower at runtime) | Recommend (a): the plan already roots the phone; (b) needs a licence exception. |
| D-S2 | Real aarch64 namespace proof | (a) wait for the phone (Phase 3); (b) rent an ARM64 VPS for an hour; (c) qemu-system-aarch64 full VM (GPL tool, test-only, slow, needs a kernel image) | qemu-user is proven insufficient (finding 1). |
| D-S3 | Seccomp + Landlock layer | add a small seccomp filter (deny ptrace/keyctl/bpf/mount family) and Landlock where the kernel has it | VPS kernel has no Landlock; box has it. Cannot be tested under qemu-user. |
| D-S4 | Plan §11 gate | the plan says no build before §11 answers; this is a throwaway prototype on a local branch | Confirm it may stay as the L3 starting point. |

## Not done / open

- Root mode, seccomp, Landlock, cgroup caps: not implemented (see D-S1, D-S3). RLIMIT_NPROC counts
  per real uid, so it is a coarse fork-bomb cap, not a cgroup.
- No jank/thermal test (needs the phone, Phase 3).
- Not wired into openmind-sandbox or the admin CLI yet.

## Escape suite (slot 164)

See `docs/sandbox/galion-box-escape.md` and `docs/real-tests/galion-os-box-escape.md`.
One Rust test per family (ptrace, mount/pivot_root, userns, bpf/perf, io_uring, keyctl, sockets),
run only on the VPS per sandbox_vps_only.
