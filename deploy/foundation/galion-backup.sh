#!/bin/bash
# Galion snapshot backup (ws/foundation). Output: /var/backups/galion/galion-<stamp>.tar.gz (0600, root only).
# SQLite DBs are copied with the online backup API (galion-backup snapshot), never raw-copied.
# Contains secret-bearing files: stays on this host, never in git, never copied off.
set -euo pipefail
umask 077
DEST=${GALION_BACKUP_DEST:-/var/backups/galion}
KEEP=${GALION_BACKUP_KEEP:-12}           # 12 x 6h = 3 days
MINFREE_KB=${GALION_BACKUP_MINFREE_KB:-2097152}
BK=/opt/galion/rust/bin/galion-backup
STAMP=$(TZ=Europe/Zagreb date +%Y%m%d-%H%M%S%z)
log() { echo "ts=$(TZ=Europe/Zagreb date -Iseconds) svc=galion-backup $*"; }

mkdir -p "$DEST"; chmod 0700 "$DEST"
avail=$(df -Pk "$DEST" | awk 'NR==2{print $4}')
[ "$avail" -ge "$MINFREE_KB" ] || { log "level=error msg=\"low disk ${avail}KB\""; exit 1; }
exec 9>"$DEST/.lock"; flock -n 9 || { log "level=warn msg=\"already running\""; exit 0; }

W=$(mktemp -d "$DEST/.work-XXXXXX"); trap 'rm -rf "$W"' EXIT
S="$W/galion-$STAMP"; mkdir -p "$S/db" "$S/files"

snap() { # src name
  [ -f "$1" ] || { log "level=warn msg=\"missing db $1\""; return 0; }
  "$BK" snapshot "$1" "$S/db/$2" >/dev/null && "$BK" verify "$S/db/$2" >/dev/null \
    && echo "$1 $2" >> "$S/db/MAP" || { log "level=error msg=\"snapshot failed $1\""; return 1; }
}
snap /var/lib/memory-tencentdb/instances/galion/vectors.db memory-vectors.db
snap /var/lib/memory-tencentdb/vectors.db memory-root-vectors.db
snap /var/lib/hermes-rs/rs-state.db hermes-rs-state.db
snap /opt/galion/hermes/state.db hermes-legacy-state.db

# Plain files (DBs and their -wal/-shm excluded; covered above).
FILES=(
  /var/lib/memory-tencentdb
  /var/lib/hermes-rs
  /opt/galion/galion-src/galion-infrastructure/cloudflared
  /root/.cloudflared
  /opt/galion/hermes/.env /opt/galion/hermes/config.yaml
  /etc/openmind-memory.env /etc/memory-gateway.env
  /root/galion-secrets/admin-device.json
  /etc/systemd/system/hermes-rs.service /etc/systemd/system/openmind-memory.service
  /etc/systemd/system/link-watch-rs.service /etc/systemd/system/cloudflared.service
  /etc/systemd/system/galion-health.service
)
for f in /root/galion-switch-20261018-2140/dns-backup-*.json; do [ -e "$f" ] && FILES+=("$f"); done
EX=()
for f in "${FILES[@]}"; do [ -e "$f" ] && EX+=("$f"); done
tar -C / --exclude='*.db' --exclude='*.db-wal' --exclude='*.db-shm' --exclude='.backup' \
  -cf "$S/files/files.tar" "${EX[@]#/}" 2>/dev/null || [ $? -eq 1 ]

( cd "$S" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS )
tar -C "$W" -czf "$DEST/.galion-$STAMP.tar.gz.part" "galion-$STAMP"
chmod 0600 "$DEST/.galion-$STAMP.tar.gz.part"
mv "$DEST/.galion-$STAMP.tar.gz.part" "$DEST/galion-$STAMP.tar.gz"

# Rotation: keep newest $KEEP archives.
ls -1t "$DEST"/galion-*.tar.gz 2>/dev/null | tail -n +$((KEEP+1)) | xargs -r rm -f
size=$(du -k "$DEST/galion-$STAMP.tar.gz" | cut -f1)
log "level=info msg=\"ok\" archive=galion-$STAMP.tar.gz size_kb=$size kept=$(ls -1 "$DEST"/galion-*.tar.gz | wc -l)"
