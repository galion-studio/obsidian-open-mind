#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
# DESK-ID: openmind.PY-SCAN
# Search crates/ for the word python, case insensitive, in Rust and toml files.
# Print matching paths, one per line. Exit 0 if none, exit 1 if any.
# Does not delete or edit the files it finds.
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd -- "$ROOT" || exit 0

if [ ! -d crates ]; then
  exit 0
fi

hits=0
while IFS= read -r path; do
  [ -n "$path" ] || continue
  printf '%s\n' "$path"
  hits=$((hits + 1))
done < <(grep -r -l -i -I -F \
  --include='*.rs' \
  --include='*.toml' \
  -- python crates | sort -u || true)

if [ "$hits" -gt 0 ]; then
  exit 1
fi
exit 0
