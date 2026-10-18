#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Staged switch for the OpenMind binaries. DRY RUN by default.
# Usage: switch.sh <unit> [--apply]
#   <unit> is a unit name without the .service suffix, e.g. galion-desk
# The script never starts or stops a service on its own: it installs the staged unit
# file, reloads systemd, and prints the restart command for the operator to run.
set -u
ROOT=/opt/galion/openmind
UNIT_DIR=$ROOT/deploy
BACKUP_DIR=$ROOT/deploy/backup
LIVE_DIR=/etc/systemd/system

if [ $# -lt 1 ]; then
  echo "usage: switch.sh <unit> [--apply]"
  exit 2
fi

UNIT=${1%.service}
APPLY=no
shift
for arg in "$@"; do
  [ "$arg" = "--apply" ] && APPLY=yes
done

SRC=$UNIT_DIR/$UNIT.service
if [ ! -f "$SRC" ]; then
  echo "switch: no staged unit at $SRC"
  exit 1
fi

echo "switch: unit        $UNIT"
echo "switch: staged file $SRC"
echo "switch: live file   $LIVE_DIR/$UNIT.service"
if [ -f "$LIVE_DIR/$UNIT.service" ]; then
  echo "switch: diff live -> staged"
  diff -u "$LIVE_DIR/$UNIT.service" "$SRC" || true
else
  echo "switch: no live unit installed, this would be a new unit"
fi

STAMP=$(TZ=Europe/Zagreb date +%Y%m%dT%H%M%S%z)
echo "switch: backup      $BACKUP_DIR/$UNIT.$STAMP.service"
echo "switch: would run"
echo "  mkdir -p $BACKUP_DIR"
if [ -f "$LIVE_DIR/$UNIT.service" ]; then
  echo "  cp -p $LIVE_DIR/$UNIT.service $BACKUP_DIR/$UNIT.$STAMP.service"
fi
echo "  install -m 644 $SRC $LIVE_DIR/$UNIT.service"
echo "  systemctl daemon-reload"
echo "  systemctl restart $UNIT   (operator runs this, not this script)"

if [ "$APPLY" != "yes" ]; then
  echo "switch: DRY RUN, nothing changed. Re-run with --apply after approval."
  exit 0
fi

mkdir -p "$BACKUP_DIR"
if [ -f "$LIVE_DIR/$UNIT.service" ]; then
  cp -p "$LIVE_DIR/$UNIT.service" "$BACKUP_DIR/$UNIT.$STAMP.service"
fi
install -m 644 "$SRC" "$LIVE_DIR/$UNIT.service"
systemctl daemon-reload
echo "switch: installed $LIVE_DIR/$UNIT.service"
echo "switch: now run: systemctl restart $UNIT"
