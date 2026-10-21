<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: sec-denylist-parity (slot 220, security S4.3)

Branch `swarm/sec-denylist-parity`, base integration-5 0d68f46. Box only.
No sandbox run on the box (sandbox_vps_only): `omnode sandbox denylist`
installs no filter and tries no syscall; it only reads the compiled tables.

## What changed

- `edge/openmind-node/src/seccomp_table.rs` (new): the x86_64 / aarch64 /
  armv7 deny arrays compiled on every target, a catalog of 54 syscalls with
  their number per arch (kernel tables), written exceptions, `check()` and
  `markdown()`.
- `seccomp.rs`: `DENIED` / `DENIED_UNIFIED` now point at those arrays (same
  BPF program layout).
- `omnode sandbox denylist [--markdown] [--check FILE]`: JSON report for the
  binary's own arch (live filter == table, parity, doc current), exit 2 on any
  gap.
- `docs/security/denylist-parity.md`: generated table, checked by the unit test
  `checked_in_doc_is_current` (CI runs omnode tests in self-eval.yml).

## Findings

1. **armv7 gap (fixed):** `kexec_file_load` was denied on x86_64 (320) and
   aarch64 (294) but allowed on armv7 (401, arch/arm/tools/syscall.tbl). The
   old comment said "same set as aarch64". armv7 now denies 401.
2. armv7 also denies OABI `umount` (22): free on EABI kernels (ENOSYS anyway),
   closes the old-ABI twin of `umount2` where OABI_COMPAT is on.
3. All three arches now deny `quotactl_fd` (443) and `open_tree_attr` (467),
   the fd-based twins of the already denied `quotactl` and `open_tree`.
   (Owner decision D-220-2 below; trivially revertible.)
4. `crates/openmind-sandbox` has **no seccomp deny-list** (by design: userns,
   Landlock, rlimits, caps). Its own raw syscall numbers (close_range 436,
   mount_setattr 442, landlock 444-446) are the same on all three arches;
   `openmind_sandbox_raw_numbers_match` parses its source and checks them.
5. Watch list (allowed everywhere, no parity issue): setns, chroot, kcmp,
   syslog, uselib, modify_ldt (x86 only), _sysctl, process_madvise.

Totals: x86_64 40, aarch64 38, armv7 39 denied numbers (13 unified). Before:
x86_64 38, aarch64 36, armv7 35.

## Box runs (2026-10-21, +02:00)

| time | command | result |
|---|---|---|
| 23:00 | `cargo test --locked -- --skip sandbox:: --skip boot:: --skip install_ --skip self_test` (x86_64) | 55 passed, 0 failed (11 seccomp_table, 3 seccomp pure) + 1 |
| 23:01 | `cargo test --target aarch64-unknown-linux-musl --bins -- seccomp --skip install_ --skip self_test` (qemu-aarch64) | 15 passed, 0 failed |
| 23:01 | same for armv7-unknown-linux-musleabihf (qemu-arm) | 15 passed, 0 failed |
| 23:05:19 | `GALION_REAL=1 OMNODE_BIN=<x86_64 musl release> cargo test --release --test real_denylist_parity` | 1/1: arch x86_64, audit 0xc000003e, live 40, parity_ok, doc_current; stale doc exit 2; missing file exit 2; bad flag exit 2 |
| 23:05:19 | same, aarch64 musl release via OMNODE_RUNNER=qemu-aarch64 | 1/1: arch aarch64, audit 0xc00000b7, live 38 |
| 23:05:19 | same, armv7 musl release via OMNODE_RUNNER=qemu-arm | 1/1: arch armv7, audit 0x40000028, live 39, 401 denied |

Release binaries (static musl, rust-lld): x86_64 1,545,864 B (sha256 05c894f6d67d8437...),
aarch64 1,317,088 B (2652150a1ae3e1b1...), armv7 1,296,588 B (ccbab58499e2b969...).

Negative checks (unit tests): dropping 401 from armv7 gives exactly one
violation `kexec_file_load armv7: allowed here (nr 401) but denied on x86_64,
aarch64`; removing the iopl/aarch64 exception gives `missing here`; stale,
empty-reason and unknown-name exceptions, duplicate numbers and numbers not in
the catalog all fail.

## VPS (read-only, 2026-10-21 23:05:30 +02:00)

- `uname`: Linux 6.8.12-43-pve x86_64; the login shell already runs under
  `Seccomp: 2` (container filter). 6.8 has no `open_tree_attr` (6.15): the new
  deny answers EPERM where the kernel would answer ENOSYS, no behaviour lost.
- omnode is not installed on the VPS; nothing deployed.
- Nerve 127.0.0.1:8670 /state: mode live, score 76 / grade C, verdict
  "degraded: 2 open incidents" (galion-one mouth 503 loading). `galion-nerve`
  and `gaia-cortex` units active.
- Enforcement on real aarch64/armv7 kernels is not part of this row (box only;
  sandbox runs belong on the VPS, which is x86_64).

## Slip

The qemu filter `-- seccomp` also matched
`sandbox::tests::seccomp_reported_and_blocks_ptrace_when_enforced` (an omnode
sandbox test) under qemu-user on the box at 23:01; it passed. Not repeated.

## Owner decisions

- D-220-1: keep the armv7 kexec_file_load (401) deny (recommended: yes, parity).
- D-220-2: keep the new unified denies quotactl_fd (443) and open_tree_attr (467).
- D-220-3: deny `modify_ldt` on x86_64 too (x86-only, so no parity effect).
