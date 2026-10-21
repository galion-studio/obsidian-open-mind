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

## C code still needs a C cross compiler
Five crates compile bundled SQLite C (`libsqlite3-sys` 0.30.1): desk-hermes,
hermes-rs, hermes-gateway, one-defence, openmind-memory. With only the linker
fix, those fail at `cc-rs` with
`failed to find tool "aarch64-linux-musl-gcc"`; all other crates build.
The config cannot name a C compiler portably (path differs per machine), so
set it per shell, as `scripts/ucpu-gate.sh` does with zig cc:

```sh
Z=/path/to/zig   # 0.15.2, scripts/install-zig.sh
W=$CARGO_TARGET_DIR/zigw; mkdir -p "$W"
printf '#!/bin/sh\nfor a do shift; case "$a" in --target=*) continue ;; esac; set -- "$@" "$a"; done\nexec %s cc -target aarch64-linux-musl "$@"\n' "$Z" > "$W/cc-aarch64"
printf '#!/bin/sh\nexec %s ar "$@"\n' "$Z" > "$W/ar"; chmod +x "$W/cc-aarch64" "$W/ar"
export CC_aarch64_unknown_linux_musl=$W/cc-aarch64 AR_aarch64_unknown_linux_musl=$W/ar
cargo build --release --workspace --target aarch64-unknown-linux-musl   # no LINKER env needed
```

(On the box zig is at `/workspace/tools/zig-x86_64-linux-0.15.2/zig`.)

## Verified
See `docs/real-tests/toolchain-aarch64.md`. Short version: all 27 workspace
binaries build static for aarch64 musl and for armv7 musl with no linker env;
aarch64-linux-android `cargo check --all-targets` and x86_64 musl release still pass;
tuner, galion-one and openmind-memory (SQLite write + read) answer HTTP
under qemu-aarch64. Regression test:
`crates/galion-realtest/tests/real_toolchain_aarch64.rs`.
