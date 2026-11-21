#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Install openmind-desktop on Ubuntu 24.04, including the WSL2 distro
# named Galion-Ubuntu. Safe to run again: packages, the .deb, the menu
# file, and the launcher are written to the same paths.
#
# The Linux package is the .deb from scripts/desktop/package-artifact.sh.
# dpkg -i installs that package. This script does not publish or sign.
#
# WSLg lists /usr/share/applications/openmind-desktop.desktop in the
# Windows Start menu for this distro. The entry runs
# /usr/bin/openmind-desktop-wslg (scripts/desktop/wslg-launch.sh).
#
# Sandbox: the desktop binary is not Chromium. --no-sandbox is a usage
# error for it (exit 2). The launcher adds that switch only for a
# Chromium-family browser, and only when the browser sandbox cannot
# start. See --help.
set -euo pipefail

usage() {
  cat << 'EOF'
usage: sudo scripts/install-wsl-ubuntu.sh [--smoke] [--no-browser] [--with-browser]

Install build packages, a rustup stable toolchain when rustc is older
than 1.88, and the openmind-desktop .deb built by
scripts/desktop/package-artifact.sh. Then install the WSLg menu entry.

Run it from a checkout, inside Ubuntu 24.04. A second run exits 0.

--smoke         after install, check --version, xvfb-run --version,
                the menu file, a parsed listen banner, and that the
                Chromium switch is not passed to the desktop binary.
                Does not install a browser.
--no-browser    do not install epiphany-browser
--with-browser  install epiphany-browser even when a browser exists
--help          print this text

On WSL, with neither browser flag, epiphany-browser is installed when
no usable Firefox, Chrome, Chromium, Edge, or Epiphany is on PATH.
Ubuntu 24.04's Firefox package is a snap shim; a snap shim is skipped.

rustc 1.88 or newer is required because crates/galion-common/src/hex.rs
calls as_chunks. The Ubuntu 24.04 rustc package is older. This script
installs upstream rustup stable into the root user's cargo home. It
does not install the Ubuntu rustup package: that package conflicts with
the rustc package.

The listener log is $HOME/.local/state/openmind-desktop/wslg.log.
Browser GPU flags are off unless the environment asks. See the launcher
--help for OPENMIND_WSL_NO_SANDBOX, OPENMIND_WSL_SOFTWARE_GL,
OPENMIND_WSL_WEBKIT_SOFTWARE, and OPENMIND_WSL_X11.

exit 0  installed
exit 2  bad arguments, or a publish or sign flag
exit 1  install or smoke check failed
EOF
}

die() {
  printf 'openmind-wsl: %s\n' "$1" >&2
  exit 1
}

refuse() {
  printf 'openmind-wsl: refusing to publish, upload, or sign (%s)\n' "$1" >&2
  printf 'openmind-wsl: local install only; no release is created\n' >&2
  exit 2
}

# CDPATH= clears CDPATH for this cd. The space is the empty assignment.
# shellcheck disable=SC1007
script_path=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/$(basename -- "$0")
# shellcheck disable=SC1007
repo=$(CDPATH= cd -- "$(dirname -- "$script_path")/.." && pwd)
original_args=("$@")

smoke=0
no_browser=0
with_browser=0

for arg in "$@"; do
  case "$arg" in
    --publish|--upload|--gh-release|--cargo-publish|--sign|--signed|publish|upload|sign)
      refuse "$arg"
      ;;
  esac
done

if [ "${OPENMIND_DESKTOP_PUBLISH:-}" = 1 ]; then
  refuse "OPENMIND_DESKTOP_PUBLISH"
fi
if [ "${OPENMIND_DESKTOP_SIGN:-}" = 1 ]; then
  refuse "OPENMIND_DESKTOP_SIGN"
fi

while [ "$#" -gt 0 ]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --smoke)
      smoke=1
      shift
      ;;
    --no-browser)
      no_browser=1
      shift
      ;;
    --with-browser)
      with_browser=1
      shift
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

if [ "$no_browser" -eq 1 ] && [ "$with_browser" -eq 1 ]; then
  usage >&2
  exit 2
fi

require_ubuntu_2404() {
  local id ver
  # shellcheck disable=SC1091
  id=$(. /etc/os-release && printf '%s' "$ID")
  # shellcheck disable=SC1091
  ver=$(. /etc/os-release && printf '%s' "$VERSION_ID")
  case "$ver" in
    24.04|24.04.*) ;;
    *) die "Ubuntu 24.04 is required (found ${id:-unknown} ${ver:-unknown})" ;;
  esac
  if [ "$id" != ubuntu ]; then
    die "Ubuntu 24.04 is required (found ${id:-unknown} ${ver:-unknown})"
  fi
}

