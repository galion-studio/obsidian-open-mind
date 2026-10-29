#!/bin/bash
# hermes-gate VPS install (run as root). Never touches /opt/galion/openmind, /opt/galion/rust/src or hermes-rs.
set -euo pipefail
H=/var/lib/galion-gate
id galion-gate >/dev/null 2>&1 || useradd --system --home-dir $H --no-create-home --shell /usr/sbin/nologin galion-gate
install -d -o galion-gate -g galion-gate -m 750 $H
install -d -o galion-gate -g galion-gate -m 700 $H/keys
for d in repo scratch staging toolchain runs; do install -d -o galion-gate -g galion-gate -m 750 $H/$d; done
install -d -o root -g root -m 755 /opt/galion/gate /opt/galion/gate/bin
install -o galion-gate -g galion-gate -m 640 /tmp/gate-v2.bundle $H/gate-v2.bundle
rm -f /tmp/gate-v2.bundle
echo "layout ok"
