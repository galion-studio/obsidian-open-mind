#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Slavik / Galion Studio
#
# omnode installer: one file, POSIX sh, no Rust or compiler needed.
# Works on Android (Termux), Linux laptops and Raspberry Pi (x86_64, aarch64).
#
#   sh install.sh                 # binaries next to this script (dist dir)
#   OMNODE_DIST=/path/to/dist sh install.sh
#   OMNODE_BASE_URL=https://.../download/v0.1.0 sh install.sh   # once a release exists
#
# What it does, in order, and nothing else:
#   1. detect CPU arch and Termux
#   2. get omnode-<arch>-linux-musl + SHA256SUMS (local dir, or OMNODE_BASE_URL)
#   3. verify the sha256 checksum (refuses to install on mismatch)
#   4. install to $PREFIX/bin (Termux) or ~/.local/bin (mode 755)
#   5. run `omnode identity init` (creates the device key once; never overwrites)
#   6. run `omnode discover --dry-run` (opens NO sockets, only prints the plan)
#   7. run `omnode sandbox check` and print a summary
# It never uses sudo, never edits shell rc files, never sends anything.
set -eu

say() { printf '%s\n' "omnode-install: $*"; }
die() { printf '%s\n' "omnode-install: ERROR: $*" >&2; exit 1; }

# ---- 1. platform ----
case "$(uname -s 2>/dev/null)" in
  Linux) ;;
  *) die "only Linux/Android is supported (got $(uname -s 2>/dev/null))" ;;
esac
m=$(uname -m 2>/dev/null || echo unknown)
case "$m" in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64|armv8*) arch=aarch64 ;;
  *) die "unsupported CPU '$m' (prebuilt: x86_64, aarch64). Build from source: cargo build --release" ;;
esac
termux=no
if [ -n "${TERMUX_VERSION:-}" ]; then
  termux=yes
else
  case "${PREFIX:-}" in *com.termux*) termux=yes ;; esac
fi
if [ "$termux" = yes ] && [ -z "${PREFIX:-}" ]; then
  PREFIX=/data/data/com.termux/files/usr
fi
if [ "$termux" = yes ]; then
  bindir="$PREFIX/bin"
else
  bindir="${OMNODE_BINDIR:-$HOME/.local/bin}"
fi
name="omnode-$arch-linux-musl"
say "platform: $(uname -s) $m -> $arch, termux=$termux, install dir: $bindir"

# ---- 2. fetch or locate ----
here=$(CDPATH= cd -- "$(dirname -- "$0")" 2>/dev/null && pwd || pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/omnode-install.XXXXXX") || die "mktemp failed"
trap 'rm -rf "$tmp"' EXIT INT TERM
src=""
for d in "${OMNODE_DIST:-}" "$here" "$here/dist" "$here/../dist"; do
  if [ -n "$d" ] && [ -f "$d/$name" ] && [ -f "$d/SHA256SUMS" ]; then src=$d; break; fi
done
if [ -n "$src" ]; then
  cp "$src/$name" "$src/SHA256SUMS" "$tmp/"
  say "using local files from $src"
elif [ -n "${OMNODE_BASE_URL:-}" ]; then
  case "$OMNODE_BASE_URL" in https://*) ;; *) die "OMNODE_BASE_URL must be https://" ;; esac
  if command -v curl >/dev/null 2>&1; then
    get() { curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"; }
  elif command -v wget >/dev/null 2>&1; then
    get() { wget -q -O "$2" "$1"; }
  else
    die "need curl or wget to download (Termux: pkg install curl)"
  fi
  get "$OMNODE_BASE_URL/$name" "$tmp/$name" || die "download failed: $OMNODE_BASE_URL/$name"
  get "$OMNODE_BASE_URL/SHA256SUMS" "$tmp/SHA256SUMS" || die "download failed: SHA256SUMS"
  say "downloaded from $OMNODE_BASE_URL"
else
  die "no $name + SHA256SUMS next to this script and OMNODE_BASE_URL unset"
fi

# ---- 3. verify ----
want=$(awk -v n="$name" '$2==n || $2=="*"n {print $1; exit}' "$tmp/SHA256SUMS")
[ -n "$want" ] || die "SHA256SUMS has no entry for $name"
if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$tmp/$name" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  got=$(shasum -a 256 "$tmp/$name" | awk '{print $1}')
elif command -v openssl >/dev/null 2>&1; then
  got=$(openssl dgst -sha256 "$tmp/$name" | awk '{print $NF}')
else
  die "no sha256 tool (sha256sum/shasum/openssl) to verify the binary"
fi
[ "$got" = "$want" ] || die "checksum MISMATCH for $name (want $want, got $got); not installing"
say "sha256 OK ($got)"

# ---- 4. install ----
mkdir -p "$bindir"
chmod 755 "$tmp/$name"
"$tmp/$name" version >/dev/null 2>&1 || die "binary does not run on this device"
if [ -f "$bindir/omnode" ]; then
  cp -p "$bindir/omnode" "$bindir/omnode.bak-$(TZ=Europe/Zagreb date +%Y%m%d-%H%M%S%z 2>/dev/null || date +%Y%m%d-%H%M%S%z)"
  say "previous omnode backed up next to the new one"
fi
mv "$tmp/$name" "$bindir/omnode.new" && mv "$bindir/omnode.new" "$bindir/omnode"
om="$bindir/omnode"
say "installed $om ($("$om" version))"
case ":$PATH:" in *":$bindir:"*) onpath=yes ;; *) onpath=no ;; esac

# ---- 5-7. first run ----
idout=$("$om" identity init 2>&1) || idout=$("$om" identity show 2>&1) || idout="identity: failed"
printf '%s\n' "$idout" | sed 's/^/  /'
"$om" discover --dry-run >"$tmp/plan.json" 2>&1 && plan=ok || plan=failed
"$om" sandbox check >"$tmp/sbx.json" 2>&1 && sbx=ok || sbx=failed
layers=$(grep -E '^ +"(rlimits|no_new_privs|user_namespace|net_namespace|pid_namespace|read_only_root|home_hidden|caps_dropped)": (true|false)' "$tmp/sbx.json" 2>/dev/null | tr -d ' ",' | tr '\n' ' ')
net=$(sed -n 's/^ *"network": "\(.*\)",*$/\1/p' "$tmp/sbx.json" | head -n 1)

echo
echo "================ omnode install summary ================"
echo "binary     : $om"
echo "arch       : $arch ($m), termux: $termux"
echo "checksum   : verified sha256 $got"
echo "identity   : $(printf '%s' "$idout" | sed -n 's/^fingerprint *//p' | head -n 1)"
echo "network    : $(printf '%s' "$idout" | sed -n 's/^network *//p' | head -n 1)"
echo "discover   : dry-run $plan (no sockets opened; real scan: omnode discover)"
echo "sandbox    : check $sbx; network: ${net:-unknown}"
echo "             $layers"
if [ "$onpath" = no ]; then
  echo "PATH       : $bindir is not on PATH; run $om directly or add it to PATH"
fi
echo "next       : omnode discover --seconds 5 ; omnode pair https://<hermes-host>/openmind/enroll"
echo "nothing was sent anywhere."
echo "========================================================="
