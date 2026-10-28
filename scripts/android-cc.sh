#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# DESK-ID: openmind.UCPU-1
#
# C compiler for Android targets when no NDK is configured (slot 399,
# fmt-android-baseline, 2026-10-28 CET +01:00).
#
# .cargo/config.toml [env] points CC_aarch64_linux_android and
# CC_armv7_linux_androideabi here, so a plain
#   cargo check --workspace --target aarch64-linux-android
# works on any box with zig, like scripts/ucpu-gate.sh check does. Without it,
# cc-rs looks for aarch64-linux-android-clang, fails, and the bundled SQLite
# in libsqlite3-sys stops the whole check.
#
# Order (first match wins):
#   1. an NDK clang named <triple>-clang or <triple><api>-clang on PATH, or in
#      $ANDROID_NDK_HOME / $ANDROID_NDK_ROOT: used as is (real Android build).
#   2. zig cc against zig's musl headers: compile only. The object is fine for
#      `cargo check` (it never links) but must not be linked into an Android
#      binary; a real Android link needs the NDK (docs/ucpu.md).
# An explicit CC_<triple> in the environment always wins over this file,
# because cargo [env] never overrides a variable that is already set.
set -u

t=${TARGET:-}
for a do
  case "$a" in --target=*) t=${t:-${a#--target=}} ;; esac
done
case "$t" in
  aarch64-linux-android) zt=aarch64-linux-musl ;;
  armv7-linux-androideabi) zt=arm-linux-musleabihf ;;
  *) echo "android-cc.sh: unsupported target '$t'" >&2; exit 2 ;;
esac

# 1. NDK clang
ndk_triple=$t
[ "$t" = armv7-linux-androideabi ] && ndk_triple=armv7a-linux-androideabi
for d in ${ANDROID_NDK_HOME:-} ${ANDROID_NDK_ROOT:-}; do
  [ -d "$d/toolchains/llvm/prebuilt/linux-x86_64/bin" ] &&
    PATH=$d/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH
done
if command -v "$ndk_triple-clang" >/dev/null 2>&1; then
  exec "$ndk_triple-clang" "$@"
fi
for api in 35 34 33 32 31 30 29 28 27 26 25 24 23 22 21; do
  if command -v "$ndk_triple$api-clang" >/dev/null 2>&1; then
    exec "$ndk_triple$api-clang" "$@"
  fi
done

# 2. zig cc, musl headers (check only). Drop cc-rs's --target=<rust triple>.
zig=${ZIG:-zig}
command -v "$zig" >/dev/null 2>&1 || {
  echo "android-cc.sh: no NDK clang for $t and no zig on PATH (scripts/install-zig.sh)" >&2
  exit 127
}
for a do
  shift
  case "$a" in --target=*) continue ;; esac
  set -- "$@" "$a"
done
exec "$zig" cc -target "$zt" "$@"
