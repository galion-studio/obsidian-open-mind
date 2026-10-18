#!/bin/sh
# galion-offsite-encrypt (ws/offsite). Runs as ExecStartPost of galion-backup.service.
# Encrypts plaintext backups in $SRC to the offsite age RECIPIENT (public key only; the
# decryption key is NOT needed here) -> $OUT/galion-<stamp>.tar.gz.age (0640 root:galion-offsite).
# Keeps $KEEP newest, a `latest` symlink and SHA256SUMS (oldest -> newest, last line = newest).
# Usage: galion-offsite-encrypt.sh [--perms-only]
set -eu
umask 027
SRC=${GALION_BACKUP_DEST:-/var/backups/galion}
OUT=${GALION_OFFSITE_DIR:-$SRC/offsite}
RCPT=${GALION_OFFSITE_RECIPIENT:-/etc/galion/offsite.recipient}
KEEP=${GALION_OFFSITE_KEEP:-12}
GRP=${GALION_OFFSITE_GROUP:-galion-offsite}
AGE=${GALION_AGE:-age}
log() { echo "ts=$(TZ=Europe/Zagreb date -Iseconds) svc=galion-offsite-encrypt $*"; }

# galion-backup.sh resets $SRC to 0700 on every run; give the offsite group traverse-only
# (no list, no read: plaintext archives stay 0600 root).
perms() {
  mkdir -p "$SRC" "$OUT"
  chown root:"$GRP" "$SRC" && chmod 0710 "$SRC"
  chown root:"$GRP" "$OUT" && chmod 0750 "$OUT"
}
perms
[ "${1:-}" = "--perms-only" ] && exit 0

[ -s "$RCPT" ] || { log level=error msg="missing recipient $RCPT"; exit 1; }
if grep -q 'AGE-SECRET-KEY' "$RCPT"; then log level=error msg="recipient file holds a secret key, refusing"; exit 1; fi
command -v "$AGE" >/dev/null || { log level=error msg="no age binary"; exit 1; }

exec 8>/run/galion-offsite-encrypt.lock
flock -n 8 || { log level=warn msg="already running"; exit 0; }

new=0
# newest $KEEP plaintext archives that have no ciphertext yet
for f in $(ls -1t "$SRC"/galion-*.tar.gz 2>/dev/null | head -n "$KEEP"); do
  n=$(basename "$f")
  [ -f "$OUT/$n.age" ] && continue
  tmp="$OUT/.$n.age.part"
  rm -f "$tmp"
  if ! "$AGE" -e -R "$RCPT" -o "$tmp" "$f"; then rm -f "$tmp"; log level=error msg="encrypt failed $n"; exit 1; fi
  [ "$(head -c 21 "$tmp")" = "age-encryption.org/v1" ] || { rm -f "$tmp"; log level=error msg="bad age header $n"; exit 1; }
  touch -r "$f" "$tmp"
  chown root:"$GRP" "$tmp"; chmod 0640 "$tmp"
  mv -f "$tmp" "$OUT/$n.age"
  new=$((new+1))
  log level=info msg=encrypted file="$n.age" size_bytes="$(stat -c %s "$OUT/$n.age")"
done

# rotation by mtime (= plaintext archive time)
ls -1t "$OUT"/galion-*.tar.gz.age 2>/dev/null | tail -n +$((KEEP+1)) | while read -r old; do rm -f "$old"; done

newest=$(ls -1t "$OUT"/galion-*.tar.gz.age 2>/dev/null | head -n 1 || true)
[ -n "$newest" ] || { log level=warn msg="no offsite archives"; exit 0; }
ln -sfn "$(basename "$newest")" "$OUT/.latest.tmp" && mv -Tf "$OUT/.latest.tmp" "$OUT/latest"

( cd "$OUT" && ls -1tr galion-*.tar.gz.age | while read -r a; do sha256sum "$a"; done > .SHA256SUMS.part )
chown root:"$GRP" "$OUT/.SHA256SUMS.part"; chmod 0640 "$OUT/.SHA256SUMS.part"
mv -f "$OUT/.SHA256SUMS.part" "$OUT/SHA256SUMS"
log level=info msg=ok new="$new" latest="$(basename "$newest")" kept="$(ls -1 "$OUT"/galion-*.tar.gz.age | wc -l)"
