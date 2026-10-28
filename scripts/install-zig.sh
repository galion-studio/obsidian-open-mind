#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# DESK-ID: openmind.UCPU-1
#
# Install the pinned zig (C cross compiler for bundled SQLite) into DIR and
# print the path of the zig binary. Checksum-verified tarball from
# ziglang.org; no apt, no root. Used by CI (self-eval.yml, ucpu jobs).
#
#   scripts/install-zig.sh DIR   ->  DIR/zig
set -eu
ZIG_VERSION=0.15.2
ZIG_SHA256=02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239
dir=${1:?usage: install-zig.sh DIR}
if [ -x "$dir/zig" ] && [ "$("$dir/zig" version)" = "$ZIG_VERSION" ]; then
  printf '%s\n' "$dir/zig"
  exit 0
fi
tgz="zig-x86_64-linux-${ZIG_VERSION}.tar.xz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sSfL -o "$tmp/$tgz" "https://ziglang.org/download/${ZIG_VERSION}/$tgz"
printf '%s  %s\n' "$ZIG_SHA256" "$tmp/$tgz" | sha256sum -c - >&2
mkdir -p "$dir"
tar -C "$dir" --strip-components=1 -xJf "$tmp/$tgz"
printf '%s\n' "$dir/zig"
