<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# UCPU-1: universal-CPU gate for every binary

Owner constraint: `UNIVERSAL-CPU.md`. Gate: `scripts/ucpu-gate.sh`. CI:
`.github/workflows/self-eval.yml` jobs `ucpu-tier1`, `ucpu-tier2`, `ucpu-qemu`
(they replace the old `tier1` job, which built and printed sizes but did not fail).
Measured on the box on 2026-10-20 between 18:53 and 19:05 CEST (+02:00) with
rustc 1.99.1, zig 0.15.2 and qemu-user-static 10.0.13, from base 13a296c.

## What the gate does

| stage | what | fails when |
|---|---|---|
| `grep` | tracked `*.rs`: `asm!`, `global_asm!`, `naked_asm!`, `std::arch` / `core::arch`, `#[target_feature]`, `cfg(target_feature = ...)`, `is_*_feature_detected!`, `_mm*_` (x86) and `vld/vst` (NEON) intrinsics. Tracked `*.toml`, `.cargo/config`, `*.rs`, `*.sh`, `*.yml`: `target-cpu`, `target-feature`, `-C target...`, `-mcpu=`, `-march=native`. The current `RUSTFLAGS`. `openssl-sys` in either `Cargo.lock`. | any hit. `crates/repo-check` is skipped (its pattern table names these strings); a line can opt out with `ucpu-gate: allow` (none do). |
| `build` | `cargo build --release --workspace --locked` plus omnode (`edge/openmind-node`, its own workspace) for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` | any build error |
| `static` | every built ELF (workspace + omnode, both targets): `readelf` shows no `PT_INTERP`, no `DT_NEEDED`, and `e_machine` matches the target | a dynamic loader, a shared library, or the wrong arch |
| `size` | `scripts/size-budget.sh --check-dir` per target dir | any binary over 2 MiB (2,097,152 bytes) or still carrying `.symtab` |
| `check` | `cargo check --workspace --all-targets --locked` plus omnode for `armv7-unknown-linux-musleabihf` and `aarch64-linux-android` | any error |
| `clippy` | (not in `all`; CI runs it) clippy `-D warnings` per tier-1 target | any warning |

`UCPU_TIER1` / `UCPU_TIER2` narrow the target lists (CI runs one target per
matrix leg). Builds run `nice -n 10 ... -j 4` and honour `CARGO_TARGET_DIR`.

### C code: one cross compiler for every target (zig cc)

Five crates compile SQLite from C through `rusqlite` "bundled"
(`libsqlite3-sys` 0.30.1): desk-hermes, hermes-rs, hermes-gateway (through
hermes-rs), one-defence and openmind-memory. The gate writes small wrappers in
`$CARGO_TARGET_DIR/ucpu-zig/` and exports `CC_<triple>` / `AR_<triple>`:

| rust target | zig target used for C | links? |
|---|---|---|
| x86_64-unknown-linux-musl | x86_64-linux-musl | yes, with rust-lld |
| aarch64-unknown-linux-musl | aarch64-linux-musl | yes, with rust-lld |
| armv7-unknown-linux-musleabihf | arm-linux-musleabihf | yes, with rust-lld (used by the qemu test) |
| aarch64-linux-android | aarch64-linux-musl (headers only) | **no**: `cargo check` only |

The wrapper drops cc-rs's `--target=<rust triple>` and passes zig's own
`-target`. No aarch64/arm gcc and no NDK are needed for the gate.
`scripts/install-zig.sh` installs the pinned zig 0.15.2 (sha256-checked) for CI.

### Android: what a real link needs (NDK)

`zig cc -target aarch64-linux-android` fails with
`unable to provide libc for target 'aarch64-linux.5.10...6.16-android.29'`,
and `#include <stdio.h>` is not found: zig ships no bionic. A `cargo check`
never links, so the SQLite object only has to compile. For that we compile it
against zig's musl headers. The object is never used. libsqlite3-sys adds
`-DSQLITE_TEMP_STORE=3` for Android on its own, so the check still covers the
Android `cfg` paths of the Rust code.

To *build* (link) an Android binary you need the Android NDK (r26 or newer):
bionic headers + `libc.so`/`libm.so`/`libdl.so` stubs and `crtbegin_dynamic.o`
in `toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/<api>/`, and set:

```
CC_aarch64_linux_android=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang
AR_aarch64_linux_android=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$CC_aarch64_linux_android
```

Plain `cargo check --target aarch64-linux-android` (or `armv7-linux-androideabi`)
without the gate also works since slot 399: `.cargo/config.toml` `[env]` points
`CC_`/`AR_<triple>` at `scripts/android-cc.sh` / `android-ar.sh` (NDK clang if found,
else zig cc with musl headers, check only). An exported `CC_<triple>` wins.

