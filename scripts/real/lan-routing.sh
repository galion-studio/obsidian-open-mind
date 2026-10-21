#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real on-box test for lan-routing (swarm slot 31, ports 19310-19319).
# No real model runs and nothing paid is called: the "models" are
# mock_openai stand-ins on 127.0.0.1.
#
#   19310/udp  mDNS port for the test announcer and the browse (not 5353)
#   19311/tcp  mock OpenAI server "cpu"       = the CPU fallback (--cpu)
#   19312/tcp  openmind-discovery serve-route (the router under test)
#   19313/tcp  mock OpenAI server "lan-gpu"   = the LAN GPU (announced, approved)
#   19314/tcp  mock OpenAI server "lan-other" = a second LAN GPU (announced, never approved)
#   19315/tcp  mock "lan-gpu" again after it "moved" to a new port
#   19316/tcp  openmind-queue (POSEIDON) reading the same provider registry
#
# Flow: announce 2 devices -> find (2 pending) -> route goes to CPU ->
# approve lan-gpu (still CPU: not joined) -> join up -> routes to LAN ->
# POSEIDON pool shows lan-gpu only -> kill lan-gpu -> router falls back to
# CPU on the same request -> join down -> not even tried -> lan-gpu comes
# back on a new port -> find notes the move, routing still at the old
# endpoint -> re-approve applies it -> join -> LAN again -> deny -> CPU,
# never used again -> lan-other (pending) got 0 requests -> teardown.
# Needs: cargo, curl, jq. No Python.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/lan-routing}
MDNS=19310 CPU=19311 ROUTER=19312 LAN=19313 OTHER=19314 MOVED=19315 QUEUE=19316
WORK=$(mktemp -d /tmp/lan-routing-real.XXXXXX)
REG="$WORK/providers.json"
LOG="$WORK/approvals.log"
PIDS=()

cleanup() {
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
}
trap cleanup EXIT

