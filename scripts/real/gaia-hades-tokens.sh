#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# REAL-life test of `gaia heal-tick` with hades-gate approvals, ON THE BOX ONLY
# (swarm slot 23, ports 19230-19239; no VPS, no systemd, no live ports):
#
#   probe -> nerve -> GAIA -> DEMETER -> HADES plan -> hades-gate check_gaia
#   (disable switch, admin-safety rule, Ed25519 signature, expiry, scope
#   heal-apply + service + plan SHA-256, single use via the fsynced replay
#   ledger) -> restart (max 3 per hour) -> nerve event.
#
# The admin key pair is made with `hades-gate keygen` in a 0700 dir; tokens
# are minted with `hades-gate mint` over the exact plan bytes that
# `gaia heal-plan --out` writes. No seed or token is ever printed: outputs show
# only the admin fingerprint, nonces and SHA-256 prefixes, and a final grep
# checks that no token string reached any output, log or the nerve.
#
# Usage: scripts/real/gaia-hades-tokens.sh [OUT_DIR]
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
export TZ=Europe/Zagreb

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/gaia-hades-tokens}
NERVE_PORT=19230
DUMMY_PORT=19231
OTHER_PORT=19232
ALLOW=19230-19239
OUT=${1:-/tmp/gaia-hades-tokens-real-$(date +%Y%m%d-%H%M%S)}
umask 077
mkdir -p "$OUT/state" "$OUT/gate" "$OUT/admin" "$OUT/tok" "$OUT/plan"
chmod 700 "$OUT" "$OUT/admin"
BIN=$CARGO_TARGET_DIR/release
NERVE_PID=""
FIRST_PID=""

