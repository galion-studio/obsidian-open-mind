#!/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Build every binary the cutover rehearsals need, on the BOX only, into
# $SIM/bin (default /workspace/swarm/sim/deploy-sim/bin). Static x86_64 musl.
#   simsup, sim-broken, sim-hang  this branch, sim/deploy-sim
#   tuner-old                     main 13a296c (same logic as the live hand build)
#   tuner-new                     this branch (== swarm/tuner-cutover 5fd1611 crate)
#   nerve-old                     ws/nerve a63b730 crates/galion-nerve (the live galion-nerve line)
#   nerve-new                     swarm/nerve 5802d0e crates/openmind-nerve
#   room-old / room-new           main 13a296c / swarm/room 5bb27e1
#   desk-old / desk-new           main 13a296c / swarm/desk-linkwatch a54c792
# Nothing is copied anywhere else; nothing touches the VPS.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
HERE=$(cd "$(dirname "$0")/../.." && pwd)
SIM=${SIM:-/workspace/swarm/sim/deploy-sim}
TGT=${TGT:-/workspace/swarm/target/deploy-sim}
TRIPLE=x86_64-unknown-linux-musl
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
mkdir -p "$SIM/bin" "$SIM/src"

free_gb() { df -BG --output=avail / | tail -1 | tr -dc 0-9; }
while [ "$(free_gb)" -lt 8 ]; do echo "disk < 8 GB free; waiting 120 s"; sleep 120; done

wt() { # wt NAME REV -> detached worktree of this repo at REV
  local d="$SIM/src/$1"
  if [ ! -d "$d" ]; then git -C "$HERE" worktree add -q --detach "$d" "$2"; fi
  git -C "$d" checkout -q --detach "$2"
  echo "$d"
}
build() { # build DIR TAG PKG BIN OUT
  local dir=$1 tag=$2 pkg=$3 bin=$4 out=$5
  (cd "$dir" && CARGO_TARGET_DIR="$TGT/$tag" nice -n 10 cargo build -q --release --locked -j 3 -p "$pkg" --bin "$bin" --target $TRIPLE)
  install -m 0755 "$TGT/$tag/$TRIPLE/release/$bin" "$SIM/bin/$out"
}

(cd "$HERE/sim/deploy-sim" && CARGO_TARGET_DIR="$TGT/simsup" nice -n 10 cargo build -q --release --locked -j 3 --target $TRIPLE)
for b in simsup sim-broken sim-hang; do install -m 0755 "$TGT/simsup/$TRIPLE/release/$b" "$SIM/bin/$b"; done

MAIN=$(wt main 13a296ce15b5bbd66adc26f6bfae787b2c1b5190)
build "$MAIN" main tuner tuner tuner-old
build "$MAIN" main room room room-old
build "$MAIN" main galion-desk galion-desk desk-old
build "$HERE" self tuner tuner tuner-new
build "$(wt nerve-old a63b730)" nerve-old galion-nerve galion-nerve nerve-old
build "$(wt nerve-new 5802d0e1dd69f3f71b3a1926f00d626ddd9c788d)" nerve-new openmind-nerve openmind-nerve nerve-new
build "$(wt room-new 5bb27e1)" room-new room room room-new
build "$(wt desk-new a54c792)" desk-new galion-desk galion-desk desk-new

ART=/workspace/swarm/artifacts/tuner/tuner-x86_64-unknown-linux-musl
if [ -f "$ART" ]; then
  if cmp -s "$ART" "$SIM/bin/tuner-new"; then echo "tuner-new == swarm/tuner-cutover artefact (reproducible)"; else echo "WARNING: tuner-new differs from $ART"; fi
fi
(cd "$SIM/bin" && sha256sum ./* | sed 's| \./| |' > SHA256SUMS && cat SHA256SUMS && ls -l)
