<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: sec-tooling-licence-memo (slot 234)

Docs only. **No new dependency, no copied code.** This file records the external
programs and facts the tree already uses, as one register for the owner ruling in
`docs/security/tooling-licence-memo.md`. Status of every row: *pending owner ruling* (L1-L8).

## External programs run as separate processes (never linked, vendored or shipped)

| tool | licence (SPDX) | version box (Debian 13) / VPS (Ubuntu 24.04) | source | run by | memo line |
|---|---|---|---|---|---|
| strace | LGPL-2.1-or-later | 6.13+ds-1 / 6.8-0ubuntu2 | https://github.com/strace/strace | room tests, room.sh, edge-boot.sh | L1 |
| util-linux `unshare`, `nsenter`, `prlimit` | GPL-2.0-or-later | 2.41.5-0+deb13u1 / 2.39.3-9ubuntu6 | https://github.com/util-linux/util-linux | room, galion-backup, galion-guard real tests; room.sh, edge-boot.sh; slot 219 prlimit | L1 |
| qemu-user-static | GPL-2.0-only | 1:10.0.13+ds-0+deb13u1 / not installed | https://www.qemu.org/ | tuner real_ucpu, ucpu.sh, edge-boot.sh, CI self-eval ucpu-qemu | L1 |
| binutils `readelf` | GPL-3.0-or-later | 2.44-3 / 2.42-4ubuntu2.10 | https://www.gnu.org/software/binutils/ | ucpu-gate.sh, size-budget.sh, real scripts | L1 |
| bash, coreutils | GPL-3.0-or-later | box default / 5.2.21 | https://www.gnu.org/software/ | all scripts/real/*.sh | L1 |
| git | GPL-2.0-only | 1:2.47.3 / 1:2.43.0 | https://git-scm.com/ | 11 test files, ucpu-gate.sh, launch-gates.sh; **runtime**: history-purge (swarm/sec-runbooks) | L1, L2 |
| systemd `journalctl`, `systemctl` | LGPL-2.1-or-later | 257.13 / 255.4-1ubuntu8 | https://github.com/systemd/systemd | **runtime**: openmind-nerve, galion-guard, galion-health, gaia-live, galion-deploy | L2 |
| nftables `nft` | GPL-2.0-only | not installed / 1.0.9-1build1 | https://netfilter.org/projects/nftables/ | **runtime**: galion-guard backend (staged dry-run) | L2 |
| curl | curl | box default / 8.5.0 | https://curl.se | **runtime**: hermes-rs, link-watch; scripts | (allowed, MIT-style) |

## Facts, not code

| item | licence of the source | where | memo line |
|---|---|---|---|
| syscall numbers, `AUDIT_ARCH_*`, `seccomp_data` offsets | GPL-2.0 WITH Linux-syscall-note | edge/openmind-node/src/seccomp.rs | L3 |
| landlock rights bits and `repr(C)` attr layouts, `SYS_*` 436-446 | GPL-2.0 WITH Linux-syscall-note | crates/openmind-sandbox/src/lib.rs | L3 |

## Not used today (recorded so nobody adds them silently)

| tool | licence / terms | status | memo line |
|---|---|---|---|
| libFuzzer / cargo-fuzz | Apache-2.0 WITH LLVM-exception (legacy NCSA OR MIT) | 0 uses; fuzzing is std-only seeded Rust | L4 |
| CodeQL CLI | GitHub CodeQL Terms (not OSI): OSI codebases on GitHub.com or paid GHAS only | 0 uses; `codeql-action/upload-sarif` (MIT) only | L5 |
| BFG, git-filter-repo, Semgrep CE | GPL-3.0, MIT but Python, LGPL-2.1 | rejected | L6 |

## Toolchain parts linked into shipped binaries (allowed or proposed)

| part | licence | evidence | memo line |
|---|---|---|---|
| musl libc (self-contained `libc.a`) | MIT | Rust musl target | allowed |
| LLVM libunwind (self-contained `libunwind.a`) | Apache-2.0 WITH LLVM-exception | `strings omnode` shows 70 `libunwind` strings; omnode 0 NEEDED, no interpreter | L7 |
| glibc, libgcc_s (dynamic, OS-provided) | LGPL-2.1+, GPL-3.0 WITH GCC-exception-3.1 | `dist/bin/*` NEEDED entries (not in public tree, not in releases) | L8 |
