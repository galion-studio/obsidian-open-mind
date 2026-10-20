#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# REAL-life test of the first end-to-end Gaia path, ON THE BOX ONLY:
#   nerve event -> GAIA core -> DEMETER heal-dry / guardian-tick -> HADES plan
#   -> approval-token check -> healer restart (max 3 per service per hour)
#   -> nerve event recording the result.
#
# It builds release binaries, starts the real `openmind-nerve serve` on
# 127.0.0.1:19031 and the throwaway `gaia-dummy` on 127.0.0.1:19032 (swarm
# slot 03 range 19030-19039), kills the dummy, and lets `gaia heal-tick`
# detect, plan, approve (with a per-run random TEST token) and restart it,
# three times, then shows the fourth restart refused by the cap. It also
# shows the refusals: a live stack port (8420), no approval, a wrong token.
# Nothing here talks to the VPS, systemd or any port outside 19030-19039.
# The test token is never printed; gaia reports only its fnv fingerprint.
#
# Usage: scripts/real/gaia-live.sh [OUT_DIR]
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
export TZ=Europe/Zagreb

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/gaia-live}
NERVE_PORT=19031
DUMMY_PORT=19032
ALLOW=19030-19039
OUT=${1:-/tmp/gaia-live-real-$(date +%Y%m%d-%H%M%S)}
mkdir -p "$OUT/state"
chmod 700 "$OUT"
BIN=$CARGO_TARGET_DIR/release
NERVE_PID=""
FIRST_PID=""

ts() { date '+%Y-%m-%d %H:%M:%S %Z (%:z)'; }
say() { printf '\n[%s] %s\n' "$(ts)" "$*"; }

dummy_pid() { curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" | jq -r '.pid // empty' 2>/dev/null || true; }

cleanup() {
  local p
  p=$(dummy_pid)
  [ -n "$p" ] && kill "$p" 2>/dev/null || true
  [ -n "$FIRST_PID" ] && kill "$FIRST_PID" 2>/dev/null || true
  [ -n "$NERVE_PID" ] && kill "$NERVE_PID" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup EXIT

for p in $NERVE_PORT $DUMMY_PORT; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then
    echo "port $p is already in use; refusing to start" >&2; exit 1
  fi
done

say "build (release, nice -n 10, -j 4, CARGO_TARGET_DIR=$CARGO_TARGET_DIR)"
(cd "$ROOT" && nice -n 10 cargo build -q -j 4 --release -p gaia -p openmind-nerve)
ls -l "$BIN/gaia" "$BIN/gaia-dummy" "$BIN/openmind-nerve" | awk '{print $5, $9}'

say "per-run TEST approval token (random, 128 bits, mode 600, never printed)"
umask 077
TOKEN=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
printf '%s\n' "$TOKEN" > "$OUT/trust.token"
printf 'alpha=owner action=heal-apply service=gaia-dummy token=%s\n' "$TOKEN" > "$OUT/approval"
printf 'alpha=owner action=heal-apply service=gaia-dummy token=%s\n' "$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')" > "$OUT/approval.wrong"
unset TOKEN
ls -l "$OUT/trust.token" "$OUT/approval" | awk '{print $1, $9}'

say "start the real openmind-nerve on 127.0.0.1:$NERVE_PORT"
"$BIN/openmind-nerve" serve --listen "127.0.0.1:$NERVE_PORT" --log "$OUT/nerve.jsonl" 2>"$OUT/nerve.stderr" &
NERVE_PID=$!
say "start gaia-dummy on 127.0.0.1:$DUMMY_PORT"
"$BIN/gaia-dummy" --port "$DUMMY_PORT" 2>>"$OUT/state/gaia-dummy.log" &
FIRST_PID=$!
for _ in $(seq 50); do
  curl -s --max-time 1 "http://127.0.0.1:$NERVE_PORT/health" >/dev/null && curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" >/dev/null && break
  sleep 0.1
done
echo "nerve  GET /health: $(curl -s "http://127.0.0.1:$NERVE_PORT/health")"
echo "dummy  GET /health: $(curl -s "http://127.0.0.1:$DUMMY_PORT/health")"

tick() { # tick LABEL [approval-file] [port]
  local label=$1 ap=${2:-} port=${3:-$DUMMY_PORT} rc=0
  local args=(heal-tick --compact --service gaia-dummy --port "$port" --allow-ports "$ALLOW"
    --nerve "127.0.0.1:$NERVE_PORT" --state "$OUT/state" --trust-file "$OUT/trust.token")
  [ -n "$ap" ] && args+=(--approval-file "$ap")
  "$BIN/gaia" "${args[@]}" -- "$BIN/gaia-dummy" --port "$port" > "$OUT/tick-$label.json" || rc=$?
  printf '%-14s exit=%s outcome=%s detail=%s\n' "$label" "$rc" \
    "$(jq -r '.result.outcome' "$OUT/tick-$label.json")" "$(jq -r '.result.detail' "$OUT/tick-$label.json")"
  return 0
}

say "1. dummy up: gaia heal-tick sees it healthy"
tick 1-healthy "$OUT/approval"

say "2. scope guard: a live stack port (8420) is refused before any probe"
tick 2-live-port "$OUT/approval" 8420

say "3. kill the dummy (pid $FIRST_PID, our own child)"
kill "$FIRST_PID"; wait "$FIRST_PID" 2>/dev/null || true; FIRST_PID=""
echo "dummy GET /health now: $(curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" || echo 'connection refused')"

say "4. detected + planned, but no approval: refused"
tick 4-no-approval
say "5. wrong token: refused"
tick 5-wrong-token "$OUT/approval.wrong"

for n in 1 2 3; do
  say "6.$n approved with the test token: restart $n of 3"
  tick "6.$n-restart" "$OUT/approval"
  p=$(dummy_pid)
  echo "dummy GET /health: $(curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health")"
  say "kill the restarted dummy (pid $p)"
  kill "$p"; sleep 0.3
