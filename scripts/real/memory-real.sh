#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# Real-life test run for openmind-memory (swarm slot 09, ports 19090-19099).
# Builds the release binary, runs crates/openmind-memory/tests/real_memory.rs
# against it (each test starts the binary on its own 1909x port with a fresh
# fixture database and kills it afterwards), then makes sure nothing is left
# listening. GALION_REAL_VPS=1 adds the READ-ONLY GET probe of live
# 127.0.0.1:8420 over ssh (status codes, latency, key shape; no bodies).
# Times are Europe/Zagreb with an offset. No Python.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/swarm/target/memory-real}"
export GALION_REAL=1
stamp() { TZ=Europe/Zagreb date -Iseconds; }

echo "memory-real: start $(stamp)"
nice -n 10 cargo build --release -j 4 -p openmind-memory
BIN="$CARGO_TARGET_DIR/release/openmind-memory"
echo "memory-real: binary $BIN ($(stat -c %s "$BIN") bytes, sha256 $(sha256sum "$BIN" | cut -c1-16))"
"$BIN" --version

cleanup() {
  # Any server a panicking test leaked: only our own binary on our ports.
  for p in $(seq 19090 19099); do
    pids=$(ss -ltnpH "sport = :$p" 2>/dev/null | sed -n 's/.*pid=\([0-9]*\).*/\1/p' | sort -u)
    for pid in $pids; do
      if [ "$(readlink -f /proc/"$pid"/exe 2>/dev/null)" = "$(readlink -f "$BIN")" ]; then
        kill -9 "$pid" 2>/dev/null || true
      fi
    done
  done
}
trap cleanup EXIT

OPENMIND_MEMORY_BIN="$BIN" nice -n 10 cargo test --release -j 4 -p openmind-memory \
  --test real_memory -- --test-threads=1 --nocapture

cleanup
left=$(ss -ltnH | awk '{print $4}' | grep -cE ':1909[0-9]$' || true)
echo "memory-real: listeners left on 19090-19099: $left"
echo "memory-real: done $(stamp)"
