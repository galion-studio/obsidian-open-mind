#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# build-dist-bins.sh: rebuild the five binaries that used to be committed in
# dist/bin/ (removed in slot 445, D-428-2; see
# docs/release/dist-bin-removed-manifest.txt) from source, reproducibly.
#
#   scripts/build-dist-bins.sh [--target TRIPLE] [--out DIR]
#
# Default target x86_64-unknown-linux-musl, default out
# $CARGO_TARGET_DIR (or target/)/dist-bins/<triple>. Writes the binaries and a
# SHA256SUMS there, plus RECIPE.txt with the exact command, toolchain
# versions, GALION_BUILD_COMMIT and SOURCE_DATE_EPOCH (= the commit time), so
# a second run from another checkout path must give the same sha256.
# It builds only; it never copies anything into dist/bin/, installs,
# uploads or publishes. A musl C compiler must be on PATH for the crates that
# compile C (the swarm box uses zig 0.15.2 wrappers named
# <arch>-linux-musl-gcc / -ar). No Python.
set -euo pipefail
export LC_ALL=C
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
target=x86_64-unknown-linux-musl
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --target) target=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "build-dist-bins: unknown argument $1" >&2; exit 2 ;;
  esac
done
bins=(desk-hermes galion-desk galion-one link-watch room)
cd "$root"
commit=$(git rev-parse HEAD)
epoch=$(git log -1 --format=%ct HEAD)
tdir=${CARGO_TARGET_DIR:-$root/target}
[ -n "$out" ] || out="$tdir/dist-bins/$target"
pkgs=()
for b in "${bins[@]}"; do pkgs+=(-p "$b"); done
cmd=(cargo build --release --locked -j "${JOBS:-4}" --target "$target" "${pkgs[@]}")
GALION_BUILD_COMMIT=$commit SOURCE_DATE_EPOCH=$epoch \
  RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$root=/src" "${cmd[@]}"
mkdir -p "$out"
for b in "${bins[@]}"; do
  install -m 0755 "$tdir/$target/release/$b" "$out/$b"
done
(cd "$out" && sha256sum "${bins[@]}" > SHA256SUMS)
{
  echo "commit $commit"
  echo "SOURCE_DATE_EPOCH $epoch"
  echo "target $target"
  echo "command GALION_BUILD_COMMIT=$commit SOURCE_DATE_EPOCH=$epoch RUSTFLAGS='--remap-path-prefix=<checkout>=/src' ${cmd[*]}"
  echo "rustc $(rustc --version)"
  echo "cargo $(cargo --version)"
  echo "cc $(command -v "${target%%-unknown-*}-linux-musl-gcc" || echo none)"
  for b in "${bins[@]}"; do echo "size $b $(stat -c %s "$out/$b")"; done
} > "$out/RECIPE.txt"
cat "$out/SHA256SUMS"