ts() { date '+%Y-%m-%d %H:%M:%S %Z (%:z)'; }
say() { printf '\n[%s] %s\n' "$(ts)" "$*"; }
dummy_pid() { curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" | jq -r '.pid // empty' 2>/dev/null || true; }
consumed() { grep -c '"event":"consumed"' "$OUT/gate/ledger.jsonl" 2>/dev/null || true; }

cleanup() {
  local p
  p=$(dummy_pid)
  [ -n "$p" ] && kill "$p" 2>/dev/null || true
  [ -n "$FIRST_PID" ] && kill "$FIRST_PID" 2>/dev/null || true
  [ -n "$NERVE_PID" ] && kill "$NERVE_PID" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup EXIT

for p in $NERVE_PORT $DUMMY_PORT $OTHER_PORT; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then
    echo "port $p is already in use; refusing to start" >&2; exit 1
  fi
done

say "build (release, nice -n 10, -j 3, CARGO_TARGET_DIR=$CARGO_TARGET_DIR)"
(cd "$ROOT" && CARGO_INCREMENTAL=0 nice -n 10 cargo build -q -j 3 --release -p gaia -p openmind-nerve -p hades-gate)
ls -l "$BIN/gaia" "$BIN/gaia-dummy" "$BIN/openmind-nerve" "$BIN/hades-gate" | awk '{print $5, $9}'

say "admin side: hades-gate keygen (seed in $OUT/admin, mode 600, never printed); a second 'attacker' key"
"$BIN/hades-gate" keygen --seed "$OUT/admin/admin.seed" --pub "$OUT/gate/admin.pub"
"$BIN/hades-gate" keygen --seed "$OUT/admin/evil.seed" --pub "$OUT/admin/evil.pub" >/dev/null
ls -l "$OUT/admin/admin.seed" "$OUT/gate/admin.pub" | awk '{print $1, $9}'
cat > "$OUT/gate/hades.toml" <<TOML
admin_pubkey = "$OUT/gate/admin.pub"
ledger = "$OUT/gate/ledger.jsonl"
disable_file = "$OUT/gate/DISABLED"
max_ttl_secs = 900
TOML
echo "gate status: $("$BIN/hades-gate" status --config "$OUT/gate/hades.toml")"

say "gaia heal-plan: the exact plan bytes the admin signs"
GOOD=("$BIN/gaia-dummy" --port "$DUMMY_PORT")
"$BIN/gaia" heal-plan --compact --service gaia-dummy --port "$DUMMY_PORT" --allow-ports "$ALLOW" --out "$OUT/plan/good.txt" -- "${GOOD[@]}" | tee "$OUT/plan/good.json" | jq -c '.result | {target, plan_sha256, admin_safety}'
cat "$OUT/plan/good.txt"; echo
echo "hades-gate plan-hash: $("$BIN/hades-gate" plan-hash --plan-file "$OUT/plan/good.txt")"
"$BIN/gaia" heal-plan --compact --service gaia-dummy --port "$OTHER_PORT" --allow-ports "$ALLOW" --out "$OUT/plan/other-port.txt" -- "$BIN/gaia-dummy" --port "$OTHER_PORT" | jq -c '.result | {plan_sha256, admin_safety}'
UNSAFE=("$BIN/gaia-dummy" --port "$DUMMY_PORT" --note=secret)
"$BIN/gaia" heal-plan --compact --service gaia-dummy --port "$DUMMY_PORT" --allow-ports "$ALLOW" --out "$OUT/plan/unsafe.txt" -- "${UNSAFE[@]}" | jq -c '.result | {plan_sha256, admin_safety}'

mint() { # mint NAME SEED TARGET PLAN [extra hades-gate flags]
  local name=$1 seed=$2 target=$3 plan=$4; shift 4
  "$BIN/hades-gate" mint --seed "$seed" --action heal-apply --target "$target" --plan-file "$plan" --out "$OUT/tok/$name" "$@" \
    | jq -c --arg n "$name" '{token: $n, target, plan_sha256: .plan_sha256[0:16], nonce, iat, exp}' | tee -a "$OUT/minted.jsonl"
}
say "admin side: hades-gate mint (tokens written mode 600 to $OUT/tok, never printed)"
NOW=$(date +%s)
mint forged     "$OUT/admin/evil.seed"  gaia-dummy "$OUT/plan/good.txt"
mint wrong-svc  "$OUT/admin/admin.seed" gaia-other "$OUT/plan/good.txt"
mint wrong-plan "$OUT/admin/admin.seed" gaia-dummy "$OUT/plan/other-port.txt"
mint expired    "$OUT/admin/admin.seed" gaia-dummy "$OUT/plan/good.txt" --iat $((NOW - 400)) --ttl 300
mint unsafe     "$OUT/admin/admin.seed" gaia-dummy "$OUT/plan/unsafe.txt"
for n in 1 2 3 4; do mint "valid-$n" "$OUT/admin/admin.seed" gaia-dummy "$OUT/plan/good.txt"; done
# wrong action: hades-gate mint for `rollback` over the same plan
"$BIN/hades-gate" mint --seed "$OUT/admin/admin.seed" --action rollback --target gaia-dummy --plan-file "$OUT/plan/good.txt" --out "$OUT/tok/wrong-action" | jq -c '{token: "wrong-action", action, target, nonce}'
# tampered: valid-4's signature with the last hex digit flipped (written by the script, never printed)
t=$(cat "$OUT/tok/valid-4"); last=${t: -1}; [ "$last" = 0 ] && f=1 || f=0; printf '%s' "${t%?}$f" > "$OUT/tok/tampered"; unset t
ls -l "$OUT/tok" | awk 'NR>1 {print $1, $9}' | tr '\n' ' '; echo

say "start the real openmind-nerve on 127.0.0.1:$NERVE_PORT and gaia-dummy on 127.0.0.1:$DUMMY_PORT"
"$BIN/openmind-nerve" serve --listen "127.0.0.1:$NERVE_PORT" --log "$OUT/nerve.jsonl" 2>"$OUT/nerve.stderr" &
NERVE_PID=$!
"$BIN/gaia-dummy" --port "$DUMMY_PORT" 2>>"$OUT/state/gaia-dummy.log" &
FIRST_PID=$!
for _ in $(seq 50); do
  curl -s --max-time 1 "http://127.0.0.1:$NERVE_PORT/health" >/dev/null && curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" >/dev/null && break
  sleep 0.1
done
echo "nerve GET /health: $(curl -s "http://127.0.0.1:$NERVE_PORT/health")"
echo "dummy GET /health: $(curl -s "http://127.0.0.1:$DUMMY_PORT/health")"

tick() { # tick LABEL [token-name] [port] [unsafe]
  local label=$1 tok=${2:-} port=${3:-$DUMMY_PORT} mode=${4:-} rc=0
  local args=(heal-tick --compact --service gaia-dummy --port "$port" --allow-ports "$ALLOW"
    --nerve "127.0.0.1:$NERVE_PORT" --state "$OUT/state" --hades-config "$OUT/gate/hades.toml")
  [ -n "$tok" ] && args+=(--token-file "$OUT/tok/$tok")
  local start=("$BIN/gaia-dummy" --port "$port")
  [ "$mode" = unsafe ] && start=("${UNSAFE[@]}")
  "$BIN/gaia" "${args[@]}" -- "${start[@]}" > "$OUT/tick-$label.json" || rc=$?
  printf '%-16s exit=%s outcome=%-9s code=%-13s consumed=%s detail=%s\n' "$label" "$rc" \
    "$(jq -r '.result.outcome' "$OUT/tick-$label.json")" \
    "$(jq -r '[.result.steps[] | select(.step=="approval") | .detail.code][0] // "-"' "$OUT/tick-$label.json")" \
    "$(consumed)" "$(jq -r '.result.detail' "$OUT/tick-$label.json")"
  return 0
}

say "1. dummy up: healthy, no token needed or touched"
tick 01-healthy
say "2. scope guard: live stack port 8420 refused before any probe or token"
tick 02-live-port valid-1 8420

say "3. kill the dummy (pid $FIRST_PID, our own child)"
kill "$FIRST_PID"; wait "$FIRST_PID" 2>/dev/null || true; FIRST_PID=""
echo "dummy GET /health now: $(curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health" || echo 'connection refused')"

say "4. refusals (service down, nothing restarted, no token consumed)"
tick 04-no-token
tick 05-forged forged
tick 06-tampered tampered
tick 07-wrong-service wrong-svc
tick 08-wrong-action wrong-action
tick 09-wrong-plan wrong-plan
tick 10-expired expired
say "5. admin safety first: a valid signature over an unsafe plan is still refused"
tick 11-admin-safety unsafe "$DUMMY_PORT" unsafe

say "6. hades-gate disable (no token, no key) beats a valid token; enable"
"$BIN/hades-gate" disable --config "$OUT/gate/hades.toml"
tick 12-disabled valid-1
"$BIN/hades-gate" enable --config "$OUT/gate/hades.toml"

for n in 1 2 3; do
  say "7.$n valid token valid-$n: restart $n of 3"
  tick "13.$n-valid" "valid-$n"
  p=$(dummy_pid)
  echo "dummy GET /health: $(curl -s --max-time 1 "http://127.0.0.1:$DUMMY_PORT/health")"
  say "kill the restarted dummy (pid $p)"
  kill "$p"; sleep 0.3
  if [ "$n" = 1 ]; then
    say "7.1b replay valid-1"
    tick 14-replayed valid-1
  fi
done

say "8. fourth valid token within the hour: capped, token NOT consumed"
tick 15-capped valid-4
n4=$(jq -r 'select(.token=="valid-4") | .nonce' "$OUT/minted.jsonl")
echo "valid-4 nonce $n4 in ledger as consumed: $(grep "\"event\":\"consumed\"" "$OUT/gate/ledger.jsonl" | grep -c "$n4" || true) (expected 0)"
echo "ledger: consumed nonces = $(consumed) (expected 3); refusal/dry-run audit lines = $(grep -vc '"event":"consumed"' "$OUT/gate/ledger.jsonl")"

