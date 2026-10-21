#!/bin/sh
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Real-life run of the whole box stack: build the release binaries
# (CPU-polite), boot every service on 127.0.0.1:19500-19551 under
# galion-boot supervise, run all scenarios, tear down, write
# junit.xml + report.md. Optional: LIVE=1 also runs the READ-ONLY live probe
# (GET-only over ssh; never writes or restarts anything).
set -eu
cd "$(dirname "$0")/../.."
T="${CARGO_TARGET_DIR:-target}"
nice -n 10 cargo build --release --locked -j "${JOBS:-4}" -p galion-realtest \
  -p galion-boot -p galion-one -p openmind-memory -p tuner -p galion-desk \
  -p link-watch -p guardian-tick -p persona-plex-relay
rc=0
"$T/release/galion-realtest" stack --bin-dir "$T/release" --out "$T/realtest/stack" || rc=$?
if [ "${LIVE:-0}" = 1 ]; then
  "$T/release/galion-realtest" live --out "$T/realtest/live" || rc=$?
fi
exit "$rc"
