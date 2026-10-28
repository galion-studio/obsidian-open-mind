#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# DESK-ID: openmind.UCPU-1
#
# Archiver for Android targets when no NDK is configured (slot 399,
# fmt-android-baseline, 2026-10-28 CET +01:00). Pair of scripts/android-cc.sh;
# .cargo/config.toml [env] sets AR_aarch64_linux_android and
# AR_armv7_linux_androideabi here. Order: llvm-ar from $ANDROID_NDK_HOME /
# $ANDROID_NDK_ROOT or PATH, then `zig ar`, then the host `ar` (an .a archive
# is target-neutral). An exported AR_<triple> always wins over this file.
set -u

for d in ${ANDROID_NDK_HOME:-} ${ANDROID_NDK_ROOT:-}; do
  [ -d "$d/toolchains/llvm/prebuilt/linux-x86_64/bin" ] &&
    PATH=$d/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH
done
if command -v llvm-ar >/dev/null 2>&1; then exec llvm-ar "$@"; fi
zig=${ZIG:-zig}
if command -v "$zig" >/dev/null 2>&1; then exec "$zig" ar "$@"; fi
if command -v ar >/dev/null 2>&1; then exec ar "$@"; fi
echo "android-ar.sh: no llvm-ar, zig or ar on PATH" >&2
exit 127
