#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# DESK-ID: openmind.B08
# Release size budget for the OpenMind workspace.
# Builds with nice -n 19 and -j 2, then fails if any workspace
# ELF executable is over 2 MiB (hermes-gateway: 2.5 MiB) or not stripped.
# Needs readelf (binutils), not file(1). Bash only.
set -eu

# 2 MiB. Fail when a binary is strictly larger than this.
LIMIT=$((2 * 1024 * 1024))
# hermes-gateway only. x86_64 musl was 2306688 bytes on the 2026-10-21 PR #12
# ucpu run (actions/runs/39692890293) after enroll and the coding tools, with
# the serial curve25519 backend already selected. Every other binary stays
# at 2 MiB (the next largest, openmind-nerve, was 1918568). 2.5 MiB.
HERMES_GATEWAY_LIMIT=$((5 * 1024 * 1024 / 2))

usage() {
  cat << 'HELP'
usage: size-budget.sh [--check-dir DIR] [--skip-build] [--target TRIPLE]

Build (default):
  nice -n 19 cargo build -j 2 --release --workspace
  CARGO_TARGET_DIR is honoured and never overwritten.

Check:
  ELF executables in $CARGO_TARGET_DIR/release
  (or $CARGO_TARGET_DIR/<triple>/release when --target is set).
  If CARGO_TARGET_DIR is unset, <repo>/target is used.
  A bin fails when its size is greater than 2097152 bytes
  (hermes-gateway: 2621440) or when it still has a .symtab section
  (not stripped).
  ELF detection and the strip check use readelf (binutils);
  file(1) is not needed.

--check-dir DIR   do not build; check ELF executables in DIR
--skip-build      do not build; check the cargo release directory
--target TRIPLE   optional musl (or other) target; not required

exit 0  every bin is within budget and stripped
exit 1  oversize, not stripped, no bins, or bad arguments
HELP
}

die() {
  printf 'size-budget: %s\n' "$*" >&2
  exit 1
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/.." && pwd)

check_dir=""
skip_build=0
triple=""

while [ $# -gt 0 ]; do
  case "$1" in
    --check-dir)
      [ $# -ge 2 ] || die "--check-dir needs a directory"
      check_dir=$2
      skip_build=1
      shift 2
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    --target)
      [ $# -ge 2 ] || die "--target needs a triple"
      triple=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

export LC_ALL=C

if [ -n "$check_dir" ]; then
  [ -d "$check_dir" ] || die "not a directory: $check_dir"
  bins_dir=$check_dir
else
  if [ "$skip_build" -eq 0 ]; then
    if ! command -v cargo >/dev/null 2>&1; then
      if [ -x /root/.cargo/bin/cargo ]; then
        PATH="/root/.cargo/bin:${PATH}"
        export PATH
      else
        die "cargo not found"
      fi
    fi
    cd "$root"
    if [ -n "$triple" ]; then
      nice -n 19 cargo build -j 2 --release --workspace --target "$triple"
    else
      nice -n 19 cargo build -j 2 --release --workspace
    fi
  fi

  if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    case "$CARGO_TARGET_DIR" in
      /*) bins_parent=$CARGO_TARGET_DIR ;;
      *) bins_parent=$root/$CARGO_TARGET_DIR ;;
    esac
  else
    bins_parent=$root/target
  fi
  if [ -n "$triple" ]; then
    bins_dir=$bins_parent/$triple/release
  else
    bins_dir=$bins_parent/release
  fi
  [ -d "$bins_dir" ] || die "release dir not found: $bins_dir"
fi

ts=$(TZ=Europe/Zagreb date -Iseconds)
printf 'size-budget: DESK-ID openmind.B08\n'
printf 'size-budget: time %s\n' "$ts"
printf 'size-budget: dir %s\n' "$bins_dir"
printf 'size-budget: limit_bytes %s\n' "$LIMIT"
printf 'size-budget: hermes-gateway limit_bytes %s\n' "$HERMES_GATEWAY_LIMIT"
printf '%-32s %12s %12s %8s %s\n' "binary" "bytes" "stripped" "over" "status"

command -v readelf >/dev/null 2>&1 || die "readelf not found (install binutils)"

found=0
failed=0
shopt -s nullglob
for path in "$bins_dir"/*; do
  [ -f "$path" ] || continue
  name=$(basename -- "$path")
  case "$name" in
    .*) continue ;;
    *.d|*.rlib|*.rmeta|*.so|*.a) continue ;;
  esac
  # ELF magic first (cheap), then the ELF type: EXEC, or DYN with an entry
  # point (static-pie and PIE executables; shared objects have entry 0x0).
  [ "$(head -c 4 -- "$path" | od -An -c | tr -d " ")" = "177ELF" ] || continue
  hdr=$(readelf -hW -- "$path" 2>/dev/null) || continue
  etype=$(printf "%s\n" "$hdr" | sed -n "s/^ *Type: *\([A-Z]*\).*/\1/p")
  entry=$(printf "%s\n" "$hdr" | sed -n "s/^ *Entry point address: *//p")
  case "$etype" in
    EXEC) ;;
    DYN) [ "$entry" != "0x0" ] || continue ;;
    *) continue ;;
  esac
  found=$((found + 1))
  size=$(stat -c '%s' -- "$path")
  stripped=yes
  if readelf -SW -- "$path" 2>/dev/null | grep -q "\] \.symtab "; then
    stripped=no
  fi
  cap=$LIMIT
  # Release artifacts carry a commit tag: hermes-gateway-<7+ hex>[-suffix]
  # (slot 312: hermes-gateway-38a0770 was checked against 2 MiB, not 2.5 MiB).
  case "$name" in
    hermes-gateway | hermes-gateway-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*)
      cap=$HERMES_GATEWAY_LIMIT
      ;;
  esac
  over=no
  if [ "$size" -gt "$cap" ]; then
    over=yes
  fi
  status=ok
  if [ "$stripped" != yes ] || [ "$over" = yes ]; then
    status=FAIL
    failed=1
  fi
  printf '%-32s %12s %12s %8s %s\n' "$name" "$size" "$stripped" "$over" "$status"
done
shopt -u nullglob

if [ "$found" -eq 0 ]; then
  printf 'size-budget: no ELF executables in %s\n' "$bins_dir" >&2
  exit 1
fi

if [ "$failed" -ne 0 ]; then
  printf 'size-budget: FAIL\n' >&2
  exit 1
fi
printf 'size-budget: ok (%s binaries)\n' "$found"
exit 0
