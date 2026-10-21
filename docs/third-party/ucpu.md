<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party record: swarm/ucpu (UCPU-1)

**No new Rust crates and no copied code.** `Cargo.lock` and
`edge/openmind-node/Cargo.lock` are unchanged.

Build and test tools (not linked into any binary, not shipped):

| tool | version | licence (SPDX) | source | used for | why not write it ourselves |
|---|---|---|---|---|---|
| zig (`zig cc`, `zig ar`) | 0.15.2, sha256 `02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239` | MIT | https://ziglang.org/download/0.15.2/zig-x86_64-linux-0.15.2.tar.xz | C cross compiler for the bundled SQLite amalgamation (x86_64/aarch64/armv7 musl; headers only for the Android check). It was already used by the old CI `tier1` job; pinned in `crates/xtask/src/tools.rs` (formerly `scripts/install-zig.sh`). | A C cross toolchain per arch (gcc + musl sysroots) is far heavier; zig is one checksum-verified tarball for every target. |
| qemu-user-static | Debian 1:10.0.13+ds-0+deb13u1 (box), Ubuntu archive (CI) | GPL-2.0-only (tool only) | https://www.qemu.org/ , Debian package `qemu-user-static` | Runs the aarch64/armv7 test binaries on the x86_64 box/CI runner in `scripts/real/ucpu.sh` and `crates/tuner/tests/real_ucpu.rs`. | Emulating a CPU is not something to write. **Licence note:** GPL is not on the allowed list for *dependencies or copied code*. qemu is neither: it is an external test runner, invoked as a separate process, never linked, vendored or redistributed. Owner to confirm this reading. |

Facts used (not code):

| item | source | licence | use |
|---|---|---|---|
| ARM EABI syscall numbers (ptrace 26, mount 21, ...; 24 entries) | https://raw.githubusercontent.com/torvalds/linux/master/arch/arm/tools/syscall.tbl | GPL-2.0 WITH Linux-syscall-note (kernel UAPI) | omnode seccomp deny list for armv7. These are ABI numbers (interface facts); the table was not copied, only checked. |
| `AUDIT_ARCH_ARM = EM_ARM \| __AUDIT_ARCH_LE` = 0x40000028 | `/usr/include/linux/audit.h`, `linux/elf-em.h` | GPL-2.0 WITH Linux-syscall-note | Same; a single ABI constant. |

SQLite itself is unchanged in version (public domain, bundled by
`libsqlite3-sys` 0.30.1, MIT). `.cargo/config.toml` only changes its
compile-time options (`LIBSQLITE3_FLAGS`).
