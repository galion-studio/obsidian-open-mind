#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Download the locked crate graph so `cargo tree --offline` can resolve
# curve25519-dalek-derive, then run that tree for the desktop, the gateway,
# and hades-gate (the crate that pulls curve25519-dalek).
#
# .cargo/config.toml sets curve25519_dalek_backend=serial. That cfg drops
# curve25519-dalek-derive from an x86_64 build. `cargo fetch` honours the
# cfg and skips the crate. `cargo tree` does not apply those rustflags, so
# an offline tree still asks for curve25519-dalek-derive and fails if the
# crate is not in the registry cache. A second fetch with an empty RUSTFLAGS
# replaces the config list for that one command and downloads the crate.
# RUSTFLAGS is not exported, so a later build keeps the serial backend.
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$root"

if [ "$#" -eq 0 ]; then
  set -- x86_64-unknown-linux-musl aarch64-unknown-linux-musl
fi

for target in "$@"; do
  case "$target" in
    x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
    *)
      printf 'linux-fetch: unknown target %s\n' "$target" >&2
      exit 2
      ;;
  esac
done

for target in "$@"; do
  printf 'linux-fetch: cargo fetch --locked --target %s\n' "$target"
  cargo fetch --locked --target "$target"
  printf 'linux-fetch: RUSTFLAGS= cargo fetch --locked --target %s\n' "$target"
  RUSTFLAGS= cargo fetch --locked --target "$target"
done

pkgs=(openmind-desktop hermes-gateway hades-gate)
for target in "$@"; do
  for pkg in "${pkgs[@]}"; do
    out=$(mktemp)
    cargo tree --offline --locked -p "$pkg" --target "$target" \
      -e normal,build --prefix none -f '{p}' >"$out"
    derive=$(grep -c 'curve25519-dalek-derive' "$out" || true)
    printf 'linux-fetch: cargo tree --offline -p %s --target %s ok derive_lines=%s\n' \
      "$pkg" "$target" "$derive"
    rm -f "$out"
  done
done
