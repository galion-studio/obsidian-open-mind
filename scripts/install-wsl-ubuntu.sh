#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# W0 entry for the Obsidian desktop step. One script:
#   sudo bash scripts/install-wsl-ubuntu.sh
# Run it as root inside Galion-Ubuntu, from a checkout of this repository.
# Safe to run again: packages, the .deb, the menu file, and the launcher
# are written to the same paths.
#
# Cargo, rustup, the package directory, and the listener state stay on the
# distro filesystem (/opt/galion, /var/cache/galion, /var/lib/galion). That
# filesystem is the distro disk. A path on /mnt/<letter> is refused.
#
# After install, the script starts
#   openmind-desktop --listen 127.0.0.1:18181
# and adds --stack off when nothing accepts a connection on
# 127.0.0.1:8091. The page is http://127.0.0.1:18181/ .
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
usage: sudo bash scripts/install-wsl-ubuntu.sh [--smoke] [--no-browser] [--with-browser]

W0 calls that command. It is the only desktop entry. Install build
packages, a rustup stable toolchain when rustc is older than 1.88, and
the openmind-desktop .deb built by scripts/desktop/package-artifact.sh.
Then install the WSLg menu entry.

Run it as root inside Ubuntu 24.04, from a checkout of this repository.
A second run exits 0.

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
installs upstream rustup stable under /opt/galion. It does not install
the Ubuntu rustup package: that package conflicts with the rustc package.

Cargo home, the rustup home, and the cargo target directory are
/opt/galion/cargo, /opt/galion/rustup, and /opt/galion/cargo/target.
The .deb directory and temp files are under /var/cache/galion/tmp.
Those paths are on the distro filesystem. The script refuses the
checkout or any of those directories when it is under /mnt/<letter>.

The script then starts the installed binary and leaves it running:
  openmind-desktop --listen 127.0.0.1:18181
Plain --listen dials 127.0.0.1:8091 (DEFAULT_STACK in
crates/openmind-desktop/src/files.rs). When that address accepts no
connection, the script adds --stack off. The page is
http://127.0.0.1:18181/
A second run leaves a matching listener in place.

The listener log is /var/lib/galion/openmind-desktop/listener.log .
The menu launcher's own log, if it has to start the binary, is
$HOME/.local/state/openmind-desktop/wslg.log when that directory is on
the distro filesystem. A home directory on /mnt/<letter> uses
/var/tmp/openmind-desktop-$USER instead.
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

