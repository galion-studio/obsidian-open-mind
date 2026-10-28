<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life sandbox tests — `openmind-sandbox` + `cgroup-detect`

Slot 19 (`swarm/sandbox`). All work on the disposable box
(`grok-bot-vm-303083841`, Linux 6.12.94, cgroup v2, Landlock present). Times
are Europe/Zagreb (CEST +02:00).

## 1. What the review flagged, and what was actually wrong

The review (`review-20261020/REVIEW.md` C-note, `CODE.md` §1) listed three
worries about `openmind-sandbox` / `cgroup-detect`:

1. `h_caps_dropped_when_enforced` — "CapEff read empty when caps report
   enforced: possibly a real sandbox defect".
2. `j_net_namespace_has_only_lo` — flaky, ~1 run in 5 failing with
   `cat: /proc/net/dev: Permission denied`.
3. the cgroup-detect live test needing delegation.

### 1a. The caps "defect" is not a caps bug — it was the flake in disguise

Running a real process inside the sandbox and reading `/proc/self/status`
(2026-10-20, box user) shows capabilities **are** fully dropped when the
`caps` layer reports `Enforced`:

```
rlimits..caps all Enforced
CapEff: 0000000000000000
CapPrm: 0000000000000000
CapInh: 0000000000000000   (the host process has CapInh=000001ffffffffff)
CapAmb: 0000000000000000
CapBnd: 0000000000000000   (userns / root path)
```

The symptom the review saw — `CapEff` "read empty", i.e. the test could not
read `/proc` at all — was the **same root cause as the `j_net` flake**: see
1b. Both the caps test and the net test did `cat /proc/self/*` inside the
sandbox, so when `/proc` was unreadable they hit the same permission error
and the test **skipped**, which masked the real check about 1 run in 5.

The old caps test also only checked `CapEff`/`CapPrm`. It now asserts every
set (`CapEff`, `CapPrm`, `CapInh`, `CapAmb`, and `CapBnd` on the userns/root
path) is zero, and it **no longer skips** on an unreadable `/proc` — that is
now a hard failure, because it is a real bug, not a machine quirk.

### 1b. Root cause of the flake: `RLIMIT_NOFILE` set before the uid/gid map

In the forked pre-exec child, `RLIMIT_NOFILE` (default `nofile = 64`) was set
**before** writing `/proc/self/uid_map` and `/proc/self/gid_map` and before
Landlock opened `/proc`. Those steps each need to `open(2)` a file. When the
sandbox was spawned from a busy, multi-threaded parent (the test runner under
`cargo test`, or a real agent process) that already had many descriptors
open, the fd table inherited at fork could already hold **more than 64**
descriptors, so `open("/proc/self/uid_map")` failed with `EMFILE`. The map
was then never written, the process ran as the **overflow uid (65534)**, and
a process with an unmapped uid cannot read its own `/proc` entries →
`Permission denied`. Whether it happened depended on how many fds were open
at the instant of the fork, which is why it was ~1 in 5 under parallel tests.

Two fixes, both in `crates/openmind-sandbox/src/lib.rs`:

- **`RLIMIT_NOFILE` is now applied last**, after the uid/gid map, the
  namespace setup and the capability drop — after every `open(2)` the setup
  needs. The limit still applies to the executed command.
