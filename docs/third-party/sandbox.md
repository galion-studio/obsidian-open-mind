<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party code and dependencies — `sandbox` slot (swarm/sandbox)

Scope: `crates/openmind-sandbox` and `crates/cgroup-detect`.

## New dependencies

**None.** The sandbox hardening, the per-run cgroup support, the adversarial
`escape` example and the `real_escape` suite are all written against the
standard library plus the `libc` crate that `openmind-sandbox` already
depended on (`libc = "0.2"`, MIT OR Apache-2.0). `cgroup-detect` keeps using
only `std`. No crate was added to either `Cargo.toml`; `Cargo.lock` is
unchanged for these crates.

## Copied code

**None.** No code was copied from another project. The syscall numbers,
Landlock / `mount_setattr` structs and cgroup v2 file semantics come from the
Linux UAPI headers and the `man 2` / `man 7` pages (kernel documentation; no
code copied).

## References consulted (read-only)

- `man 7 cgroups`, `man 7 cgroup_namespaces`, kernel
  `Documentation/admin-guide/cgroup-v2.rst` — delegation rules, `cgroup.kill`,
  `pids.max`, `memory.max`, `memory.oom.group`, the "no internal processes"
  rule. Used for the per-run cgroup and `cgroup-detect`'s checks.
- `man 2 close_range`, `man 2 unshare`, `man 2 prctl` (`PR_SET_NO_NEW_PRIVS`,
  `PR_CAPBSET_*`, `PR_CAP_AMBIENT`), `man 7 user_namespaces` — the fd and
  capability handling.

All of the above are documentation, not licensed source; nothing was vendored.
