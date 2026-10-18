#!/bin/sh
# add-laptop-key.sh (ws/offsite): authorise a laptop-generated ssh PUBLIC key for galion-offsite.
# The private key never leaves the laptop. Run as root on the VPS.
#   add-laptop-key.sh 'ssh-ed25519 AAAA... [comment]'          add (idempotent, de-duplicated)
#   add-laptop-key.sh --remove 'ssh-ed25519 AAAA... [comment]' remove by key blob (or just 'AAAA...')
#   add-laptop-key.sh --list                                   fingerprints of authorised keys
# Each line is written as: restrict,command="/usr/local/bin/galion-offsite-serve" ssh-ed25519 <blob> [comment]
# Only a single ssh-ed25519 public key is accepted (no options, no other key types, no extra lines).
# Owner/mode of authorized_keys are preserved (default root:root 0644); update is atomic (tmp + mv).
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin; export PATH
AK=${GALION_OFFSITE_AK:-/var/lib/galion-offsite/.ssh/authorized_keys}
PFX='restrict,command="/usr/local/bin/galion-offsite-serve"'
die() { echo "add-laptop-key: $*" >&2; exit 1; }
say() { echo "add-laptop-key: $*"; }
[ "$(id -u)" = 0 ] || die "run as root"
[ -d "$(dirname "$AK")" ] || die "missing $(dirname "$AK") (run setup-vps.sh first)"

mode=add
case "${1:-}" in
  --remove) mode=remove; shift ;;
  --list)   mode=list; shift ;;
  -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
esac

if [ "$mode" = list ]; then
  [ -s "$AK" ] || { say "no keys"; exit 0; }
  t=$(mktemp); trap 'rm -f "$t"' EXIT
  sed 's/^restrict,command="[^"]*" //' "$AK" > "$t"; ssh-keygen -lf "$t"; exit 0
fi

[ $# -ge 1 ] || die "usage: add-laptop-key.sh [--remove] '<ssh-ed25519 public key line>'"
in=$(printf '%s' "$*" | tr -d '\r')           # tolerate CRLF from Windows copy/paste
[ "$(printf '%s\n' "$in" | wc -l)" -eq 1 ] || die "expected exactly one line"
set -f
# shellcheck disable=SC2086
set -- $in
set +f
if [ "$mode" = remove ] && [ $# -eq 1 ]; then set -- ssh-ed25519 "$1"; fi
[ $# -ge 2 ] && [ $# -le 3 ] || die "expected: ssh-ed25519 <base64> [comment] (got $# fields)"
[ "$1" = ssh-ed25519 ] || die "only ssh-ed25519 keys are accepted (got '$1'; key options are not allowed)"
blob=$2; cmt=${3:-}
printf '%s\n' "$blob" | grep -Eq '^AAAAC3NzaC1lZDI1NTE5AAAAI[A-Za-z0-9+/]{43}$' || die "not a valid ssh-ed25519 key blob"
[ -z "$cmt" ] || printf '%s\n' "$cmt" | grep -Eq '^[A-Za-z0-9._@:+=-]{1,100}$' || die "comment may only contain A-Z a-z 0-9 . _ @ : + = - (max 100)"
chk=$(mktemp); trap 'rm -f "$chk"' EXIT
printf 'ssh-ed25519 %s\n' "$blob" > "$chk"
fp=$(ssh-keygen -lf "$chk" 2>/dev/null) || die "ssh-keygen rejects this key"
case "$fp" in 256\ SHA256:*\(ED25519\)) ;; *) die "unexpected key type: $fp" ;; esac
fp=$(printf '%s\n' "$fp" | awk '{print $2}')

exec 9>/run/lock/galion-offsite-authorized_keys.lock
flock -w 30 9 || die "could not lock"
if [ -e "$AK" ]; then own=$(stat -c '%u:%g' "$AK"); perm=$(stat -c '%a' "$AK"); else own=0:0; perm=644; touch "$AK"; fi
tmp=$(mktemp "$AK.XXXXXX"); trap 'rm -f "$chk" "$tmp"' EXIT
before=$(grep -c . "$AK" || true)
# drop every line carrying this blob (re-added once below in add mode) and exact duplicate lines
awk -v b="$blob" '{ for (i = 1; i <= NF; i++) if ($i == b) next } NF && !seen[$0]++' "$AK" > "$tmp"
if [ "$mode" = add ]; then
  if [ -n "$cmt" ]; then printf '%s ssh-ed25519 %s %s\n' "$PFX" "$blob" "$cmt" >> "$tmp"
  else printf '%s ssh-ed25519 %s\n' "$PFX" "$blob" >> "$tmp"; fi
fi
chown "$own" "$tmp"; chmod "$perm" "$tmp"
mv -f "$tmp" "$AK"
after=$(grep -c . "$AK" || true)
if [ "$mode" = add ]; then say "authorised $fp ${cmt:+($cmt) }for galion-offsite; keys: $before -> $after"
else
  [ "$after" -lt "$before" ] && say "removed $fp; keys: $before -> $after" || say "not present: $fp; keys: $after"
fi
