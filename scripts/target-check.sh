#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
# DESK-ID: openmind.UCPU-1
# Print rustc -vV, then whether host rustc knows four target names.
# KNOWN: rustc --print cfg --target <name> exits 0 (built-in spec only).
# MISSING: rustc does not know the name, or rustc is absent.
# No cargo build. No rustup target add. No download.
# Exit 0 even when a target is MISSING.
# A full cross build is not done and is not claimed.
set -u

export PATH="/root/.cargo/bin:${PATH:-/usr/bin:/bin}"

targets=(
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-musl
  aarch64-linux-android
  armv7-unknown-linux-gnueabihf
)

if ! command -v rustc >/dev/null 2>&1; then
  printf "rustc MISSING\n"
  for t in "${targets[@]}"; do
    printf "%s MISSING\n" "$t"
  done
  exit 0
fi

printf "=== rustc -vV ===\n"
if ! rustc -vV; then
  printf "rustc -vV MISSING\n"
  for t in "${targets[@]}"; do
    printf "%s MISSING\n" "$t"
  done
  exit 0
fi

printf "=== target names ===\n"
for t in "${targets[@]}"; do
  if rustc --print cfg --target "$t" >/dev/null 2>&1; then
    printf "%s KNOWN\n" "$t"
  else
    printf "%s MISSING\n" "$t"
  fi
done

exit 0
