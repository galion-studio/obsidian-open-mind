#!/bin/bash
# Restore the newest (or given) backup into a temp dir and verify it. Touches nothing live.
set -euo pipefail
umask 077
DEST=${GALION_BACKUP_DEST:-/var/backups/galion}
A=${1:-$(ls -1t "$DEST"/galion-*.tar.gz | head -1)}
T=$(mktemp -d /tmp/galion-restore-XXXXXX); trap 'rm -rf "$T"' EXIT
tar -C "$T" -xzf "$A"
cd "$T"/galion-*
sha256sum --quiet -c SHA256SUMS && echo "checksums ok"
for d in db/*.db; do /opt/galion/rust/bin/galion-backup verify "$d" | sed "s|$T/||"; done
mkdir -p restored && tar -C restored -xf files/files.tar
echo "files restored: $(find restored -type f | wc -l)"
for p in var/lib/memory-tencentdb/persona.md var/lib/hermes-rs root/galion-secrets/admin-device.json etc/openmind-memory.env; do
  [ -e "restored/$p" ] && echo "present: /$p" || echo "MISSING: /$p"
done
echo "restore test ok: $(basename "$A")"
