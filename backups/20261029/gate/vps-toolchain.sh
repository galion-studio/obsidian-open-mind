#!/bin/bash
# as galion-gate: own rust toolchain + crate cache under /var/lib/galion-gate/toolchain
set -euo pipefail
export HOME=/var/lib/galion-gate RUSTUP_HOME=/var/lib/galion-gate/toolchain/rustup CARGO_HOME=/var/lib/galion-gate/toolchain/cargo
cd $HOME
if [ ! -x $CARGO_HOME/bin/cargo ]; then
  curl -sSf --proto '=https' --tlsv1.2 https://sh.rustup.rs -o /var/lib/galion-gate/rustup-init.sh
  sh /var/lib/galion-gate/rustup-init.sh -y -q --no-modify-path --profile minimal --default-toolchain 1.99.1 >/dev/null 2>&1
  rm -f /var/lib/galion-gate/rustup-init.sh
fi
$CARGO_HOME/bin/rustc --version
if [ ! -d repo/.git ]; then rm -rf repo; git clone -q -b swarm/hermes-sandbox-gate-v2 gate-v2.bundle repo; fi
cd repo && git log --oneline -1
$CARGO_HOME/bin/cargo fetch -q --locked && echo "fetch ok"
CARGO_TARGET_DIR=$HOME/gate-target nice -n 10 $CARGO_HOME/bin/cargo build -q --release --locked --offline -p hermes-gate -j 4 && echo "build ok"
