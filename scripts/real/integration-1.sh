#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life run for swarm/integration-1 (slot 22: ports 19220-19229, plus the
# real-harness range 19500-19551). Box only; nothing touches the VPS.
#
#   A. galion-realtest stack                 (19500.., no known-failure list
#                                              entries for galion-one any more)
#   B. galion-realtest stack --with-nerve     (nerve-ingest against the merged
#                                              swarm/nerve openmind-nerve)
#   C. cross-branch: swarm/gaia-live `gaia heal-tick` driving the swarm/nerve
#      `openmind-nerve serve --log` binary (POST /event, GET /events) and a
#      throwaway gaia-dummy on 19222, with a per-run random test token.
#
# Usage: scripts/real/integration-1.sh [OUT_DIR]
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
export TZ=Europe/Zagreb
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/integration-1}
BIN=$CARGO_TARGET_DIR/release
OUT=${1:-$CARGO_TARGET_DIR/real-integration-1}
NERVE_PORT=19221
DUMMY_PORT=19222
ALLOW=19220-19229
rm -rf "$OUT"; mkdir -p "$OUT/state" "$OUT/nerve-data"; chmod 700 "$OUT"
ts() { date '+%Y-%m-%dT%H:%M:%S%:z'; }
say() { printf '\n[%s] %s\n' "$(ts)" "$*"; }
NERVE_PID=""; DUMMY_PID=""
dummy_pid() { curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" | jq -r '.pid // empty' 2>/dev/null || true; }
cleanup() {
  local p; p=$(dummy_pid)
  [ -n "$p" ] && kill "$p" 2>/dev/null || true
  [ -n "$DUMMY_PID" ] && kill "$DUMMY_PID" 2>/dev/null || true
  [ -n "$NERVE_PID" ] && kill "$NERVE_PID" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup EXIT

say "build release binaries (nice -n 10, -j 3)"
(cd "$ROOT" && nice -n 10 cargo build -q --release --locked -j 3 -p galion-realtest \
  -p galion-boot -p galion-one -p openmind-memory -p tuner -p galion-desk \
  -p link-watch -p guardian-tick -p persona-plex-relay -p openmind-nerve -p gaia)

rc=0
say "A. galion-realtest stack"
"$BIN/galion-realtest" stack --bin-dir "$BIN" --base-port 19500 --out "$OUT/stack" || rc=1
grep -E '^Checks:' "$OUT/stack/report.md"
say "B. galion-realtest stack --with-nerve"
"$BIN/galion-realtest" stack --bin-dir "$BIN" --base-port 19500 --with-nerve --out "$OUT/stack-nerve" || rc=1
grep -E '^Checks:' "$OUT/stack-nerve/report.md"

say "C. gaia heal-tick (swarm/gaia-live) -> openmind-nerve serve (swarm/nerve)"
for p in $NERVE_PORT $DUMMY_PORT; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then echo "port $p in use" >&2; exit 1; fi
done
umask 077
TOKEN=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
printf '%s\n' "$TOKEN" > "$OUT/trust.token"
printf 'alpha=owner action=heal-apply service=gaia-dummy token=%s\n' "$TOKEN" > "$OUT/approval"
unset TOKEN
NERVE_DATA_DIR="$OUT/nerve-data" NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_GALION_HEALTH_PORT=0 \
  NERVE_TELEGRAM_ENV="$OUT/absent.env" NERVE_DRY_RUN=1 \
  "$BIN/openmind-nerve" serve --listen "127.0.0.1:$NERVE_PORT" --log "$OUT/nerve.jsonl" 2>"$OUT/nerve.stderr" &
NERVE_PID=$!
"$BIN/gaia-dummy" --port "$DUMMY_PORT" 2>>"$OUT/state/gaia-dummy.log" &
DUMMY_PID=$!
for _ in $(seq 50); do
  curl -s --max-time 1 "http://127.0.0.1:$NERVE_PORT/health" >/dev/null &&
    curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" >/dev/null && break
  sleep 0.1
done
echo "nerve GET /health: $(curl -s "http://127.0.0.1:$NERVE_PORT/health" | jq -c '{status, service, appended}')"
tick() { # tick LABEL [approval-file]
  local label=$1 ap=${2:-} r=0
  local args=(heal-tick --compact --service gaia-dummy --port "$DUMMY_PORT" --allow-ports "$ALLOW"
    --nerve "127.0.0.1:$NERVE_PORT" --state "$OUT/state" --trust-file "$OUT/trust.token")
  [ -n "$ap" ] && args+=(--approval-file "$ap")
  "$BIN/gaia" "${args[@]}" -- "$BIN/gaia-dummy" --port "$DUMMY_PORT" > "$OUT/tick-$label.json" || r=$?
  printf '%-12s exit=%s outcome=%s\n' "$label" "$r" "$(jq -r '.result.outcome' "$OUT/tick-$label.json")"
}
tick 1-healthy "$OUT/approval"
kill "$DUMMY_PID"; wait "$DUMMY_PID" 2>/dev/null || true; DUMMY_PID=""
tick 2-no-approval
tick 3-approved "$OUT/approval"
echo "dummy after restart: $(curl -s --max-time 2 "http://127.0.0.1:$DUMMY_PORT/health" | jq -c '{pid}')"
curl -s "http://127.0.0.1:$NERVE_PORT/events?n=100" > "$OUT/nerve-events.json"
echo "nerve events: $(jq length "$OUT/nerve-events.json"), kinds: $(jq -r '[.[].kind] | unique | join(",")' "$OUT/nerve-events.json")"
echo "nerve JSONL lines on disk: $(wc -l < "$OUT/nerve.jsonl")"
if grep -rqF -f "$OUT/trust.token" "$OUT"/tick-*.json "$OUT/nerve.jsonl" "$OUT/nerve-events.json"; then
  echo "LEAK: token found in output" >&2; rc=1
else
  echo "ok: test token not present in tick output or nerve log"
fi
cleanup; NERVE_PID=""; DUMMY_PID=""; trap - EXIT; sleep 0.3
for p in $NERVE_PORT $DUMMY_PORT; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then echo "port $p still open" >&2; rc=1; else echo "port $p closed"; fi
done
say "done rc=$rc; evidence in $OUT"
exit "$rc"
