<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: sec-denylist-parity (slot 220)

No new crates and no copied code. `edge/openmind-node/Cargo.lock` is unchanged.

Data source (numbers only, no code copied): the Linux kernel syscall tables,
read 2026-10-21 22:55 +02:00 from https://github.com/torvalds/linux (master):

| file | used for |
|---|---|
| arch/x86/entry/syscalls/syscall_64.tbl | x86_64 numbers |
| scripts/syscall.tbl (asm-generic, abi common/64) | aarch64 numbers |
| arch/arm/tools/syscall.tbl | armv7 (EABI + OABI) numbers |

Syscall numbers are kernel ABI facts (and the UAPI is under the
Linux-syscall-note exception), so no licence obligation follows from
listing them. A second independent source is the `libc` crate (MIT OR
Apache-2.0, already a dependency): `catalog_matches_libc_for_this_arch`
compares every number libc has for the build arch.
