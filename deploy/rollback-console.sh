#!/usr/bin/env bash
# Staged rollback for the console. DRY RUN by default, --apply required.
# Wrapper around deploy/rollback.sh: it restores the console unit file that
# switch-console.sh copied into deploy/backup/ before installing the new one.
# NOTE: today no console unit named console-prod exists, so there is nothing to
# back up yet; the live unit is `console` (vite dev server) and this script does
# not touch it. Restoring the vite dev unit means copying `console` back from
# deploy/backup/ if the operator makes a backup of it by hand.
set -u
ROOT=/opt/galion/openmind
exec bash "$ROOT/deploy/rollback.sh" console-prod "$@"
