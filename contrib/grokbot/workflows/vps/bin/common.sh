#!/usr/bin/env bash
# Shared helpers for Slavik's VPS scripts. Sourced, not executed.
# Loads config.env, validates placeholders, and provides vps_ssh / vps_run_remote.
set -o pipefail

VPS_BIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VPS_HOME_DIR="$(dirname "$VPS_BIN_DIR")"
VPS_CONFIG="${VPS_CONFIG:-$VPS_HOME_DIR/config.env}"

die()  { echo "ERROR: $*" >&2; exit 1; }
warn() { echo "WARN: $*" >&2; }
info() { echo "==> $*" >&2; }
ts()   { date +%Y%m%d-%H%M%S; }

[ -f "$VPS_CONFIG" ] || die "config not found: $VPS_CONFIG"
# shellcheck source=/dev/null
. "$VPS_CONFIG"

: "${VPS_PORT:=22}" "${VPS_ALIAS:=slavik-vps}" "${VPS_KEY:=$HOME/.ssh/id_ed25519_vps}"
: "${VPS_CONNECT_TIMEOUT:=10}" "${VPS_DATA_DIR:=$VPS_HOME_DIR/data}"
: "${VPS_STACK_MAP:=$VPS_HOME_DIR/stack-map.md}" "${VPS_SERVICES_LIST:=$VPS_HOME_DIR/services.list}"
: "${VPS_REMOTE_BACKUP_DIR:=~/vps-backups}" "${VPS_ALERT_CMD:=}"

require_target() {
  case "${VPS_HOST:-}" in ""|__FILL_ME*) die "VPS_HOST not set. Edit $VPS_CONFIG or run set-target.sh <host> <user>";; esac
  case "${VPS_USER:-}" in ""|__FILL_ME*) die "VPS_USER not set. Edit $VPS_CONFIG or run set-target.sh <host> <user>";; esac
  [ -f "$VPS_KEY" ] || die "SSH key missing: $VPS_KEY (do not generate a new one; ask the user)"
}

# Command-line -o options override ~/.ssh/config, so config.env alone is enough for scripts.
VPS_SSH_OPTS=()
build_ssh_opts() {
  VPS_SSH_OPTS=(
    -o "HostName=$VPS_HOST" -o "User=$VPS_USER" -o "Port=$VPS_PORT"
    -i "$VPS_KEY" -o IdentitiesOnly=yes
    -o BatchMode=yes -o "ConnectTimeout=$VPS_CONNECT_TIMEOUT"
    -o ServerAliveInterval=15 -o ServerAliveCountMax=3
    -o StrictHostKeyChecking=accept-new
  )
}

# vps_ssh [ssh args...] -- run ssh against the VPS
vps_ssh() {
  require_target
  build_ssh_opts
  ssh "${VPS_SSH_OPTS[@]}" "$VPS_ALIAS" "$@"
}

# vps_run_remote <script-file-or-> [VAR=value ...]
# Pipes a local bash script to the VPS ("bash -s"); extra VAR=value pairs are exported remotely.
vps_run_remote() {
  local script="$1"; shift
  local envs="" kv
  for kv in "$@"; do envs+="export $(printf '%q' "$kv"); "; done
  if [ "$script" = "-" ]; then
    vps_ssh "${envs}bash -s"
  else
    vps_ssh "${envs}bash -s" < "$script"
  fi
}

ensure_data_dirs() {
  mkdir -p "$VPS_DATA_DIR"/{maps,health,logs,changes}
}

# confirm "<message>" -- interactive y/N unless VPS_ASSUME_YES=1 (still requires the --apply flag upstream)
confirm() {
  local ans
  if [ "${VPS_ASSUME_YES:-0}" = "1" ]; then return 0; fi
  [ -t 0 ] || die "confirmation required but no TTY; re-run interactively or set VPS_ASSUME_YES=1 deliberately"
  read -r -p "$1 Type 'yes' to continue: " ans
  [ "$ans" = "yes" ] || die "aborted by user"
}

# record_change <file> <text...> -- append to local change log
record_change() {
  local f="$1"; shift
  printf '%s\n' "$@" >> "$f"
}
