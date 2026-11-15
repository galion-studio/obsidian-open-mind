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
# an offline tree still asks for curve25519-dalek-derive. It asks on
# aarch64 too. An aarch64 fetch never selects the x86_64 simd backend, so
# the crate stays missing and `cargo tree --offline` fails (linux-musl
# aarch64 job on 24f867cb, run 43725539086). One fetch with an empty
# RUSTFLAGS and --target x86_64-unknown-linux-musl downloads it. RUSTFLAGS
# is not exported, so a later build keeps the serial backend.
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
done

# The derive crate is selected only when the x86_64 simd backend is the
# one cargo resolves. Do this even if the caller asked only for aarch64.
if ! rustup target list --installed | grep -qx x86_64-unknown-linux-musl; then
  rustup target add x86_64-unknown-linux-musl
fi
printf 'linux-fetch: RUSTFLAGS= cargo fetch --locked --target x86_64-unknown-linux-musl\n'
RUSTFLAGS= cargo fetch --locked --target x86_64-unknown-linux-musl

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
