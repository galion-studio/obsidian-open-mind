#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio
#
# SEC-3: tighten Hermes home permissions. STAGED, NOT RUN BY THE SWARM.
# Owner (Slavik) runs it on the VPS as root, after reading
# docs/launch/SEC-RUNBOOKS.md (SEC-3).
#
#   sec3-hermes-perms.sh                      dry run: show current -> target, change nothing
#   sec3-hermes-perms.sh --apply              change the listed items (records a rollback file first)
#   sec3-hermes-perms.sh --apply --fix-world-writable
#                                             also chmod o-w every other world-writable file/dir
#                                             under HERMES_HOME (reported in the dry run)
#   sec3-hermes-perms.sh --rollback FILE      restore modes/owners recorded in FILE
#
# Targets (root:root):
#   config.yaml        666 -> 600  holds api_key / client_secret / sudo_password fields
#   SOUL.md            666 -> 644  read-only persona text; only root writes it
#   vault/             777 -> 700  local browser vault dir
#   vault/.vault.lock  666 -> 600
#   USER.md            664 -> 600  operator profile (personal data)
#
# It never reads file contents, never follows symlinks (a symlinked target is
# refused), never touches anything outside HERMES_HOME, and refuses to run
# against secrets/persist or anything named galion-control. Every live service
# that reads these files runs as root (systemctl show: User= empty), so 600/700
# does not take access away from hermes-rs, console or galion-one.
# Env: HERMES_HOME (default /opt/galion/hermes), SEC3_STATE_DIR (default
# /root/sec3-perms), SEC3_NO_CHOWN=1 (skip chown, for fixture tests as non-root).
set -euo pipefail
umask 077
H=${HERMES_HOME:-/opt/galion/hermes}
STATE=${SEC3_STATE_DIR:-/root/sec3-perms}
mode=dry; fixww=0; rb=""
while [ $# -gt 0 ]; do
  case "$1" in
    --apply) mode=apply ;;
    --fix-world-writable) fixww=1 ;;
    --rollback) mode=rollback; rb=${2:?--rollback needs a file}; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

H=$(cd "$H" && pwd -P) || { echo "no such HERMES_HOME" >&2; exit 2; }
case "$H" in
  */secrets/persist*|*galion-control*|/|/opt|/opt/galion|/root|/etc) echo "refusing HERMES_HOME=$H" >&2; exit 2 ;;
esac

# path (relative to H) | target mode | expected type (f/d)
TARGETS="config.yaml 600 f
SOUL.md 644 f
vault 700 d
vault/.vault.lock 600 f
USER.md 600 f"

ts() { TZ=Europe/Zagreb date +%Y-%m-%dT%H:%M:%S%:z; }
cur() { stat -c '%a %U:%G' -- "$1"; }

world_writable() {
  # other world-writable entries (not symlinks), excluding the listed targets
  find "$H" -xdev \( -type f -o -type d \) -perm -o+w -printf '%P\n' 2>/dev/null |
    grep -v -x -F -e config.yaml -e SOUL.md -e vault -e vault/.vault.lock -e USER.md || true
}

if [ "$mode" = rollback ]; then
  [ -f "$rb" ] || { echo "no rollback file $rb" >&2; exit 2; }
  while IFS=$'\t' read -r rel m own; do
    case "$rel" in ''|\#*) continue ;; esac
    p="$H/$rel"
    [ -e "$p" ] && [ ! -L "$p" ] || { echo "skip (missing or symlink): $rel"; continue; }
    chmod "$m" -- "$p"
    [ "${SEC3_NO_CHOWN:-}" = 1 ] || chown "$own" -- "$p"
  done < "$rb"
  echo "$(ts) rollback from $rb done"
  exit 0
fi

echo "$(ts) SEC-3 $mode on $H"
printf '%-22s %-16s -> %s\n' path current target
plan=()
while read -r rel want typ; do
  p="$H/$rel"
  if [ -L "$p" ]; then echo "REFUSE: $rel is a symlink" >&2; exit 3; fi
  if [ ! -e "$p" ]; then printf '%-22s %-16s -> %s\n' "$rel" missing "skip"; continue; fi
  case "$typ" in f) [ -f "$p" ] ;; d) [ -d "$p" ] ;; esac || { echo "REFUSE: $rel is not type $typ" >&2; exit 3; }
  printf '%-22s %-16s -> %s root:root\n' "$rel" "$(cur "$p")" "$want"
  plan+=("$rel $want")
done <<< "$TARGETS"

ww=$(world_writable)
if [ -n "$ww" ]; then
  echo "other world-writable entries under HERMES_HOME ($(printf '%s\n' "$ww" | wc -l)):"
  printf '%s\n' "$ww" | head -50 | sed 's/^/  /'
  [ $fixww = 1 ] && echo "  -> chmod o-w (--fix-world-writable)" || echo "  -> report only (add --fix-world-writable to fix)"
fi

if [ "$mode" = dry ]; then echo "dry run: nothing changed"; exit 0; fi

mkdir -p "$STATE"
bk="$STATE/sec3-before-$(TZ=Europe/Zagreb date +%Y%m%dT%H%M%S%N).tsv"
{
  echo "# SEC-3 rollback file, $(ts), HERMES_HOME=$H (modes and owners only)"
  for e in "${plan[@]}"; do rel=${e% *}; printf '%s\t%s\t%s\n' "$rel" "$(stat -c %a -- "$H/$rel")" "$(stat -c %U:%G -- "$H/$rel")"; done
  printf '%s\n' "$ww" | while read -r rel; do [ -n "$rel" ] && printf '%s\t%s\t%s\n' "$rel" "$(stat -c %a -- "$H/$rel")" "$(stat -c %U:%G -- "$H/$rel")"; done
} > "$bk"
echo "rollback file: $bk"

for e in "${plan[@]}"; do
  rel=${e% *}; want=${e##* }
  [ "${SEC3_NO_CHOWN:-}" = 1 ] || chown root:root -- "$H/$rel"
  chmod "$want" -- "$H/$rel"
done
if [ $fixww = 1 ] && [ -n "$ww" ]; then
  printf '%s\n' "$ww" | while read -r rel; do [ -n "$rel" ] && [ ! -L "$H/$rel" ] && chmod o-w -- "$H/$rel"; done
fi
echo "after:"
for e in "${plan[@]}"; do rel=${e% *}; printf '  %-22s %s\n' "$rel" "$(cur "$H/$rel")"; done
echo "$(ts) SEC-3 applied; remaining world-writable under HERMES_HOME: $(world_writable | grep -c . || true)"
