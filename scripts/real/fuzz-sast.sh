#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real on-box test for the fuzz-sast work (swarm slot 25, ports 19250-19259).
#
#   19250/tcp  galion-origin (release) on 127.0.0.1, served by the
#              galion-common HTTP server whose parser the http_parse_head
#              fuzz target covers
#   19251/udp  openmind-discovery announce (release) as the mDNS responder,
#              on a test port (not 5353), whose wire parser mdns_parse covers
#
# Flow: build release binaries and the two replay examples -> start both
# services -> replay the fuzz corpus (fuzz/corpus/<target> when present,
# else the committed fuzz/seeds/<target>) at them -> check both are still
# alive and still answer a well-formed request -> tear down.
# Optional: FUZZ_SECS=N first runs every cargo-fuzz target for N seconds
# (needs nightly + cargo-fuzz; nice -n 15, at most two targets at once).
# Needs: cargo, curl. No Python.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/fuzz-sast/ws}
HTTP_PORT=19250
MDNS_PORT=19251
WORK=$(mktemp -d /tmp/fuzz-sast-real.XXXXXX)
PIDS=()

cleanup() {
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

step() { printf '\n== %s  %s\n' "$(date '+%Y-%m-%dT%H:%M:%S%:z')" "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
corpus() { if [ -d "$ROOT/fuzz/corpus/$1" ] && [ -n "$(ls -A "$ROOT/fuzz/corpus/$1")" ]; then echo "$ROOT/fuzz/corpus/$1"; else echo "$ROOT/fuzz/seeds/$1"; fi; }

if [ -n "${FUZZ_SECS:-}" ]; then
  step "cargo-fuzz: every target for ${FUZZ_SECS}s (two at a time, nice -n 15)"
  "$ROOT/scripts/fuzz.sh" "$FUZZ_SECS"
fi

step "build (release, nice, -j 3)"
(cd "$ROOT" && nice -n 10 cargo build -q --release -j 3 \
  -p galion-origin --bin galion-origin -p galion-common --example replay_http \
  -p openmind-discovery --bin openmind-discovery --example replay_mdns)
R="$CARGO_TARGET_DIR/release"

step "start galion-origin on 127.0.0.1:$HTTP_PORT (empty ORIGIN_ROOT)"
mkdir -p "$WORK/root"
ORIGIN_ADDR="127.0.0.1:$HTTP_PORT" ORIGIN_ROOT="$WORK/root" "$R/galion-origin" >"$WORK/origin.log" 2>&1 & ORIGIN=$!; PIDS+=($ORIGIN)
for _ in $(seq 50); do curl -fsS -o /dev/null "http://127.0.0.1:$HTTP_PORT/health" && break; sleep 0.1; done
curl -fsS "http://127.0.0.1:$HTTP_PORT/health"; echo

step "replay http_parse_head corpus: $(corpus http_parse_head) ($(ls "$(corpus http_parse_head)" | wc -l) files)"
"$R/examples/replay_http" "127.0.0.1:$HTTP_PORT" "$(corpus http_parse_head)" | tee "$WORK/http.json"
kill -0 "$ORIGIN" || fail "galion-origin died during the replay"
code=$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP_PORT/health")
echo "GET /health after replay -> $code"
[ "$code" = 200 ] || fail "health after replay: $code"

step "start openmind-discovery announce on mDNS test port $MDNS_PORT (up to 90 s)"
"$R/openmind-discovery" announce --mdns-port "$MDNS_PORT" --host fuzz-real --addr 127.0.0.1 \
  --duration-s 90 --service "galion:fuzz-real:$HTTP_PORT:v=1" \
  >"$WORK/announce.json" 2>"$WORK/announce.err" & ANN=$!; PIDS+=($ANN)
sleep 1

step "replay mdns_parse corpus: $(corpus mdns_parse) ($(ls "$(corpus mdns_parse)" | wc -l) files)"
"$R/examples/replay_mdns" "127.0.0.1:$MDNS_PORT" "$(corpus mdns_parse)" "_galion._tcp.local." | tee "$WORK/mdns.json"
kill -0 "$ANN" || fail "announcer died during the replay"
echo "announcer log: $(grep -c "query from" "$WORK/announce.err" || true) queries answered, $(grep -c "rate-limited" "$WORK/announce.err" || true) rate-limited"

step "tear down"
kill "$ORIGIN" "$ANN" 2>/dev/null || true
wait "$ANN" 2>/dev/null || true
echo "PASS"
