#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Release-build openmind-desktop and hermes-gateway for one musl triple.
# Fetches the offline cargo tree first (curve25519-dalek-derive), then
# checks the 2 MiB size cap and prints the measured byte counts.
#
# hermes-gateway links bundled SQLite, so the C objects are compiled with
# the same zig cc wrappers as scripts/ucpu-gate.sh. The linker is rust-lld
# from .cargo/config.toml. This script does not publish or upload.
set -euo pipefail

usage() {
  printf 'usage: scripts/linux/build-musl.sh x86_64-unknown-linux-musl|aarch64-unknown-linux-musl\n'
}

if [ "$#" -ne 1 ] || [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
  usage
  [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]
  exit $?
fi

target=$1
case "$target" in
  x86_64-unknown-linux-musl) zig_target=x86_64-linux-musl ;;
  aarch64-unknown-linux-musl) zig_target=aarch64-linux-musl ;;
  *)
    usage >&2
    exit 2
    ;;
esac

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$root"

if ! rustup target list --installed | grep -qx "$target"; then
  rustup target add "$target"
fi

zig_bin=${ZIG:-$("$root/scripts/install-zig.sh" "$root/target/linux-zig")}
wrap=$root/target/linux-zig-wrap
mkdir -p "$wrap"
cat > "$wrap/cc-$target" << WRAP
#!/bin/sh
for a do shift; case "\$a" in --target=*) continue ;; esac; set -- "\$@" "\$a"; done
exec "$zig_bin" cc -target $zig_target "\$@"
WRAP
chmod +x "$wrap/cc-$target"
printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$zig_bin" > "$wrap/ar"
chmod +x "$wrap/ar"
# cc-rs reads CC_<triple> / AR_<triple> with hyphens turned into underscores
# (same names as scripts/ucpu-gate.sh). Not the uppercase CARGO_TARGET_* form.
cc_var=CC_$(printf '%s' "$target" | tr '-' '_')
ar_var=AR_$(printf '%s' "$target" | tr '-' '_')
export "$cc_var=$wrap/cc-$target"
export "$ar_var=$wrap/ar"
printf 'linux-musl: zig %s cc %s\n' "$("$zig_bin" version)" "$wrap/cc-$target"

"$root/scripts/linux/fetch-offline-tree.sh" "$target"

jobs=${LINUX_JOBS:-2}
nice_level=${LINUX_NICE:-19}
printf 'linux-musl: cargo build --release --locked -p openmind-desktop -p hermes-gateway --target %s\n' "$target"
nice -n "$nice_level" cargo build -j "$jobs" --release --locked \
  -p openmind-desktop -p hermes-gateway --target "$target"

rel=$root/target/$target/release
"$root/scripts/size-budget.sh" --check-dir "$rel"

cap=2097152
for bin in openmind-desktop hermes-gateway; do
  path=$rel/$bin
  [ -f "$path" ] || {
    printf 'linux-musl: missing %s\n' "$path" >&2
    exit 1
  }
  bytes=$(stat -c '%s' -- "$path")
  machine=$(readelf -hW -- "$path" | sed -n 's/^ *Machine: *//p')
  interp=no
  if readelf -lW -- "$path" | grep -q 'INTERP'; then
    interp=yes
  fi
  needed=no
  if readelf -dW -- "$path" | grep -q '(NEEDED)'; then
    needed=yes
  fi
  printf 'linux-musl: binary=%s target=%s bytes=%s cap=%s machine=%s interp=%s needed=%s\n' \
    "$bin" "$target" "$bytes" "$cap" "$machine" "$interp" "$needed"
  if [ "$bytes" -gt "$cap" ]; then
    printf 'linux-musl: %s is over the 2 MiB cap\n' "$bin" >&2
    exit 1
  fi
  if [ "$interp" = yes ] || [ "$needed" = yes ]; then
    printf 'linux-musl: %s is not a static musl binary\n' "$bin" >&2
    exit 1
  fi
done