require_ubuntu_2404

if [ "$(id -u)" -ne 0 ]; then
  if ! command -v sudo >/dev/null 2>&1; then
    die "root is required"
  fi
  exec sudo -- "$script_path" "${original_args[@]}"
fi

export DEBIAN_FRONTEND=noninteractive

on_wsl() {
  if [ -n "${WSL_DISTRO_NAME:-}" ]; then
    return 0
  fi
  if [ -e /proc/sys/fs/binfmt_misc/WSLInterop ]; then
    return 0
  fi
  if grep -qi microsoft /proc/version 2>/dev/null; then
    return 0
  fi
  return 1
}

apt_install() {
  local pkgs=(
    build-essential
    ca-certificates
    curl
    pkg-config
    binutils
    dpkg-dev
    desktop-file-utils
    procps
    xdg-utils
    xz-utils
  )
  if [ "$smoke" -eq 1 ]; then
    pkgs+=(xvfb)
  fi
  apt-get update -q
  apt-get install -y -q --no-install-recommends "${pkgs[@]}"
}

rustc_ok() {
  local ver major rest minor
  command -v rustc >/dev/null 2>&1 || return 1
  ver=$(rustc --version | awk '{print $2}')
  major=${ver%%.*}
  rest=${ver#*.}
  minor=${rest%%.*}
  if ! printf '%s' "$major" | grep -Eq '^[0-9]+$'; then
    return 1
  fi
  if ! printf '%s' "$minor" | grep -Eq '^[0-9]+$'; then
    return 1
  fi
  if [ "$major" -gt 1 ]; then
    return 0
  fi
  [ "$major" -eq 1 ] && [ "$minor" -ge 88 ]
}

install_rustup() {
  local arch triple url tmp
  arch=$(uname -m)
  case "$arch" in
    x86_64) triple=x86_64-unknown-linux-gnu ;;
    aarch64|arm64) triple=aarch64-unknown-linux-gnu ;;
    *) die "no rustup-init for $arch" ;;
  esac
  url="https://static.rust-lang.org/rustup/dist/${triple}/rustup-init"
  # argv[0] must be rustup-init. A mktemp name such as tmp.XXXX is read
  # as a rustup proxy and rejected.
  tmp=$(mktemp -d)
  installer=$tmp/rustup-init
  if ! curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$installer"; then
    rm -rf -- "$tmp"
    die "could not download rustup-init"
  fi
  chmod 755 "$installer"
  if ! "$installer" -y --default-toolchain stable --profile minimal --no-modify-path; then
    rm -rf -- "$tmp"
    die "rustup-init failed"
  fi
  rm -rf -- "$tmp"
}

ensure_rust() {
  export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
  export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
  if [ -d "$CARGO_HOME/bin" ]; then
    PATH="$CARGO_HOME/bin:$PATH"
    hash -r
  fi
  if rustc_ok; then
    printf 'openmind-wsl: rustc %s\n' "$(rustc --version)"
    return 0
  fi
  if [ -x "$CARGO_HOME/bin/rustup" ]; then
    "$CARGO_HOME/bin/rustup" toolchain install stable --profile minimal
    "$CARGO_HOME/bin/rustup" default stable
    hash -r
    if rustc_ok; then
      printf 'openmind-wsl: rustc %s\n' "$(rustc --version)"
      return 0
    fi
  fi
  install_rustup
  PATH="$CARGO_HOME/bin:$PATH"
  hash -r
  if ! rustc_ok; then
    die "rustc is still older than 1.88 ($(rustc --version 2>/dev/null || printf missing))"
  fi
  printf 'openmind-wsl: rustc %s\n' "$(rustc --version)"
}

install_deb() {
  local out debs
  out=$repo/target/openmind-desktop-wsl
  "$repo/scripts/desktop/package-artifact.sh" --out "$out"
  "$repo/scripts/desktop/check-package.sh" "$out"
  shopt -s nullglob
  debs=("$out"/openmind-desktop_*.deb)
  shopt -u nullglob
  if [ "${#debs[@]}" -ne 1 ]; then
    die "expected one .deb in $out, found ${#debs[@]}"
  fi
  if ! dpkg -i "${debs[0]}"; then
    apt-get install -y -q -f --no-install-recommends
  fi
  dpkg -s openmind-desktop >/dev/null
  test -x /usr/bin/openmind-desktop
}

install_menu() {
  install -d -m 0755 \
    /usr/share/applications \
    /usr/share/icons/hicolor/64x64/apps
  install -m 0755 "$repo/scripts/desktop/wslg-launch.sh" /usr/bin/openmind-desktop-wslg
  install -m 0644 "$repo/scripts/desktop/openmind-desktop.desktop" \
    /usr/share/applications/openmind-desktop.desktop
  install -m 0644 "$repo/scripts/desktop/openmind-desktop.png" \
    /usr/share/icons/hicolor/64x64/apps/openmind-desktop.png
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications
  fi
}