Simpler path for phones: Termux runs Linux binaries, and the static
`aarch64-unknown-linux-musl` binaries need no libc at all (`DT_NEEDED` = 0),
so they are the recommended phone artefact. Proven here only under qemu-user,
not yet on a real phone (see the open items below).

## Results (box, 2026-10-20 CEST)

All 40 tier-1 binaries (19 workspace bins + omnode, times 2 targets) are static:
no `PT_INTERP`, no `DT_NEEDED`, correct `e_machine`. Tier 2 `cargo check`
(all targets, workspace + omnode) passes for armv7 and aarch64-android after
the omnode fix below. Grep gate: 0 hits.

Release sizes in bytes (stripped, opt-level z, lto, codegen-units 1, panic abort):

| binary | x86_64 musl | aarch64 musl |
|---|---:|---:|
| desk-hermes | 1445392 | 1348128 |
| galion-boot | 852344 | 735472 |
| galion-desk | 753416 | 660264 |
| galion-one | 822184 | 732216 |
| galion-origin | 475528 | 428544 |
| galleon-arena | 451512 | 401408 |
| guardian-tick | 730328 | 634888 |
| **hermes-gateway** | **1921312** (was 2129888) | 1750384 (was 1940736) |
| link-watch | 725528 | 627080 |
| omnode | 1410024 | 1205696 |
| one-defence | 1368592 (was 1579184) | 1283952 (was 1476128) |
| one-set-model | 422360 | 374008 |
| one-talk | 422232 | 373736 |
| openmind-discovery | 453112 | 401944 |
| openmind-mcp-stack | 458200 | 401456 |
| openmind-memory | 1759968 (was 1967200) | 1620104 (was 1809160) |
| persona-plex-relay | 480952 | 425920 |
| repo-check | 462328 | 407144 |
| room | 469832 | 423104 |
| tuner | 450568 | 404504 |

armv7 (built for the qemu test): galion-one 709004, tuner 374444,
openmind-memory 1583556, hermes-gateway 1717772.

## hermes-gateway over budget: fixed

At 13a296c the x86_64 musl hermes-gateway was 2,129,888 bytes, 32,736 over
2 MiB. The release profile already had opt-level z, lto, codegen-units 1,
panic abort and strip, so those had nothing left to give. The weight was
SQLite: libsqlite3-sys "bundled" always compiles in FTS3, R*Tree, DBSTAT,
STAT4, SOUNDEX, column metadata and runtime extension loading. No crate uses
any of them (grep of `crates/*/src` and every `CREATE` statement). The new
`.cargo/config.toml` sets `LIBSQLITE3_FLAGS` to undefine them and adds
`SQLITE_OMIT_LOAD_EXTENSION`, which also drops the `dlopen` path, which a
static binary cannot use anyway. Result: hermes-gateway 1,921,312 bytes
(−208,576, 175,840 under budget). desk-hermes, one-defence and
openmind-memory shrink by about 200 KB each too.

Kept on purpose: **FTS5 and JSON**. The legacy Python Hermes `state.db` may
contain FTS5 tables. hermes-gateway opens it read-only, but we keep the
module so it still works if a query ever touches them.

Not done, and why:
- Dropping the `sqlite` feature from hermes-gateway (the JSONL store) would
  save more, but it breaks read-compat with the live `state.db`, a behaviour
  change for a production service. Owner call, not needed now.
- `serde_json/preserve_order` (indexmap) stays: the API output key order is
  part of the Python-compat contract.

## Bug found by the gate: omnode did not build for armv7

`edge/openmind-node/src/seccomp.rs` defined `AUDIT_ARCH` only for x86_64 and
aarch64, so `cargo check --target armv7-unknown-linux-musleabihf` failed
(`E0425: cannot find value AUDIT_ARCH`). This is fixed: armv7 gets
`AUDIT_ARCH_ARM` (0x40000028 = `EM_ARM | __AUDIT_ARCH_LE`, from
`linux/audit.h`) and the same 24-syscall deny list as aarch64, with ARM EABI
numbers checked against `arch/arm/tools/syscall.tbl`. Any other architecture
now compiles and fails closed: `AUDIT_ARCH = 0` never matches, so the filter
kills the sandboxed child, and `seccomp::available()` reports false.

## Real-life test

`crates/galion-realtest/tests/real_sh_ucpu.rs` and `crates/tuner/tests/real_ucpu.rs`
(`GALION_REAL=1`). Evidence: `docs/real-tests/ucpu.md`.

## Open items

- openmind-nerve has no binary at 13a296c (it is a library), so "nerve" could
  not be started. It is covered by the build/check of its dependants.
- No real-hardware run yet (phone/Termux, Pi-class armv7): only qemu-user.
- Android is check-only until an NDK is available (above).
- The armv7 omnode seccomp filter is compile-checked, and its numbers are
  checked against the kernel table. It has not been enforced on a real ARM
  kernel (qemu-user does not run seccomp filters the way a native kernel does).
