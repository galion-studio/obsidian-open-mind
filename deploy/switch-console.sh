#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Staged switch for the console. DRY RUN by default, --apply required.
# Wrapper around deploy/switch.sh with the unit name fixed, so the operator has one
# obvious command for the console and never has to guess the unit name.
set -u
ROOT=/opt/galion/openmind
exec bash "$ROOT/deploy/switch.sh" console-prod "$@"
