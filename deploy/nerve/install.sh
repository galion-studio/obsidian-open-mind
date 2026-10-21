#!/bin/sh
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Install or upgrade galion-nerve on the LXC. Run as root from the dir that
# holds the musl binary and this unit. DRY=1 (default) installs with reflexes
# in dry-run; switch live later with: echo live > /var/lib/galion-nerve/mode
# The binary is built from crates/openmind-nerve (`openmind-nerve`, static musl)
# and installed under the unit's old path. Changing the live unit needs the
# owner's approval (swarm rule 4); this script is never run by the swarm.
set -eu
DRY=${DRY:-1}
BIN=/opt/galion/rust/bin/galion-nerve
UNIT=/etc/systemd/system/galion-nerve.service
STAMP=$(TZ=Europe/Zagreb date +%Y%m%d-%H%M%S)
BK=/var/backups/galion/nerve; mkdir -p "$BK"; chmod 0700 "$BK"
[ -f "$BIN" ] && cp -p "$BIN" "$BK/galion-nerve-$STAMP.bin"
[ -f "$UNIT" ] && cp -p "$UNIT" "$BK/galion-nerve.service-$STAMP"
SRC=openmind-nerve; [ -f "$SRC" ] || SRC=galion-nerve
install -m 0755 "$SRC" "$BIN.new" && mv -f "$BIN.new" "$BIN"
install -m 0644 galion-nerve.service "$UNIT"
[ "$DRY" = 1 ] && sed -i 's/^Environment=NERVE_DRY_RUN=0/Environment=NERVE_DRY_RUN=1/' "$UNIT"
systemctl daemon-reload
systemctl enable galion-nerve.service
systemctl restart galion-nerve.service
sleep 3
systemctl is-active galion-nerve.service
curl -s -m 5 -o /dev/null -w 'healthz %{http_code}\n' http://127.0.0.1:8670/healthz
