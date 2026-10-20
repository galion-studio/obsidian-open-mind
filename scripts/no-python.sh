#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# DESK-ID: openmind.B16
# Fail if crates/ or deploy/ contains a .py file, or a Python spawn written as
# Command::new("python") or Command::new("python3").
# Scans only those two trees. Does not scan stage/, target/, or galion-light.
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

skip_path() {
  case "$1" in
    */stage|*/stage/*|*/target|*/target/*|*/galion-light|*/galion-light/*)
      return 0
      ;;
    /opt/galion/galion-light|/opt/galion/galion-light/*)
      return 0
      ;;
  esac
  return 1
}

fail=0

while IFS= read -r -d "" path; do
  if skip_path "$path"; then
    continue
  fi
  printf "no-python: py file: %s\n" "$path"
  fail=1
done < <(find "$ROOT/crates" "$ROOT/deploy" \
  \( -type d \( -name stage -o -name target -o -name galion-light \) -prune \) \
  -o \( \( -type f -o -type l \) -name "*.py" -print0 \))

spawn_re="Command::new[[:space:]]*\([[:space:]]*\"python"

while IFS= read -r line; do
  [ -n "$line" ] || continue
  file=${line%%:*}
  if skip_path "$file"; then
    continue
  fi
  printf "no-python: spawn: %s\n" "$line"
  fail=1
done < <(grep -r -n -I -E \
  --exclude-dir=stage --exclude-dir=target --exclude-dir=galion-light \
  "$spawn_re" "$ROOT/crates" "$ROOT/deploy" || true)

if [ "$fail" -ne 0 ]; then
  printf "no-python: fail\n"
  exit 1
fi

printf "no-python: ok\n"
exit 0
