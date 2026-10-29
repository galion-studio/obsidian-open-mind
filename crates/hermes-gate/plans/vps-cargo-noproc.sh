#!/bin/bash
# SPDX-License-Identifier: MIT
# cargo for hermes-gate stages on the VPS (LXC, lxcfs). The sandbox there
# cannot mount a fresh /proc (mount_too_revealing), so /proc is masked and
# has no /proc/self: rustup proxies, rustc's $ORIGIN rpath and the rust-lld
# wrapper cannot find their own path. Call the toolchain binaries directly,
# give rustc its sysroot and libs, and link with the system cc/ld.
set -eu
T=/var/lib/galion-gate/toolchain/rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu
export PATH="$T/bin:/usr/bin:/bin" LD_LIBRARY_PATH="$T/lib" CARGO="$T/bin/cargo" RUSTC="$T/bin/rustc"
export RUSTFLAGS="--sysroot $T -Clinker-features=-lld" RUSTDOCFLAGS="--sysroot $T"
exec "$T/bin/cargo" "$@"
