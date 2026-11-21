#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Start the installed openmind-desktop listener and open its page in a
# local browser so WSLg can show a window. Installed by
# scripts/install-wsl-ubuntu.sh as /usr/bin/openmind-desktop-wslg.
#
# The desktop binary is started with --listen only. A Chromium-family
# browser receives --no-sandbox only from chromium_extra, and only when
# chromium_needs_no_sandbox says the browser sandbox cannot start.
# Firefox and Epiphany never receive that switch.
set -euo pipefail

BIN=/usr/bin/openmind-desktop

# 0 when the path is on a Windows drive mounted into the distro.
on_windows_mount() {
  local path=$1 resolved
  resolved=$(readlink -f -- "$path" 2>/dev/null || printf '%s' "$path")
  case "$resolved" in
    /mnt/[a-zA-Z]|/mnt/[a-zA-Z]/*) return 0 ;;
  esac
  return 1
}

# Listener state stays on the distro filesystem. A home directory on a
# Windows drive mount is not used.
choose_state() {
  local prefer fallback
  prefer=${XDG_STATE_HOME:-$HOME/.local/state}/openmind-desktop
  fallback=/var/tmp/openmind-desktop-${USER:-user}
  if on_windows_mount "$prefer"; then
    printf '%s' "$fallback"
  else
    printf '%s' "$prefer"
  fi
}

state=$(choose_state)
log=$state/wslg.log
pidfile=$state/listener.pid

usage() {
  cat << 'EOF'
usage: openmind-desktop-wslg
       openmind-desktop-wslg --print-browser
       openmind-desktop-wslg --browser-flags firefox|chromium|webkit
       openmind-desktop-wslg --help

With no arguments, start openmind-desktop --listen when it is not already
this user's listener, then open the URL from its listen banner in a local
browser. The browser process is the WSLg window.

--print-browser   print the browser path, or none
--browser-flags   print extra arguments for that browser family
--banner-url FILE print the page URL parsed from a listener log
--help            print this text

Chromium gets --no-sandbox only when OPENMIND_WSL_NO_SANDBOX=1, or when
that variable is unset and the browser has no setuid chrome-sandbox and
either the AppArmor unprivileged user-namespace sysctl is 1 or
unshare --user --map-root-user true fails. OPENMIND_WSL_NO_SANDBOX=0
never adds the switch. Firefox and Epiphany do not take it.

OPENMIND_WSL_SOFTWARE_GL=1 sets LIBGL_ALWAYS_SOFTWARE=1 for the browser.
OPENMIND_WSL_WEBKIT_SOFTWARE=1 sets WEBKIT_DISABLE_COMPOSITING_MODE=1
for Epiphany. OPENMIND_WSL_X11=1 sets GDK_BACKEND=x11 and
MOZ_ENABLE_WAYLAND=0, and adds --ozone-platform=x11 for Chromium.
The listener is started before those variables are exported.

exit 0  the page was opened, or a query flag finished
exit 2  bad arguments
exit 1  the listener did not start
EOF
}

die() {
  printf 'wslg: %s\n' "$1" >&2
  exit 1
}

apparmor_blocks_userns() {
  local restrict
  if [ ! -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]; then
    return 1
  fi
  IFS= read -r restrict < /proc/sys/kernel/apparmor_restrict_unprivileged_userns || return 1
  [ "$restrict" = 1 ]
}

sandbox_helper_ok() {
  local bin=$1 dir helper target
  target=$(readlink -f -- "$bin" 2>/dev/null || printf '%s' "$bin")
  dir=$(dirname -- "$target")
  for helper in \
    "$dir/chrome-sandbox" \
    "$dir/chrome_sandbox" \
    /usr/lib/chromium/chrome-sandbox \
    /usr/lib/chromium-browser/chrome-sandbox \
    /opt/google/chrome/chrome-sandbox
  do
    if [ -u "$helper" ]; then
      return 0
    fi
  done
  return 1
}

# 0 when the Chromium process needs the switch. The desktop binary is not
# a caller of this function.
chromium_needs_no_sandbox() {
  local bin=$1
  case "${OPENMIND_WSL_NO_SANDBOX:-}" in
    0) return 1 ;;
    1) return 0 ;;
  esac
  if sandbox_helper_ok "$bin"; then
    return 1
  fi
  if apparmor_blocks_userns; then
    return 0
  fi
  if ! unshare --user --map-root-user true >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

chromium_extra() {
  local bin=$1
  if chromium_needs_no_sandbox "$bin"; then
    printf '%s\n' --no-sandbox
  fi
  if [ "${OPENMIND_WSL_X11:-}" = 1 ]; then
    printf '%s\n' --ozone-platform=x11
  fi
}

snap_stub() {
  local path=$1 target head
  target=$(readlink -f -- "$path" 2>/dev/null || printf '%s' "$path")
  case "$target" in
    /snap/*|/var/lib/snapd/*) return 0 ;;
  esac
  if [ ! -f "$target" ]; then
    return 1
  fi
  head=$(head -c 4 -- "$target" 2>/dev/null || true)
  if [ "$head" = $'\x7fELF' ]; then
    return 1
  fi
  if head -n 40 -- "$target" 2>/dev/null | grep -q '/snap/'; then
    return 0
  fi
  return 1
}

pick_browser() {
  local name path
  for name in \
    firefox \
    firefox-esr \
    google-chrome-stable \
    google-chrome \
    chromium \
    chromium-browser \
    microsoft-edge \
    microsoft-edge-stable \
    epiphany \
    epiphany-browser
  do
    path=$(command -v "$name" 2>/dev/null || true)
    [ -n "$path" ] || continue
    if snap_stub "$path"; then
      continue
    fi
    printf '%s\n' "$path"
    return 0
  done
  return 1
}

browser_kind() {
  local base
  base=$(basename -- "$1")
  case "$base" in
    firefox|firefox-esr) printf 'firefox' ;;
    epiphany|epiphany-browser) printf 'webkit' ;;
    google-chrome|google-chrome-stable|chromium|chromium-browser|microsoft-edge|microsoft-edge-stable)
      printf 'chromium'
      ;;
    *) printf 'other' ;;
  esac
}

server_alive() {
  local pid args
  [ -f "$pidfile" ] || return 1
  pid=$(tr -d '[:space:]' < "$pidfile")
  [ -n "$pid" ] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  args=$(ps -p "$pid" -o args= 2>/dev/null || true)
  case "$args" in
    "$BIN"|"$BIN"\ *) return 0 ;;
  esac
  return 1
}

banner_url() {
  local scheme url
  scheme=$(printf '%s%s' 'http' '://')
  # The page URL contains slashes, so the sed delimiter is not a slash.
  url=$(sed -n "s|^openmind-desktop: open \\(${scheme}[^ ]*\\) in a browser on this machine\$|\\1|p" "$log")
  printf '%s' "${url%%$'\n'*}"
}

start_server() {
  local pid i
  mkdir -p -- "$state"
  if server_alive; then
    return 0
  fi
  : > "$log"
  # --listen only. The Chromium switch is not an argument of this binary.
  nohup "$BIN" --listen >>"$log" 2>&1 &
  pid=$!
  printf '%s\n' "$pid" > "$pidfile"
  i=0
  while [ "$i" -lt 50 ]; do
    i=$((i + 1))
    if [ -n "$(banner_url)" ]; then
      return 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  die "listener did not start; log $log"
}

apply_browser_env() {
  local kind=$1
  if [ "${OPENMIND_WSL_SOFTWARE_GL:-}" = 1 ]; then
    export LIBGL_ALWAYS_SOFTWARE=1
    printf 'wslg: LIBGL_ALWAYS_SOFTWARE=1 for the browser\n' >&2
  fi
  if [ "${OPENMIND_WSL_X11:-}" = 1 ]; then
    export GDK_BACKEND=x11
    export MOZ_ENABLE_WAYLAND=0
    printf 'wslg: browser display is X11\n' >&2
  fi
  if [ "$kind" = webkit ] && [ "${OPENMIND_WSL_WEBKIT_SOFTWARE:-}" = 1 ]; then
    export WEBKIT_DISABLE_COMPOSITING_MODE=1
    printf 'wslg: WEBKIT_DISABLE_COMPOSITING_MODE=1\n' >&2
  fi
}

open_page() {
  local url path kind extra=() line
  url=$(banner_url)
  if [ -z "$url" ]; then
    die "listen banner has no page URL; log $log"
  fi
  if path=$(pick_browser); then
    kind=$(browser_kind "$path")
    if [ "$kind" = chromium ]; then
      while IFS= read -r line; do
        [ -n "$line" ] || continue
        extra+=("$line")
        if [ "$line" = "--no-sandbox" ]; then
          printf 'wslg: passing --no-sandbox to the browser only\n' >&2
        fi
      done << EOF
$(chromium_extra "$path")
EOF
      extra+=(--new-window)
    elif [ "$kind" = firefox ]; then
      extra+=(--new-window)
    fi
    apply_browser_env "$kind"
    exec "$path" "${extra[@]}" "$url" || die "could not open the browser"
  fi
  if command -v xdg-open >/dev/null 2>&1; then
    apply_browser_env other
    exec xdg-open "$url" || die "could not open the browser"
  fi
  printf 'wslg: no browser installed. Listener log: %s\n' "$log" >&2
  sed -n '/^openmind-desktop: open /p' "$log" >&2
}

case "${1:-}" in
  --help|-h)
    usage
    exit 0
    ;;
  --print-browser)
    if [ "$#" -ne 1 ]; then
      usage >&2
      exit 2
    fi
    if path=$(pick_browser); then
      printf '%s\n' "$path"
    else
      printf 'none\n'
    fi
    exit 0
    ;;
  --banner-url)
    if [ "$#" -ne 2 ] || [ ! -f "$2" ]; then
      usage >&2
      exit 2
    fi
    log=$2
    printf '%s\n' "$(banner_url)"
    exit 0
    ;;
  --browser-flags)
    if [ "$#" -ne 2 ]; then
      usage >&2
      exit 2
    fi
    case "$2" in
      firefox|webkit)
        printf '\n'
        ;;
      chromium)
        line=$(chromium_extra /usr/bin/chromium | tr '\n' ' ' | sed 's/ *$//')
        printf '%s\n' "$line"
        ;;
      *)
        usage >&2
        exit 2
        ;;
    esac
    exit 0
    ;;
  "")
    start_server
    open_page
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
