#!/usr/bin/env bash
# Set the VPS host and user in BOTH config.env and the slavik-vps block of ~/.ssh/config.
# Local-only edit (never touches the VPS). Backs up both files first.
# Usage: set-target.sh <host-or-ip> <user> [port]
#        set-target.sh            (no args: sync ~/.ssh/config from current config.env)
set -euo pipefail
BIN="$(cd "$(dirname "$0")" && pwd)"; CFG="$(dirname "$BIN")/config.env"
SSHCFG="$HOME/.ssh/config"
[ -f "$CFG" ] || { echo "missing $CFG" >&2; exit 1; }
# shellcheck source=/dev/null
. "$CFG"
HOST="${1:-$VPS_HOST}"; USERN="${2:-$VPS_USER}"; PORT="${3:-${VPS_PORT:-22}}"
case "$HOST$USERN" in *__FILL_ME*) echo "host/user still placeholders; pass them: $0 <host> <user>" >&2; exit 1;; esac
[[ "$HOST" =~ ^[A-Za-z0-9._:-]+$ ]] || { echo "invalid host: $HOST" >&2; exit 1; }
[[ "$USERN" =~ ^[a-z_][a-z0-9_.-]*$ ]] || { echo "invalid user: $USERN" >&2; exit 1; }
[[ "$PORT" =~ ^[0-9]+$ ]] || { echo "invalid port: $PORT" >&2; exit 1; }
stamp=$(date +%Y%m%d-%H%M%S)
cp -p "$CFG" "$CFG.bak-$stamp"
sed -i -E "s|^VPS_HOST=.*|VPS_HOST=\"$HOST\"|; s|^VPS_USER=.*|VPS_USER=\"$USERN\"|; s|^VPS_PORT=.*|VPS_PORT=\"$PORT\"|" "$CFG"
if [ -f "$SSHCFG" ] && grep -q '^# >>> slavik-vps' "$SSHCFG"; then
  cp -p "$SSHCFG" "$SSHCFG.bak-$stamp"
  awk -v h="$HOST" -v u="$USERN" -v p="$PORT" '
    /^# >>> slavik-vps/ {inb=1}
    inb && /^[[:space:]]*HostName / {print "    HostName " h; next}
    inb && /^[[:space:]]*User /     {print "    User " u; next}
    inb && /^[[:space:]]*Port /     {print "    Port " p; next}
    /^# <<< slavik-vps/ {inb=0}
    {print}' "$SSHCFG.bak-$stamp" > "$SSHCFG"
  chmod 600 "$SSHCFG"
  echo "updated slavik-vps block in $SSHCFG (backup: $SSHCFG.bak-$stamp)"
else
  echo "no '# >>> slavik-vps' block in $SSHCFG; scripts still work via config.env" >&2
fi
echo "config.env set: host=$HOST user=$USERN port=$PORT (backup: $CFG.bak-$stamp)"
echo "next: $BIN/vps-ssh.sh --check"
