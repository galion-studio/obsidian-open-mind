#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# DESK-ID: openmind.SPDX-SCAN
# List *.rs under crates/ that lack a line containing SPDX-License-Identifier.
# Does not edit files. Does not add headers. Exit 0 even if some are missing.
# B42 is not done. Timestamps are Europe/Zagreb with a numeric offset, never Z.
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

ts=$(TZ=Europe/Zagreb date -Iseconds)
printf "spdx-scan: time %s\n" "$ts"

if [ ! -d "$ROOT/crates" ]; then
  printf "spdx-scan: scanned 0\n"
  printf "spdx-scan: missing 0\n"
  exit 0
fi

missing_list=()
scanned=0

while IFS= read -r -d "" f; do
  scanned=$((scanned + 1))
  if ! grep -q -F -e "SPDX-License-Identifier" -- "$f"; then
    rel=${f#"$ROOT"/}
    missing_list+=("$rel")
  fi
done < <(find "$ROOT/crates" -type f -name "*.rs" -print0 | LC_ALL=C sort -z)

printf "spdx-scan: scanned %s\n" "$scanned"

if [ "${#missing_list[@]}" -gt 0 ]; then
  for rel in "${missing_list[@]}"; do
    printf "%s\n" "$rel"
  done
fi

printf "spdx-scan: missing %s\n" "${#missing_list[@]}"
exit 0
