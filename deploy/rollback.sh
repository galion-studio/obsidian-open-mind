#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Staged rollback: restores the unit file the switch script backed up. DRY RUN by default.
# Usage: rollback.sh <unit> [--apply]
set -u
ROOT=/opt/galion/openmind
BACKUP_DIR=$ROOT/deploy/backup
LIVE_DIR=/etc/systemd/system

if [ $# -lt 1 ]; then
  echo "usage: rollback.sh <unit> [--apply]"
  exit 2
fi

UNIT=${1%.service}
APPLY=no
shift
for arg in "$@"; do
  [ "$arg" = "--apply" ] && APPLY=yes
done

if [ ! -d "$BACKUP_DIR" ]; then
  echo "rollback: no backup directory $BACKUP_DIR yet, nothing to restore"
  exit 1
fi

LATEST=$(ls -1 "$BACKUP_DIR/$UNIT."*.service 2>/dev/null | sort | tail -1)
if [ -z "$LATEST" ]; then
  echo "rollback: no backup for unit $UNIT in $BACKUP_DIR"
  exit 1
fi

echo "rollback: unit    $UNIT"
echo "rollback: backup  $LATEST"
echo "rollback: live    $LIVE_DIR/$UNIT.service"
if [ -f "$LIVE_DIR/$UNIT.service" ]; then
  echo "rollback: diff live -> backup"
  diff -u "$LIVE_DIR/$UNIT.service" "$LATEST" || true
fi
echo "rollback: would run"
echo "  install -m 644 $LATEST $LIVE_DIR/$UNIT.service"
echo "  systemctl daemon-reload"
echo "  systemctl restart $UNIT   (operator runs this, not this script)"

if [ "$APPLY" != "yes" ]; then
  echo "rollback: DRY RUN, nothing changed. Re-run with --apply after approval."
  exit 0
fi

install -m 644 "$LATEST" "$LIVE_DIR/$UNIT.service"
systemctl daemon-reload
echo "rollback: restored $LIVE_DIR/$UNIT.service from $LATEST"
echo "rollback: now run: systemctl restart $UNIT"