- **Writing the uid/gid map now fails closed.** If the map cannot be written
  after entering a user namespace, the run returns `Status::Error`
  ("user namespace entered but the uid/gid map could not be written; refusing
  to run as the overflow uid") instead of silently running unmapped. So even
  if some future setup step exhausts fds, you get a clear error, never a
  half-mapped run with an unreadable `/proc`.

Regression test `h2_many_parent_fds_do_not_break_setup` opens 300 extra files
in the parent, sets `nofile = 16`, and asserts `/proc` is readable and the uid
is mapped 1:1 (not 65534). On the pre-fix code this test fails; with the fix
it passes. Verified by reverting `lib.rs` and running only that test:

```
test h2_many_parent_fds_do_not_break_setup ... FAILED   (old lib.rs)
test h_caps_dropped_when_enforced ... FAILED            (old lib.rs)
...
test h2_many_parent_fds_do_not_break_setup ... ok       (fixed lib.rs)
```

Stress run after the fix, 25× the full suite as the box user: **24 passed
every time, zero SKIP, zero flake** (before, the three `/proc` tests skipped
a few times per 25 runs).

### 1c. cgroup-detect delegation

`cgroup-detect` is read-only and correct. The box's own cgroup
(`0::/agent`) has no writable delegated ancestor, so `detect()` returns
`per_run_possible = false` and the live test skips — correct. To prove it
works where delegation exists, a delegated subtree was created as root and the
process moved into it; then:

```
per-run cgroup possible under /swarm-sandbox: subtree already delegates cpu, memory, and pids
test live_cgroup_v2_delegation ... ok
```

Two hardening changes to `cgroup-detect`:

- A candidate parent now also requires a **writable `cgroup.procs`**, not just
  a writable directory: moving a process into a new child needs write access
  to the common-ancestor `cgroup.procs`, so a delegation that only `chown`ed
  the directory is not a real delegation. New unit test
  `chowned_dir_without_writable_procs_is_not_a_delegation`.
- The live test takes `GALION_REAL_CGROUP=1` to mean "this process was started
  inside a delegated subtree"; then a missing delegation is a failure, not a
  skip.

## 2. New real hardening added to the sandbox

- **Leaked file descriptors are closed.** Before exec the child marks every
  descriptor above stderr close-on-exec with `close_range(2)`
  (`CLOSE_RANGE_CLOEXEC`), falling back to an `fcntl` loop. A socket or
  directory fd the host process leaked without `O_CLOEXEC` never reaches the
  command. New `fds` layer; test `h3_leaked_parent_fd_is_not_inherited`
  (fails on the old code — the leaked fd showed up in `/proc/self/fd`).
- **Per-run cgroup v2** (`crates/openmind-sandbox/src/cgroup.rs`): when a
  policy sets `cgroup_parent` (found by `cgroup-detect`), the run creates a
  child cgroup, sets `pids.max`, `memory.max`, `memory.swap.max = 0` and
  `memory.oom.group = 1`, joins it first thing after fork (so the whole
  process tree is inside), and tears it down with `cgroup.kill` at the end —
  which reaps even processes that left the process group with their own
  `setsid()`. `Outcome.cgroup` reports `pids.events`, `memory.events`,
  `memory.peak`, `pids.peak`. New `cgroup` layer.

## 3. Adversarial real-life test

`crates/openmind-sandbox/examples/escape.rs` is a real payload run **inside**
the sandbox; `crates/openmind-sandbox/tests/real_escape.rs` runs it for each
attack and checks containment two ways: the payload's own verdict **and** the
host side (the listener got no bytes, the bystander is untouched, the outside
file does not exist, no stray sleeper survived). Harness:
`scripts/real/sandbox.sh` (builds release, makes a setuid-root helper and a
delegated cgroup as root, then runs the suite as an **unprivileged** user).

Run 2026-10-20 ~19:1x CEST, as user `box` (uid 1000), inside a delegated
cgroup v2 subtree:

```
== run as box (uid 1000) inside /sys/fs/cgroup/om-sandbox-real-<pid>/runner ==
RESULT tcp           CONTAINED  Permission denied (os error 13)      # empty netns, no localhost
RESULT tcp-internet  CONTAINED  Permission denied (os error 13)      # no route out
RESULT udp           CONTAINED  Network is unreachable (os error 101)
RESULT ptrace        CONTAINED  No such process                      # bystander invisible in pidns
RESULT kill          CONTAINED  No such process
RESULT seebystander  CONTAINED  bystander not visible in /proc       # private /proc in pidns
RESULT mount         CONTAINED  mount proc: Operation not permitted
RESULT remount-root  CONTAINED  remount /: Operation not permitted
RESULT setuid        CONTAINED  /tmp/.../suid-id -> uid 1000 (setuid ignored under no_new_privs)
RESULT read          CONTAINED  hidden file: No such file or directory  # hide_home tmpfs
RESULT write         CONTAINED  Read-only file system                # ro_root
RESULT forkbomb      CONTAINED  stopped after 22 forks: Resource temporarily unavailable  # pids.max=24
(membomb)            Signaled(9)   # cgroup memory.max + oom.group -> whole group SIGKILLed
RESULT setsid-orphan CONTAINED  reaped by cgroup.kill at teardown
test result: ok. 1 passed; 0 failed
```

- **setuid**: the sandboxed process is unprivileged and runs a real
  setuid-**root** copy of `/bin/id`; `PR_SET_NO_NEW_PRIVS` means it stays uid
  1000 — the setuid bit is ignored. (When the suite is run as root the
  sandboxed process is root too, so this sub-check is skipped as meaningless,
  with a note; the no-new-privs + empty-caps proof is then
  `h_caps_dropped_when_enforced`.)
- **memory bomb**: with a per-run cgroup, `memory.max = 128 MiB` +
  `memory.oom.group = 1` SIGKILLs the whole group (`Signaled(9)`); without a
  cgroup, a single-process bomb still hits `RLIMIT_AS` (`Signaled(11)`).
- **fork bomb**: `pids.max = 24` stops it after ~22 forks with `EAGAIN`; the
  cgroup is empty and removed afterwards (asserted via `Outcome.cgroup`).

How to reproduce:

```
# unit + hardening + regression tests (as the box user)
CARGO_TARGET_DIR=/workspace/swarm/target/sandbox \
  cargo test -p openmind-sandbox -p cgroup-detect

# the full adversarial suite (sets up a setuid helper + delegated cgroup)
scripts/real/sandbox.sh
```

## 4. VPS — READ-ONLY probe, and what still needs a privileged host test

Probed `slavik-vps` read-only on 2026-10-20 ~19:1x CEST (`uname`, `cat` of
cgroup/LSM files only; nothing changed, no service touched):

```
uname -a                : Linux ip50 6.8.12-43-pve ... x86_64 (Proxmox PVE kernel)
/sys/fs/cgroup/cgroup.controllers      : cpuset cpu io memory hugetlb pids rdma misc
/sys/fs/cgroup/cgroup.subtree_control  : cpu memory pids
/proc/self/cgroup                      : 0::/user.slice/user-0.slice/session-63846.scope
/sys/kernel/security/lsm               : lockdown,capability,yama,apparmor
kernel.unprivileged_userns_clone       : 1
/proc/sys/user/max_user_namespaces     : 2147483647

(box, for comparison: Linux 6.12.94; LSM capability,landlock,selinux,bpf)
```

Differences that mean the box result does **not** fully stand in for the VPS,
so these need a privileged test **on the VPS host itself** (not done here —
rule 3/4: no builds, no changes, read-only only):

1. **No Landlock on the VPS kernel** (6.8 PVE LSM list has no `landlock`; the
   box has it). On the VPS the sandbox's `landlock` layer will report
   "kernel has no landlock", and the Landlock TCP-deny fallback for the
   `net = true` case is unavailable. Network isolation there relies entirely
   on the **network namespace** (which needs working unprivileged userns).
   Needs: build the crate on the box, copy the binary to a VPS **test** host
   (never `/opt/galion/openmind`), run `features()` and the `net`/`j_net`
   checks, confirm `landlock_abi() == None` and that `net` is still
   `Enforced` via netns.
2. **AppArmor is active on the VPS** (`apparmor` in the LSM list). On recent
   Debian/Ubuntu kernels AppArmor can restrict unprivileged user namespaces
   (`kernel.apparmor_restrict_unprivileged_userns`). `unprivileged_userns_clone
   = 1` is encouraging, but whether an **unconfined, non-root** process can
   actually `unshare(CLONE_NEWUSER|NEWNS|NEWNET|NEWPID)` under the VPS's
   AppArmor profile must be checked on the host. If it cannot, `userns`,
   `netns`, `pidns` and `mountns` all fall back to "not enforced" and the
   sandbox degrades to rlimits + no_new_privs + (no) Landlock.
3. **cgroup delegation**: the VPS root subtree already delegates
   `cpu memory pids`, but the live session lives under
   `/user.slice/user-0.slice/session-*.scope`. Whether a per-run child cgroup
   can be created depends on a **writable delegated ancestor** for the user
   the sandbox runs as (systemd user-slice delegation, or a dedicated
   `.slice`/`.scope` with `Delegate=yes`). `cgroup-detect::detect()` run as
   that user on the VPS will say yes/no without creating anything; run it
   there to confirm before relying on `pids.max`/`memory.max`.
4. **yama** `ptrace_scope` is present on the VPS (absent on the box). It only
   makes ptrace containment stronger, but the ptrace sub-check should be
   re-run on the VPS to record the exact behaviour.

Suggested read-only confirmation on the VPS (no build, no state change):
`cargo`-built `cgroup-detect` binary run as the target user, plus the existing
`j_net`/`h_caps` behaviour from a box-built static musl binary copied to a
VPS *test* area. None of this was performed, per the read-only rule.

> **2026-10-21 (realscript-port-3):** the shell driver for this evidence was ported to Rust and deleted; re-run with the matching test in `crates/galion-realtest/tests/` (see `docs/real-tests/realscript-port-3.md`).
