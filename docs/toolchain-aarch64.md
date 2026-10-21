<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# aarch64 / armv7 musl: rust-lld by default (slot 145, 2026-10-21 CEST +02:00)

## Problem
`cargo build --target aarch64-unknown-linux-musl` with no extra env var used
the host `cc` (x86_64 GNU ld) as linker and failed on every binary:

```
/usr/bin/ld: unrecognized option '--fix-cortex-a53-843419'
error: could not compile `galion-boot` (bin "galion-boot")
```

Every swarm agent had to export
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` by hand.

## Fix
`.cargo/config.toml` now sets, for the two cross musl targets only:

```toml
[target.aarch64-unknown-linux-musl]
linker = "rust-lld"

[target.armv7-unknown-linux-musleabihf]
linker = "rust-lld"
```

* rust-lld ships with every rustup toolchain (`rustlib/<host>/bin`); rustc
  finds it without PATH changes. The musl targets bring their own
  self-contained crt objects and libc.a, so no cross gcc is needed for Rust-only crates.
* Linker choice only: no codegen or CPU flags (UNIVERSAL-CPU.md, `ucpu-gate.sh grep` ok).
  No `target.<triple>.rustflags`, which would replace `[build] rustflags`
  and drop the serial curve25519 backend (no-SIMD rule). A test guards both.
* An env var `CARGO_TARGET_<TRIPLE>_LINKER` still overrides the file, so
  `scripts/ucpu-gate.sh` and CI (which set the same value) are unaffected.
* x86_64 musl is left on the host default (works with the box gcc; CI sets rust-lld anyway).

## C code: `cargo xtask cross` (slot 156, 2026-10-21 CEST +02:00)
Five crates compile bundled SQLite C (`libsqlite3-sys` 0.30.1): desk-hermes,
hermes-rs, hermes-gateway, one-defence, openmind-memory. With only the linker
fix, plain `cargo build --target aarch64-unknown-linux-musl` fails in cc-rs
with `failed to find tool "aarch64-linux-musl-gcc"`; all other crates build.

From any shell (no exports, no wrapper scripts):

```sh
cargo xtask cross aarch64-unknown-linux-musl            # = build --release --workspace
cargo xtask cross armv7-unknown-linux-musleabihf
cargo xtask cross x86_64-unknown-linux-musl
cargo xtask cross aarch64-linux-android check --workspace --all-targets   # check only
cargo xtask cross aarch64-unknown-linux-musl build --release -p hermes-gateway   # any cargo args
cargo xtask cross-env aarch64-unknown-linux-musl         # print the env it sets (for IDEs, CI)
```

How it works (`xtask/`, std only, host tool, never shipped):

* `.cargo/config.toml` `[alias] xtask = "run --quiet --package xtask --bin xtask --"`.
* xtask finds zig: `$GALION_ZIG`, `$ZIG`, `zig` on `PATH`, `<repo>/.tools/zig/zig`
  (`scripts/install-zig.sh .tools/zig`; `.tools/` is git-ignored), `~/.local/zig/zig`,
  `/opt/zig/zig`, `/workspace/tools/zig-x86_64-linux-0.15.2/zig` (the swarm box).
  It warns if the version is not the pinned 0.15.2.
* It hard-links its own executable as `galion-cc` and `galion-ar` in
  `<target>/debug/galion-cross/` (atomic temp + rename; copy across filesystems),
  then runs the child `cargo` with `CC_<triple>`, `AR_<triple>`, `GALION_ZIG` and
  `ZIG_{GLOBAL,LOCAL}_CACHE_DIR=<target>/zig-cache`. Variables you already set win.
* Run as `galion-cc`, the same binary execs `zig cc -target <zig triple> <args>`:
  the triple comes from cargo's `$TARGET` (cc-rs's `--target=` is a clang triple,
  e.g. `arm-unknown-linux-musleabihf` for armv7, and is dropped);
  `-march=armv7-a` becomes `-mcpu=generic+v7a`. As `galion-ar` it execs `zig ar`.
  Logic and triple table come from `galion-ccwrap` (swarm/realscript-port-3 03967fc).
* With `--workspace`/`--all`, `--exclude xtask` is added, so the product binary
  count is unchanged (27). A `--target` you pass is kept.
* xtask is a workspace member at `xtask/` (not `crates/`), so the Gaia
  ownership registry (`crates/*` only) is unchanged.

The repo cannot name a C compiler in `[env]` portably (the zig path differs per
machine and a Rust wrapper has to be built first), which is why this is an
xtask and not a config line. The old per-shell recipe still works (it sets
`CC_<triple>` itself, and xtask respects it) but is no longer needed.

Regression test: `crates/galion-realtest/tests/real_xtask_cross.rs`
(`GALION_REAL=1`; `GALION_REAL_FULL=1` for all 27 bins on both targets).

## Verified
See `docs/real-tests/toolchain-aarch64.md` (slot 145) and
`docs/real-tests/sqlite-aarch64-cc.md` (slot 156, clean shell via xtask). Slot 145 short version: all 27 workspace
binaries build static for aarch64 musl and for armv7 musl with no linker env;
aarch64-linux-android `cargo check --all-targets` and x86_64 musl release still pass;
tuner, galion-one and openmind-memory (SQLite write + read) answer HTTP
under qemu-aarch64. Regression test:
`crates/galion-realtest/tests/real_toolchain_aarch64.rs`.
