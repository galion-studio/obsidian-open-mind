#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# DESK-ID: openmind.B08
# Release size budget for the OpenMind workspace.
# Builds with nice -n 19 and -j 2, then fails if any workspace ELF
# executable is over its budget or not stripped. Budgets come from ONE file,
# config/release/size-budget.tsv (slot 353): 2 MiB for every binary since
# slot 329 removed the hermes-gateway 2.5 MiB exception.
# Needs readelf (binutils), not file(1). Bash only.
set -eu

usage() {
  cat << 'HELP'
usage: size-budget.sh [--check-dir DIR] [--skip-build] [--target TRIPLE]
                      [--budget-file FILE]

Build (default):
  nice -n 19 cargo build -j 2 --release --workspace
  CARGO_TARGET_DIR is honoured and never overwritten.

Check:
  ELF executables in $CARGO_TARGET_DIR/release
  (or $CARGO_TARGET_DIR/<triple>/release when --target is set).
  If CARGO_TARGET_DIR is unset, <repo>/target is used.
  A bin fails when its size is greater than its budget or when it still
  has a .symtab section (not stripped).
  Budgets: config/release/size-budget.tsv (<binary> <max bytes>, plus a
  `default` row; 2097152 for every binary). Release artifact names
  <binary>-<7..40 hex>[-suffix] (hermes-gateway-124a212,
  gaia-cortex-77df80f-llm) are looked up without the commit tag.
  ELF detection and the strip check use readelf (binutils);
  file(1) is not needed.

--check-dir DIR     do not build; check ELF executables in DIR
--skip-build        do not build; check the cargo release directory
--target TRIPLE     optional musl (or other) target; not required
--budget-file FILE  budget table (default config/release/size-budget.tsv;
                    env SIZE_BUDGET_FILE works too)

exit 0  every bin is within budget and stripped
exit 1  oversize, not stripped, no bins, bad budget file or bad arguments
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
budget_file=${SIZE_BUDGET_FILE:-$root/config/release/size-budget.tsv}

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
    --budget-file)
      [ $# -ge 2 ] || die "--budget-file needs a file"
      budget_file=$2
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

# ---- budget table (read before any build so a bad file fails fast)
[ -f "$budget_file" ] || die "budget file not found: $budget_file"
declare -A BUDGET=()
set -f
lineno=0
while IFS= read -r line || [ -n "$line" ]; do
  lineno=$((lineno + 1))
  line=${line%%#*}
  # shellcheck disable=SC2086
  set -- $line
  [ $# -eq 0 ] && continue
  [ $# -eq 2 ] || die "$budget_file:$lineno: want '<binary> <max bytes>'"
  case "$1" in
    *[!A-Za-z0-9._-]*) die "$budget_file:$lineno: bad binary name: $1" ;;
  esac
  case "$2" in
    '' | *[!0-9]*) die "$budget_file:$lineno: bad byte count: $2" ;;
  esac
  [ -z "${BUDGET[$1]+x}" ] || die "$budget_file:$lineno: duplicate row: $1"
  BUDGET[$1]=$2
done < "$budget_file"
set --
set +f
[ -n "${BUDGET[default]+x}" ] || die "$budget_file: no 'default' row"
LIMIT=${BUDGET[default]}

# Budget key for a file name: the exact name if it has a row, else the name
# without a release commit tag (-<7..40 hex>[-suffix]) if that has a row,
# else `default`.
tag_re='^(.+)-[0-9a-f]{7,40}(-[A-Za-z0-9._]+)?$'
budget_key() {
  local n=$1
  if [ -n "${BUDGET[$n]+x}" ] && [ "$n" != default ]; then
    printf '%s' "$n"
    return
  fi
  if [[ $n =~ $tag_re ]] && [ -n "${BUDGET[${BASH_REMATCH[1]}]+x}" ]; then
    printf '%s' "${BASH_REMATCH[1]}"
    return
  fi
  printf 'default'
}

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
printf 'size-budget: budget_file %s (%s rows)\n' "$budget_file" "${#BUDGET[@]}"
printf 'size-budget: limit_bytes %s\n' "$LIMIT"
printf '%-32s %12s %12s %8s %s %s %s\n' "binary" "bytes" "stripped" "over" "status" "budget" "key"

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
  key=$(budget_key "$name")
  cap=${BUDGET[$key]}
  over=no
  if [ "$size" -gt "$cap" ]; then
    over=yes
  fi
  status=ok
  if [ "$stripped" != yes ] || [ "$over" = yes ]; then
    status=FAIL
    failed=1
  fi
  printf '%-32s %12s %12s %8s %s %s %s\n' "$name" "$size" "$stripped" "$over" "$status" "$cap" "$key"
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