maybe_browser() {
  local found
  if [ "$smoke" -eq 1 ] || [ "$no_browser" -eq 1 ]; then
    return 0
  fi
  if [ "$with_browser" -eq 1 ]; then
    apt-get install -y -q --no-install-recommends epiphany-browser
    return 0
  fi
  if ! on_wsl; then
    return 0
  fi
  found=$(/usr/bin/openmind-desktop-wslg --print-browser)
  if [ "$found" = none ]; then
    apt-get install -y -q --no-install-recommends epiphany-browser
  fi
}

smoke_check() {
  local line exec_line ff wk cr forced blocked
  line=$(/usr/bin/openmind-desktop --version)
  printf 'openmind-wsl: %s\n' "$line"
  case "$line" in
    openmind-desktop\ [0-9]*) ;;
    *) die "version line was not openmind-desktop <version>" ;;
  esac
  xvfb-run -a /usr/bin/openmind-desktop --version >/dev/null
  desktop-file-validate /usr/share/applications/openmind-desktop.desktop
  exec_line=$(sed -n 's/^Exec=//p' /usr/share/applications/openmind-desktop.desktop)
  if [ "$exec_line" != /usr/bin/openmind-desktop-wslg ]; then
    die "menu Exec is $exec_line"
  fi
  if grep -n -F -- '--no-sandbox' /usr/bin/openmind-desktop-wslg | grep -F 'openmind-desktop'; then
    die "launcher line pairs the sandbox flag with the desktop binary"
  fi
  set +e
  /usr/bin/openmind-desktop --no-sandbox >/dev/null 2>&1
  local status=$?
  set -e
  if [ "$status" -ne 2 ]; then
    die "desktop binary accepted the Chromium switch (exit $status)"
  fi
  ff=$(/usr/bin/openmind-desktop-wslg --browser-flags firefox)
  wk=$(/usr/bin/openmind-desktop-wslg --browser-flags webkit)
  if [ -n "$ff" ] || [ -n "$wk" ]; then
    die "firefox or webkit received browser flags"
  fi
  cr=$(/usr/bin/openmind-desktop-wslg --browser-flags chromium)
  case "$cr" in
    ""|--no-sandbox) ;;
    *) die "unexpected chromium flags: $cr" ;;
  esac
  forced=$(OPENMIND_WSL_NO_SANDBOX=1 /usr/bin/openmind-desktop-wslg --browser-flags chromium)
  blocked=$(OPENMIND_WSL_NO_SANDBOX=0 /usr/bin/openmind-desktop-wslg --browser-flags chromium)
  ff=$(OPENMIND_WSL_NO_SANDBOX=1 /usr/bin/openmind-desktop-wslg --browser-flags firefox)
  if [ "$forced" != --no-sandbox ] || [ -n "$blocked" ] || [ -n "$ff" ]; then
    die "sandbox switch was not limited to Chromium"
  fi
  if grep -E 'https?://|([0-9]{1,3}\.){3}[0-9]{1,3}' \
    /usr/share/applications/openmind-desktop.desktop \
    /usr/bin/openmind-desktop-wslg; then
    die "menu files contain a URL or an IPv4 address"
  fi
  banner_smoke
}

# Start the installed binary, parse the listen banner, then stop it.
# The page URL is whatever the binary prints. It is not stored here.
banner_smoke() {
  local blog pid i url
  blog=$(mktemp)
  /usr/bin/openmind-desktop --listen >"$blog.out" 2>"$blog" &
  pid=$!
  i=0
  url=
  while [ "$i" -lt 50 ]; do
    i=$((i + 1))
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    url=$(/usr/bin/openmind-desktop-wslg --banner-url "$blog" || true)
    if [ -n "$url" ]; then
      break
    fi
    sleep 0.1
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -f -- "$blog" "$blog.out"
  if [ -z "$url" ]; then
    die "listen banner was not parsed"
  fi
  printf 'openmind-wsl: listen banner parsed\n'
}

apt_install
ensure_rust
install_deb
install_menu
maybe_browser

line=$(/usr/bin/openmind-desktop --version)
printf 'openmind-wsl: %s\n' "$line"
case "$line" in
  openmind-desktop\ [0-9]*) ;;
  *) die "version line was not openmind-desktop <version>" ;;
esac
printf 'openmind-wsl: menu /usr/share/applications/openmind-desktop.desktop\n'

if [ "$smoke" -eq 1 ]; then
  smoke_check
fi