# 0 when the path is on a Windows drive mounted into the distro.
on_windows_mount() {
  local path=$1 resolved
  resolved=$(readlink -f -- "$path" 2>/dev/null || printf '%s' "$path")
  case "$resolved" in
    /mnt/[a-zA-Z]|/mnt/[a-zA-Z]/*) return 0 ;;
  esac
  return 1
}

assert_distro_disk() {
  local path=$1 label=$2
  if on_windows_mount "$path"; then
    die "$label is on a Windows drive mount; keep it on the distro disk"
  fi
}

# Cargo, rustup, the package directory, and temp files stay on the distro
# filesystem. Inside Galion-Ubuntu that filesystem is the distro disk.
pin_distro_dirs() {
  export CARGO_HOME=/opt/galion/cargo
  export RUSTUP_HOME=/opt/galion/rustup
  export CARGO_TARGET_DIR=/opt/galion/cargo/target
  install -d -m 0755 \
    /opt/galion/cargo/target \
    /opt/galion/rustup \
    /var/cache/galion/tmp
  # shellcheck disable=SC1007
  TMPDIR=$(CDPATH= cd -- /var/cache/galion/tmp && pwd)
  export TMPDIR
  assert_distro_disk "$repo" "checkout"
  assert_distro_disk "$CARGO_HOME" "cargo home"
  assert_distro_disk "$RUSTUP_HOME" "rustup home"
  assert_distro_disk "$CARGO_TARGET_DIR" "cargo target"
  assert_distro_disk "$TMPDIR" "temp directory"
  install -d -m 0755 /var/lib/galion/openmind-desktop
  assert_distro_disk /var/lib/galion/openmind-desktop "listener state"
}

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
  export CARGO_HOME="${CARGO_HOME:-/opt/galion/cargo}"
  export RUSTUP_HOME="${RUSTUP_HOME:-/opt/galion/rustup}"
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
  out=$TMPDIR/openmind-desktop-wsl
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
  for p in "$CARGO_HOME" "$RUSTUP_HOME" "$CARGO_TARGET_DIR" "$TMPDIR"; do
    if on_windows_mount "$p"; then
      die "build path left the distro disk: $p"
    fi
  done
  test -d "$CARGO_TARGET_DIR"
  banner_smoke
}

# Page served by --listen on the default bind. DEFAULT_BIND in
# crates/openmind-desktop/src/lib.rs. DEFAULT_STACK is the Galion One intake.
PAGE_URL='http://127.0.0.1:18181/'
LISTEN_ADDR='127.0.0.1:18181'
STACK_ADDR='127.0.0.1:8091'
LISTENER_DIR=/var/lib/galion/openmind-desktop
LISTENER_LOG=$LISTENER_DIR/listener.log
LISTENER_PID=$LISTENER_DIR/listener.pid
listener_args=()

# 0 when a TCP accept happens on host:port. A refusal is not a match.
port_accepts() {
  local addr=$1 host port
  host=${addr%:*}
  port=${addr##*:}
  timeout 1 bash -c "echo >/dev/tcp/${host}/${port}" >/dev/null 2>&1
}

build_listener_args() {
  listener_args=(/usr/bin/openmind-desktop --listen "$LISTEN_ADDR")
  if ! port_accepts "$STACK_ADDR"; then
    listener_args+=(--stack off)
  fi
}

listener_pid() {
  local pid
  [ -f "$LISTENER_PID" ] || return 1
  pid=$(tr -d '[:space:]' < "$LISTENER_PID")
  [ -n "$pid" ] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  printf '%s' "$pid"
}

listener_command() {
  local pid
  pid=$(listener_pid) || return 1
  ps -p "$pid" -o args= 2>/dev/null | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

# Leave the desktop listening. A second run does not start a duplicate
# when the live command already matches.
start_listener() {
  local pid i url got
  install -d -m 0755 "$LISTENER_DIR"
  assert_distro_disk "$LISTENER_DIR" "listener state"
  build_listener_args
  got=$(listener_command || true)
  if [ "$got" = "${listener_args[*]}" ]; then
    printf 'openmind-wsl: already listening at %s\n' "$PAGE_URL"
    return 0
  fi
  if pid=$(listener_pid); then
    kill "$pid" 2>/dev/null || true
    i=0
    while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 50 ]; do
      i=$((i + 1))
      sleep 0.1
    done
  fi
  : > "$LISTENER_LOG"
  # The binary's own arguments. --no-sandbox is not one of them.
  nohup "${listener_args[@]}" >>"$LISTENER_LOG" 2>&1 </dev/null &
  pid=$!
  disown "$pid" 2>/dev/null || true
  printf '%s\n' "$pid" > "$LISTENER_PID"
  i=0
  url=
  while [ "$i" -lt 50 ]; do
    i=$((i + 1))
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    url=$(/usr/bin/openmind-desktop-wslg --banner-url "$LISTENER_LOG" || true)
    if [ -n "$url" ]; then
      break
    fi
    sleep 0.1
  done
  if [ "$url" != "$PAGE_URL" ]; then
    die "listener did not serve ${PAGE_URL}"
  fi
  printf 'openmind-wsl: listening at %s\n' "$PAGE_URL"
  if ! port_accepts "$STACK_ADDR"; then
    printf 'openmind-wsl: stack intake is closed; started with --stack off\n'
  fi
}

# The live listener's banner is the documented page URL.
banner_smoke() {
  local url got
  url=$(/usr/bin/openmind-desktop-wslg --banner-url "$LISTENER_LOG" || true)
  if [ "$url" != "$PAGE_URL" ]; then
    die "listen banner was not ${PAGE_URL}"
  fi
  build_listener_args
  got=$(listener_command || true)
  if [ "$got" != "${listener_args[*]}" ]; then
    die "listener command is not ${listener_args[*]}"
  fi
  printf 'openmind-wsl: listen banner parsed\n'
}

apt_install
pin_distro_dirs
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
start_listener

if [ "$smoke" -eq 1 ]; then
  smoke_check
fi
