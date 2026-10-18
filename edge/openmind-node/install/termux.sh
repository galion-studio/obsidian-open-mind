#!/bin/sh
# SPDX-License-Identifier: MIT
# install/termux.sh - install omnode inside Termux on Slavik's Android phone.
# Run ON THE PHONE (local Wi-Fi), never on the VPS. POSIX sh. No Python.
set -eu

VERSION="${OMNODE_VERSION:-0.1.0}"
BASE_URL="${OMNODE_RELEASE_URL:-}"
PREFIX="${PREFIX:-/data/data/com.termux/files/usr}"
BIN_DIR="$PREFIX/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/openmind"

say() { printf 'omnode-install: %s\n' "$*"; }
die() { printf 'omnode-install: ERROR: %s\n' "$*" >&2; exit 1; }

# Detect Termux.
if [ -z "${TERMUX_VERSION:-}" ] && ! echo "${PREFIX:-}" | grep -q com.termux; then
  die "does not look like Termux (PREFIX=$PREFIX). Use install/linux.sh on a Linux box."
fi

ARCH="$(uname -m)"
case "$ARCH" in
  aarch64|arm64) ARCH_TAG="aarch64" ;;
  x86_64|amd64)  ARCH_TAG="x86_64" ;;
  *) die "unsupported arch on this phone: $ARCH" ;;
esac

mkdir -p "$BIN_DIR" "$CONFIG_DIR"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Optional helpers for Wi-Fi identity.
if ! command -v termux-wifi-connectioninfo >/dev/null 2>&1; then
  say "tip: pkg install termux-api  (and install the Termux:API Android app from F-Droid)"
  say "      then grant Location permission so SSID/BSSID are readable"
fi

fetch_or_build() {
  if [ -n "$BASE_URL" ]; then
    say "fetching omnode $VERSION ($ARCH_TAG) from $BASE_URL"
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL "$BASE_URL/omnode-${VERSION}-linux-${ARCH_TAG}.tar.gz" -o "$TMP/om.tgz"
      curl -fsSL "$BASE_URL/omnode-${VERSION}-linux-${ARCH_TAG}.tar.gz.sha256" -o "$TMP/om.sha256"
    else
      die "need curl (pkg install curl)"
    fi
    (cd "$TMP" && sha256sum -c om.sha256)
    tar -xzf "$TMP/om.tgz" -C "$TMP"
    install -m 755 "$TMP/omnode" "$BIN_DIR/omnode"
    return
  fi

  SRC="${OMNODE_SRC:-}"
  if [ -z "$SRC" ]; then
    SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
    SRC="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
  fi
  if [ -f "$SRC/Cargo.toml" ]; then
    command -v cargo >/dev/null 2>&1 || die "cargo not found; run: pkg install rust"
    say "building omnode from $SRC (nice -n 19, release)"
    (cd "$SRC" && nice -n 19 cargo build --release -j 2)
    install -m 755 "$SRC/target/release/omnode" "$BIN_DIR/omnode"
    return
  fi
  die "no release URL and no source tree. Set OMNODE_RELEASE_URL or OMNODE_SRC, or copy the aarch64 binary onto the phone."
}

fetch_or_build
say "binary: $(command -v omnode) ($(omnode version))"

if [ ! -f "$CONFIG_DIR/node.key" ]; then
  say "creating device identity"
  omnode identity init || say "identity init reported a note (see above); continuing"
else
  say "identity already present: $(omnode identity show | head -1)"
fi

say "running doctor"
omnode doctor | tee "$CONFIG_DIR/doctor-last.json"
say "done. Next on this phone (on home Wi-Fi):"
say "  omnode discover --seconds 5"
say "  omnode pair <enroll-url-from-config>"
say "Config dir: $CONFIG_DIR"