done

say "7. fourth restart within the hour: capped"
tick 7-capped "$OUT/approval"

say "the step trail of restart 1 (GAIA core, DEMETER, HADES, approval, apply)"
jq -c '.result.steps[] | {step, at, d: (.detail | if .outcome then {outcome, summary, dry_run, plan} elif .healer then {live_suggestion, healer: .healer.summary, guardian_tick: .guardian_tick.summary} elif .hades_execute then {alpha, token_fp, check, released, hades: .hades_execute.summary} elif .state then {state, notes} else . end)}' "$OUT/tick-6.1-restart.json"

say "nerve GET /events?n=100 (kind, severity, ts, short data)"
curl -s "http://127.0.0.1:$NERVE_PORT/events?n=100" > "$OUT/nerve-events.json"
jq -r '.[] | [.ts, .severity, .kind, (.data | {state, stage, reason, restarts_last_hour, pid, health_verified, retry_after_s} | with_entries(select(.value != null)) | tostring)] | @tsv' "$OUT/nerve-events.json"
echo "nerve JSONL lines on disk: $(wc -l < "$OUT/nerve.jsonl")"
echo "restart ledger: $(cat "$OUT/state/restarts.jsonl" | jq -c '{service, ts, pid}' | tr '\n' ' ')"

say "token leak check (trusted token must not appear in any output)"
if grep -rqF -f "$OUT/trust.token" "$OUT"/tick-*.json "$OUT/nerve.jsonl" "$OUT/nerve-events.json" "$OUT/state"; then
  echo "LEAK: token found in output" >&2; exit 1
else
  echo "ok: token not present in tick output, nerve log or state"
fi

say "teardown"
cleanup
NERVE_PID=""; FIRST_PID=""
trap - EXIT
sleep 0.3
for p in $NERVE_PORT $DUMMY_PORT; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then echo "port $p still open" >&2; exit 1; fi
  echo "port $p closed"
done
say "done; evidence in $OUT"
