#!/bin/sh
# galion-offsite-pull.sh (ws/offsite): pull the newest encrypted Galion backup from the VPS.
# POSIX sh; macOS + Linux. Only ciphertext (.tar.gz.age) is ever written to disk.
#   1. `list` -> names + sha256 (last line = newest); skip if we already have it
#   2. `latest` > tmp, sha256 must match an entry from `list`
#   3. store in $GALION_BACKUP_DIR (default ~/GalionBackups), keep newest $GALION_OFFSITE_KEEP (8)
#   4. if age/rage and the offsite agekey are present: decrypt to a pipe | tar tz (no plaintext on disk)
# Exit: 0 ok/up to date, 1 error.
set -eu
umask 077
HOST=${GALION_OFFSITE_HOST:-51.38.137.193}
PORT=${GALION_OFFSITE_PORT:-22}
RUSER=${GALION_OFFSITE_USER:-galion-offsite}
CFG=${GALION_OFFSITE_HOME:-$HOME/.galion-offsite}
KEY=${GALION_OFFSITE_KEY:-$CFG/id_ed25519}
KH=${GALION_OFFSITE_KNOWN_HOSTS:-$CFG/known_hosts}
AGEKEY=${GALION_OFFSITE_AGEKEY:-$CFG/offsite.agekey}
DIR=${GALION_BACKUP_DIR:-$HOME/GalionBackups}
KEEP=${GALION_OFFSITE_KEEP:-8}
PATH=$PATH:/opt/homebrew/bin:/usr/local/bin; export PATH

log() { echo "$(date '+%Y-%m-%dT%H:%M:%S%z') galion-offsite-pull $*"; }
die() { log "ERROR $*"; [ -n "${TMPF:-}" ] && rm -f "$TMPF"; [ -n "${LOCK:-}" ] && rmdir "$LOCK" 2>/dev/null; exit 1; }
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | awk '{print $1}'; }

[ -r "$KEY" ] || die "missing ssh key $KEY (run install-laptop.sh)"
if [ -s "$KH" ]; then HK="-o StrictHostKeyChecking=yes"; else HK="-o StrictHostKeyChecking=accept-new"; fi
rssh() { # shellcheck disable=SC2086
  ssh -i "$KEY" -p "$PORT" -o IdentitiesOnly=yes -o BatchMode=yes -o IdentityAgent=none \
      -o UserKnownHostsFile="$KH" $HK -o ConnectTimeout=20 -o ServerAliveInterval=15 \
      -o ServerAliveCountMax=4 -o ClearAllForwardings=yes -T "$RUSER@$HOST" "$@" </dev/null; }

mkdir -p "$DIR"; chmod 700 "$DIR"
LOCK=$DIR/.pull.lock
mkdir "$LOCK" 2>/dev/null || { LOCK=; die "another pull is running ($DIR/.pull.lock)"; }
trap '[ -n "${TMPF:-}" ] && rm -f "$TMPF"; [ -n "${LOCK:-}" ] && rmdir "$LOCK" 2>/dev/null; :' EXIT INT TERM

RE='^[0-9a-f]{64}  galion-[0-9T+:-]+\.tar\.gz\.age$'
getlist() { rssh list > "$DIR/.list" || die "ssh list failed"
  grep -Ev "$RE" "$DIR/.list" >/dev/null && die "unexpected list output"
  [ -s "$DIR/.list" ] || die "remote list is empty"; }
getlist
newest=$(tail -n 1 "$DIR/.list" | awk '{print $2}')
nsum=$(tail -n 1 "$DIR/.list" | awk '{print $1}')

if [ -f "$DIR/$newest" ] && [ "$(sha "$DIR/$newest")" = "$nsum" ]; then
  log "up to date: $newest"
  name=$newest
else
  TMPF=$(mktemp "$DIR/.pull.XXXXXX")
  rssh latest > "$TMPF" || die "ssh latest failed"
  got=$(sha "$TMPF")
  name=$(awk -v s="$got" '$1==s{print $2}' "$DIR/.list" | tail -n 1)
  if [ -z "$name" ]; then getlist; name=$(awk -v s="$got" '$1==s{print $2}' "$DIR/.list" | tail -n 1); fi
  [ -n "$name" ] || die "sha256 $got of downloaded file matches nothing in remote list"
  chmod 600 "$TMPF"; mv -f "$TMPF" "$DIR/$name"; TMPF=
  log "pulled $name size=$(wc -c < "$DIR/$name" | tr -d ' ') sha256=$got (matches remote list)"
fi

# optional integrity check: decrypt to a pipe only, list the tar
AGEBIN=$(command -v age 2>/dev/null || command -v rage 2>/dev/null || true)
if [ -n "$AGEBIN" ] && [ -r "$AGEKEY" ]; then
  st=$(mktemp "$DIR/.st.XXXXXX")
  n=$( ( ("$AGEBIN" -d -i "$AGEKEY" "$DIR/$name" || echo age >&3) | (tar tzf - || echo tar >&3) | wc -l ) 3>"$st" )
  bad=$(cat "$st"); rm -f "$st"
  [ -z "$bad" ] || die "integrity check FAILED ($bad) for $name"
  log "integrity ok: $name decrypts, tar lists $(echo "$n" | tr -d ' ') entries"
else
  log "integrity check skipped (no age/rage or no $AGEKEY)"
fi

# keep newest $KEEP
ls -1t "$DIR"/galion-*.tar.gz.age 2>/dev/null | tail -n +$((KEEP+1)) | while read -r old; do rm -f "$old"; log "pruned $(basename "$old")"; done
( cd "$DIR" && for f in $(ls -1tr galion-*.tar.gz.age); do printf '%s  %s\n' "$(sha "$f")" "$f"; done > SHA256SUMS )
log "ok kept=$(ls -1 "$DIR"/galion-*.tar.gz.age | wc -l | tr -d ' ') dir=$DIR"
date '+%Y-%m-%dT%H:%M:%S%z ok '"$name" > "$DIR/.last-ok"
