#!/bin/sh
# SPDX-License-Identifier: MIT
# install/linux.sh - install omnode on a home Linux device (laptop, Pi, NUC).
# Run on the LOCAL device (the one on Slavik's Wi-Fi), never on the VPS.
# POSIX sh. No Python.
set -eu

VERSION="${OMNODE_VERSION:-0.1.0}"
BASE_URL="${OMNODE_RELEASE_URL:-}"   # e.g. https://.../release/omnode-0.1.0  (optional)
PREFIX="${OMNODE_PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/openmind"
ARCH="$(uname -m)"
OS="$(uname -s)"

say() { printf 'omnode-install: %s\n' "$*"; }
die() { printf 'omnode-install: ERROR: %s\n' "$*" >&2; exit 1; }

case "$OS" in
  Linux) ;;
  *) die "this script is for Linux (got $OS). On Android use install/termux.sh" ;;
esac

case "$ARCH" in
  x86_64|amd64)  TARGET="x86_64-unknown-linux-musl"; ARCH_TAG="x86_64" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-musl"; ARCH_TAG="aarch64" ;;
  *) die "unsupported arch: $ARCH (need x86_64 or aarch64)" ;;
esac

mkdir -p "$BIN_DIR" "$CONFIG_DIR"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fetch_or_build() {
  if [ -n "$BASE_URL" ]; then
    say "fetching omnode $VERSION ($ARCH_TAG) from $BASE_URL"
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL "$BASE_URL/omnode-${VERSION}-linux-${ARCH_TAG}.tar.gz" -o "$TMP/om.tgz"
      curl -fsSL "$BASE_URL/omnode-${VERSION}-linux-${ARCH_TAG}.tar.gz.sha256" -o "$TMP/om.sha256"
    elif command -v wget >/dev/null 2>&1; then
      wget -q -O "$TMP/om.tgz" "$BASE_URL/omnode-${VERSION}-linux-${ARCH_TAG}.tar.gz"
      wget -q -O "$TMP/om.sha256" "$BASE_URL/omnode-${VERSION}-linux-${ARCH_TAG}.tar.gz.sha256"
    else
      die "need curl or wget to fetch a release"
    fi
    (cd "$TMP" && sha256sum -c om.sha256)
    tar -xzf "$TMP/om.tgz" -C "$TMP"
    install -m 755 "$TMP/omnode" "$BIN_DIR/omnode"
    return
  fi

  # Build from the source tree this script lives in (or OMNODE_SRC).
  SRC="${OMNODE_SRC:-}"
  if [ -z "$SRC" ]; then
    SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
    SRC="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
  fi
  [ -f "$SRC/Cargo.toml" ] || die "no Cargo.toml at $SRC (set OMNODE_SRC or OMNODE_RELEASE_URL)"
  command -v cargo >/dev/null 2>&1 || die "cargo not found; install Rust from https://rustup.rs"
  say "building omnode from $SRC (nice -n 19, release)"
  (cd "$SRC" && nice -n 19 cargo build --release -j 2)
  install -m 755 "$SRC/target/release/omnode" "$BIN_DIR/omnode"
}

fetch_or_build

# Put ~/.local/bin on PATH for this session if needed.
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) export PATH="$BIN_DIR:$PATH"; say "added $BIN_DIR to PATH for this session" ;;
esac

say "binary: $(command -v omnode) ($(omnode version))"

if [ ! -f "$CONFIG_DIR/node.key" ]; then
  say "creating device identity"
  omnode identity init || say "identity init reported a note (see above); continuing"
else
  say "identity already present: $(omnode identity show | head -1)"
fi

say "running doctor"
omnode doctor | tee "$CONFIG_DIR/doctor-last.json"
say "done. Next: omnode discover --seconds 5   and   omnode pair <enroll-url>"
say "Config dir: $CONFIG_DIR"
