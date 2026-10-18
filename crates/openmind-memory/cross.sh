#!/bin/sh
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
# Cross builds for the universal-CPU targets. Rust links with rust-lld (musl is
# self-contained); the only C is SQLite's amalgamation (rusqlite `bundled`),
# compiled with `zig cc` as a portable cross C compiler.
#   ZIG=/path/to/zig  (default /workspace/oom-tools/zig-linux-x86_64-0.13.0/zig)
# Android: no NDK on the build box, so `cargo check` uses musl headers for the
# SQLite C file; a real Android/Termux build needs the NDK clang or `pkg install clang`.
set -eu
ZIG=${ZIG:-/workspace/oom-tools/zig-linux-x86_64-0.13.0/zig}
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
mk() { printf '#!/bin/sh\nargs=""\nfor a in "$@"; do case "$a" in --target=*) ;; *) args="$args \\"$a\\"" ;; esac; done\neval exec "%s" cc -target %s $args\n' "$ZIG" "$2" > "$W/cc-$1"; chmod +x "$W/cc-$1"; }
printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$ZIG" > "$W/ar"; chmod +x "$W/ar"
run() { # rust-target zig-target cmd
  t=$1; z=$2; cmd=$3; mk "$t" "$z"
  l=$(echo "$t" | tr - _); u=$(echo "$l" | tr a-z A-Z)
  env "CC_$l=$W/cc-$t" "AR_$l=$W/ar" "CARGO_TARGET_${u}_LINKER=rust-lld" \
    nice -n 19 cargo "$cmd" -j 1 --release -p openmind-memory --target "$t"
}
run x86_64-unknown-linux-musl x86_64-linux-musl build
run aarch64-unknown-linux-musl aarch64-linux-musl build
run armv7-unknown-linux-musleabihf arm-linux-musleabihf build
run aarch64-linux-android aarch64-linux-musl check
