#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# Real-life run for swarm/memory-schema (slot 36, ports 19360-19369 only).
#  1. builds the release openmind-memory binary;
#  2. builds the PREVIOUS binary (main 13a296c, git archive into the target
#     dir) for the rollback test;
#  3. runs tests/real_schema.rs on 19366-19369: kill -9 during migration in a
#     loop (large pre-v3 fixture DB), CJK/emoji round trip + FTS, import mode
#     replay/idempotence, rollback to the previous binary;
#  4. re-runs the memory-real suite (tests/real_memory.rs) on 19360-19365;
#  5. GALION_REAL_VPS=1 adds exactly ONE read-only GET /health of live 8420
#     over ssh (nothing written on the VPS). The memory-real live test, which
#     GETs more paths, is NOT run by this script.
# Kills anything of ours left on 19360-19369. Times Europe/Zagreb. No Python.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"
T="${CARGO_TARGET_DIR:-/workspace/swarm/target/memory-schema}"
export CARGO_TARGET_DIR="$T" CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export GALION_REAL=1
stamp() { TZ=Europe/Zagreb date -Iseconds; }

echo "memory-schema: start $(stamp)"
nice -n 10 cargo build --release -j 3 -p openmind-memory
BIN="$T/release/openmind-memory"
echo "memory-schema: binary $BIN ($(stat -c %s "$BIN") bytes, sha256 $(sha256sum "$BIN" | cut -c1-16))"

PREV_SRC="$T/prev-src"; PREV_T="$T/prev-target"; PREV="$PREV_T/release/openmind-memory"
if [ ! -x "$PREV" ]; then
  rm -rf "$PREV_SRC"; mkdir -p "$PREV_SRC"
  git archive 13a296ce15b5bbd66adc26f6bfae787b2c1b5190 | tar -x -C "$PREV_SRC"
  (cd "$PREV_SRC" && CARGO_TARGET_DIR="$PREV_T" nice -n 10 cargo build --release -j 3 -p openmind-memory)
fi
echo "memory-schema: previous binary (main 13a296c) $PREV"

cleanup() {
  for p in $(seq 19360 19369); do
    pids=$(ss -ltnpH "sport = :$p" 2>/dev/null | sed -n 's/.*pid=\([0-9]*\).*/\1/p' | sort -u)
    for pid in $pids; do
      exe=$(readlink -f /proc/"$pid"/exe 2>/dev/null || true)
      if [ "$exe" = "$(readlink -f "$BIN")" ] || [ "$exe" = "$(readlink -f "$PREV")" ]; then
        kill -9 "$pid" 2>/dev/null || true
      fi
    done
  done
}
trap cleanup EXIT

OPENMIND_MEMORY_BIN="$BIN" OPENMIND_MEMORY_PREV_BIN="$PREV" \
  nice -n 10 cargo test --release -j 3 -p openmind-memory --test real_schema -- --test-threads=1 --nocapture

env -u GALION_REAL_VPS OPENMIND_REAL_PORT_BASE=19360 OPENMIND_MEMORY_BIN="$BIN" \
  nice -n 10 cargo test --release -j 3 -p openmind-memory --test real_memory -- --test-threads=1 --nocapture

echo "memory-schema: schema report of a fresh DB:"
D=$(mktemp -d); OPENMIND_MEMORY_DATA_DIR="$D" OPENMIND_MEMORY_PORT=19369 timeout 2 "$BIN" >/dev/null 2>&1 || true
"$BIN" schema "$D/vectors.db"; rm -rf "$D"

cleanup
left=$(ss -ltnH | awk '{print $4}' | grep -cE ':1936[0-9]$' || true)
echo "memory-schema: listeners left on 19360-19369: $left"
echo "memory-schema: done $(stamp)"
