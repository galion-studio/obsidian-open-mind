#!/bin/bash
# as galion-gate: update the gate's own repo clone from the bundle and rebuild hermes-gate
set -euo pipefail
export HOME=/var/lib/galion-gate RUSTUP_HOME=/var/lib/galion-gate/toolchain/rustup CARGO_HOME=/var/lib/galion-gate/toolchain/cargo
cd $HOME/repo
git fetch -q $HOME/gate-v2.bundle swarm/hermes-sandbox-gate-v2
git checkout -q --detach FETCH_HEAD && git branch -q -f swarm/hermes-sandbox-gate-v2 FETCH_HEAD && git checkout -q swarm/hermes-sandbox-gate-v2
git log --oneline -1
$CARGO_HOME/bin/cargo fetch -q --locked
CARGO_TARGET_DIR=$HOME/gate-target nice -n 10 $CARGO_HOME/bin/cargo build -q --release --locked --offline -p hermes-gate -j 4 && echo "build ok"