say "approval step of restart 1"
jq -c '.result.steps[] | select(.step=="approval" or .step=="hades-plan") | {step, at, d: (.detail | if .gate then {admin_fingerprint, token_sha256_16, decision, code, approved, alpha, released} else {plan_sha256, outcome, dry_run} end)}' "$OUT/tick-13.1-valid.json"

say "hades-gate ledger (no token strings; consumed nonces and refusal reasons)"
jq -c '{ts, event, target, nonce: (.nonce // null), reason: (.reason // null), token_sha256_16: (.token_sha256_16 // null)} | with_entries(select(.value != null))' "$OUT/gate/ledger.jsonl"

say "nerve GET /events?n=200 (ts, severity, kind, short data)"
curl -s "http://127.0.0.1:$NERVE_PORT/events?n=200" > "$OUT/nerve-events.json"
jq -r '.[] | [.ts, .severity, .kind, (.data | {state, stage, code, restarts_last_hour, pid, health_verified, nonce, retry_after_s, token_consumed} | with_entries(select(.value != null)) | tostring)] | @tsv' "$OUT/nerve-events.json"
echo "nerve events: $(jq length "$OUT/nerve-events.json"); JSONL lines on disk: $(wc -l < "$OUT/nerve.jsonl")"
bad=$(jq -r '.[].ts' "$OUT/nerve-events.json" | grep -vc '+02:00$' || true)
echo "nerve stamps without +02:00: $bad"
echo "restart ledger: $(jq -c '{service, ts, pid}' "$OUT/state/restarts.jsonl" | tr '\n' ' ')"

say "token / seed leak check"
leak=0
for f in "$OUT"/tok/*; do
  if grep -rqF -- "$(cat "$f")" "$OUT"/tick-*.json "$OUT/nerve.jsonl" "$OUT/nerve-events.json" "$OUT/state" "$OUT/gate/ledger.jsonl"; then
    echo "LEAK: token $(basename "$f") found in output" >&2; leak=1
  fi
done
seedhex=$(cut -d' ' -f2 "$OUT/admin/admin.seed")
grep -rqF -- "$seedhex" "$OUT"/tick-*.json "$OUT/nerve.jsonl" "$OUT/gate" "$OUT/state" && { echo "LEAK: seed" >&2; leak=1; }
unset seedhex
[ "$leak" = 0 ] && echo "ok: no token string or seed in tick output, nerve log, gate ledger or state"

say "teardown"
cleanup
NERVE_PID=""; FIRST_PID=""
trap - EXIT
sleep 0.3
for p in $NERVE_PORT $DUMMY_PORT $OTHER_PORT; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then echo "port $p still open" >&2; exit 1; fi
  echo "port $p closed"
done
say "done; evidence in $OUT"
[ "$leak" = 0 ]