for t in jq curl; do command -v "$t" >/dev/null || { echo "$t is required" >&2; exit 2; }; done
step() { printf '\n== %s  %s\n' "$(date '+%Y-%m-%dT%H:%M:%S%:z')" "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

step "build (release, nice, -j 3)"
(cd "$ROOT" && CARGO_INCREMENTAL=0 nice -n 10 cargo build -q --release -j 3 \
  -p openmind-discovery --bin openmind-discovery --example mock_openai \
  -p openmind-queue --bin openmind-queue)
BIN="$CARGO_TARGET_DIR/release/openmind-discovery"
MOCK="$CARGO_TARGET_DIR/release/examples/mock_openai"
QBIN="$CARGO_TARGET_DIR/release/openmind-queue"
D() { "$BIN" "$@" --registry "$REG" --log "$LOG" --by real-test; }
ls -l "$BIN" "$QBIN" | awk '{print $5" bytes "$9}'

# chat: one chat completion through the router; prints "provider|attempts|content".
chat() {
  curl -sS -D "$WORK/h" -o "$WORK/b" -X POST "http://127.0.0.1:$ROUTER/v1/chat/completions" \
    -H 'Content-Type: application/json' \
    -d '{"model":"auto","messages":[{"role":"user","content":"ping"}]}'
  local p a c
  p=$(grep -i '^x-galion-provider:' "$WORK/h" | tr -d '\r' | cut -d' ' -f2)
  a=$(grep -i '^x-galion-attempts:' "$WORK/h" | tr -d '\r' | cut -d' ' -f2)
  c=$(jq -r '.choices[0].message.content // .error.type' "$WORK/b")
  echo "$p|$a|$c"
}
expect_chat() {
  local got; got=$(chat)
  echo "chat -> $got"
  [ "$got" = "$1" ] || fail "expected $1"
}
mock_hits() { grep -c 'POST /v1/chat/completions' "$WORK/mock-$1.err" || true; }
announce() { # name port seconds
  "$BIN" announce --mdns-port "$MDNS" --host lan-box --addr 127.0.0.1 --duration-s "$3" \
    --service "llamacpp:$1:$2:caps=llm;vram=$4" >>"$WORK/announce.json" 2>>"$WORK/announce.err" &
  ANNPID=$!; PIDS+=($ANNPID)
}

step "start mock OpenAI servers: cpu :$CPU, lan-gpu :$LAN, lan-other :$OTHER"
"$MOCK" cpu "$CPU" 300 2>"$WORK/mock-cpu.err" & PIDS+=($!)
"$MOCK" lan-gpu "$LAN" 300 2>"$WORK/mock-lan-gpu.err" & LANPID=$!; PIDS+=($LANPID)
"$MOCK" lan-other "$OTHER" 300 2>"$WORK/mock-lan-other.err" & PIDS+=($!)

step "announce _llamacpp._tcp lan-gpu (24 GB) and lan-other (48 GB) on mDNS port $MDNS"
announce lan-gpu "$LAN" 30 24
LANANN=$ANNPID
announce lan-other "$OTHER" 6 48
sleep 0.5

step "find (mDNS browse on :$MDNS)"
OUT=$(D find --mdns-port "$MDNS" --services llamacpp --timeout-ms 1500)
echo "$OUT" | jq -c '[.devices[] | {provider_id, status, approval, endpoint}]'
[ "$(echo "$OUT" | jq '[.devices[] | select(.status=="new" and .approval=="pending")] | length')" = 2 ] || fail "want 2 new pending rows"
"$BIN" list --registry "$REG" | jq -c '.providers[] | {id, approval, health, endpoint, vram_gb}'
G=llamacpp-lan-gpu O=llamacpp-lan-other

step "start serve-route on 127.0.0.1:$ROUTER (--cpu http://127.0.0.1:$CPU)"
"$BIN" serve-route --bind "127.0.0.1:$ROUTER" --registry "$REG" --cpu "http://127.0.0.1:$CPU" \
  --timeout-ms 1000 --duration-s 240 >"$WORK/router.json" 2>"$WORK/router.err" & RPID=$!; PIDS+=($RPID)
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$ROUTER/healthz" >/dev/null && break; sleep 0.1; done
curl -sS "http://127.0.0.1:$ROUTER/v1/route" | jq -c .

step "both LAN rows pending: routed to the CPU fallback"
expect_chat "cpu-local|cpu-local:ok|reply from cpu"

step "approve $G (not joined yet: still CPU)"
D approve "$G" --note "real test: approve" | jq -c .
expect_chat "cpu-local|cpu-local:ok|reply from cpu"

step "join $G (one TCP connect) -> up, routed to the LAN GPU"
D join "$G" | jq -c .
curl -sS "http://127.0.0.1:$ROUTER/v1/route" | jq -c .chain
expect_chat "$G|$G:ok|reply from lan-gpu"
expect_chat "$G|$G:ok|reply from lan-gpu"
curl -sS -D - -o /dev/null "http://127.0.0.1:$ROUTER/v1/models" | grep -i '^x-galion' | tr -d '\r'

step "POSEIDON on :$QUEUE reads the same registry: VRAM pool = approved rows only"
"$QBIN" serve --bind "127.0.0.1:$QUEUE" --data "$WORK/poseidon" --providers "$REG" >"$WORK/queue.out" 2>&1 & QPID=$!; PIDS+=($QPID)
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$QUEUE/healthz" >/dev/null && break; sleep 0.1; done
export POSEIDON_ADDR="127.0.0.1:$QUEUE"
pool() { curl -sS "http://127.0.0.1:$QUEUE/vram" | jq -c '{devices: [.devices[] | {id: .device.id, kind: .device.kind, gb: .device.vram_gb, state: .device.state, healthy: .device.healthy, routable}], approved_gb: .summary.approved_gb}'; }
pool
[ "$(curl -sS "http://127.0.0.1:$QUEUE/vram" | jq -r '[.devices[].device.id] | join(",")')" = "$G" ] || fail "pool must hold the approved row only"
job16() { curl -sS -X POST "http://127.0.0.1:$QUEUE/jobs" -d '{"queue":"gpu","agent":"real","kind":"echo","vram_gb":16'"${1:-}"',"payload":"hi"}'; echo; }
lease() { curl -sS -X POST "http://127.0.0.1:$QUEUE/lease" -d '{"queue":"gpu","worker":"w1","wait_ms":300}' | jq -c '{id, device, miss}'; }
job16 | jq -c '{id: .id, state: .state, error: .error}'
lease

step "kill lan-gpu (server and its announcer): the next request falls back to CPU on its own"
kill "$LANPID" "$LANANN" 2>/dev/null || true; wait "$LANPID" "$LANANN" 2>/dev/null || true
expect_chat "cpu-local|$G:connect-failed,cpu-local:ok|reply from cpu"

step "join $G -> down; the router no longer even tries it; POSEIDON marks it not routable"
D join "$G" | jq -c .
expect_chat "cpu-local|cpu-local:ok|reply from cpu"
curl -sS -X POST "http://127.0.0.1:$QUEUE/vram/sync-providers" | jq -c .
pool
echo "16 GB job without cpu_fallback:"; job16 | jq -c .
echo "16 GB job with cpu_fallback:"; job16 ',"cpu_fallback":true' | jq -c '{id: .id}'; lease

step "lan-gpu comes back on a NEW port :$MOVED and is announced there"
"$MOCK" lan-gpu "$MOVED" 300 2>"$WORK/mock-lan-gpu.err.2" & PIDS+=($!)
announce lan-gpu "$MOVED" 4 24
sleep 0.5
OUT=$(D find --mdns-port "$MDNS" --services llamacpp --timeout-ms 1500)
echo "$OUT" | jq -c '[.devices[] | {provider_id, status, approval, endpoint, seen_endpoint}]'
[ "$(echo "$OUT" | jq -r '.devices[] | select(.provider_id=="'"$G"'") | .seen_endpoint')" = "http://127.0.0.1:$MOVED" ] || fail "move not seen"
"$BIN" list --registry "$REG" | jq -c '.providers[] | select(.id=="'"$G"'") | {id, approval, health, endpoint, pending_endpoint}'
[ "$(jq -r '.providers[] | select(.id=="'"$G"'") | .endpoint' "$REG")" = "http://127.0.0.1:$LAN" ] || fail "moved row changed before re-approval"
D join "$G" | jq -c .
expect_chat "cpu-local|cpu-local:ok|reply from cpu"

step "re-approve $G: the move is applied, health resets until join"
D approve "$G" --note "real test: moved to :$MOVED" | jq -c .
expect_chat "cpu-local|cpu-local:ok|reply from cpu"
D join "$G" | jq -c .
expect_chat "$G|$G:ok|reply from lan-gpu"
curl -sS -X POST "http://127.0.0.1:$QUEUE/vram/sync-providers" | jq -c .
pool

step "deny $G: never used again"
BEFORE=$(grep -c 'POST /v1/chat/completions' "$WORK/mock-lan-gpu.err.2" || true)
D deny "$G" --note "real test: deny" | jq -c .
expect_chat "cpu-local|cpu-local:ok|reply from cpu"
expect_chat "cpu-local|cpu-local:ok|reply from cpu"
if D join "$G" 2>"$WORK/join-denied.err"; then fail "join of a denied row succeeded"; fi
cat "$WORK/join-denied.err"
AFTER=$(grep -c 'POST /v1/chat/completions' "$WORK/mock-lan-gpu.err.2" || true)
echo "lan-gpu (:$MOVED) chat requests before deny $BEFORE, after deny $AFTER"
[ "$BEFORE" = "$AFTER" ] || fail "denied device was used"
curl -sS -X POST "http://127.0.0.1:$QUEUE/vram/sync-providers" | jq -c .
pool
[ "$(curl -sS "http://127.0.0.1:$QUEUE/vram" | jq -r '.devices[0].routable')" = false ] || fail "denied device routable in POSEIDON"

step "the pending lan-other was never used (router or POSEIDON)"
echo "lan-other chat requests: $(mock_hits lan-other)"
[ "$(mock_hits lan-other)" = 0 ] || fail "pending device was used"
curl -sS "http://127.0.0.1:$QUEUE/vram" | jq -e '[.devices[].device.id] | index("'"$O"'") == null' >/dev/null || fail "pending row in pool"

step "router log (stderr)"
cat "$WORK/router.err"
step "mock request counts"
for f in "$WORK"/mock-*.err*; do echo "$(basename "$f"): $(grep -c 'POST /v1/chat' "$f" || true) chat"; done
step "approvals log"
cat "$LOG"
if grep -q 'Z"' "$LOG"; then fail "UTC Z timestamp in log"; fi

step "stop router and POSEIDON"
kill "$RPID" "$QPID" 2>/dev/null || true
step "PASS (work dir $WORK)"
