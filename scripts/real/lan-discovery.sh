#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real on-box test for openmind-discovery (swarm slot 11, ports 19110-19119).
#
#   19110/udp  mDNS port used by the test announcer and the browse (not 5353,
#              so no other responder on the LAN answers and avahi is untouched)
#   19111/tcp  stand-in HTTP port for the announced _galion._tcp service (127.0.0.1)
#   19112/tcp  stand-in HTTP port for the announced _exo._tcp service (127.0.0.1)
#   19113/tcp  stand-in HTTP port only the probe finds (ollama kind, 127.0.0.1)
#   19114/tcp  closed on purpose (probe sees it closed)
#
# Flow: build release -> start stubs and the announcer -> find (2 pending rows)
# -> join refused while pending -> deny -> find again (stays rejected) ->
# approve -> join (health up) -> probe 127.0.0.0/24 host .1 only -> refuse a
# public /24 -> print the approvals log -> tear everything down.
# The probe touches only 127.0.0.1. Nothing else on 172.30.0.0/24 is probed.
# Needs: cargo, jq. No Python.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/lan-discovery}
MDNS_PORT=19110
GALION_PORT=19111
EXO_PORT=19112
OLLAMA_PORT=19113
CLOSED_PORT=19114
WORK=$(mktemp -d /tmp/lan-discovery-real.XXXXXX)
REG="$WORK/providers.json"
LOG="$WORK/approvals.log"
PIDS=()

cleanup() {
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
}
trap cleanup EXIT

command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }

step() { printf '\n== %s  %s\n' "$(date '+%Y-%m-%dT%H:%M:%S%:z')" "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

step "build (release, nice, -j 4)"
(cd "$ROOT" && nice -n 10 cargo build -q --release -j 4 -p openmind-discovery --bin openmind-discovery --example health_stub)
BIN="$CARGO_TARGET_DIR/release/openmind-discovery"
STUB="$CARGO_TARGET_DIR/release/examples/health_stub"
D() { "$BIN" "$@" --registry "$REG" --log "$LOG" --by real-test; }
ls -l "$BIN" | awk '{print $5" bytes "$9}'

step "start stand-in HTTP ports on 127.0.0.1:$GALION_PORT, :$EXO_PORT, :$OLLAMA_PORT"
for p in "$GALION_PORT" "$EXO_PORT" "$OLLAMA_PORT"; do "$STUB" "$p" 60 & PIDS+=($!); done

step "start test announcer: _galion._tcp + _exo._tcp on mDNS port $MDNS_PORT"
"$BIN" announce --mdns-port "$MDNS_PORT" --host gaia-real --addr 127.0.0.1 --duration-s 20 \
  --service "galion:gaia-real:$GALION_PORT:id=gaia-real;caps=llm;v=1" \
  --service "exo:gaia-real:$EXO_PORT:v=1" \
  >"$WORK/announce.json" 2>"$WORK/announce.err" & ANN=$!; PIDS+=($ANN)
sleep 0.5

step "find (mDNS browse, 1 query + 1 repeat, 1.5 s)"
OUT=$(D find --mdns-port "$MDNS_PORT" --services galion,exo --timeout-ms 1500)
echo "$OUT" | jq -c '{mdns: .mdns | {iface, port, packets_sent, packets_received, responders}, devices: [.devices[] | {provider_id, status, approval, endpoint, service_type}]}'
[ "$(echo "$OUT" | jq '.mdns.packets_sent')" -le 2 ] || fail "more than 2 query packets"
[ "$(echo "$OUT" | jq '[.devices[] | select(.status=="new" and .approval=="pending")] | length')" = 2 ] || fail "expected 2 new pending devices"

step "list --approval pending"
"$BIN" list --registry "$REG" --approval pending | jq -c '.providers[] | {id, endpoint, approval, health, capabilities}'

step "join galion-gaia-real while pending (must be refused)"
if D join galion-gaia-real 2>"$WORK/join1.err"; then fail "join of a pending row succeeded"; fi
cat "$WORK/join1.err"

step "deny galion-gaia-real"
D deny galion-gaia-real --note "real test: deny first" | jq -c .

step "find again (denied row stays rejected)"
OUT=$(D find --mdns-port "$MDNS_PORT" --services galion,exo --timeout-ms 1500)
echo "$OUT" | jq -c '[.devices[] | {provider_id, status, approval}]'
[ "$(echo "$OUT" | jq -r '.devices[] | select(.provider_id=="galion-gaia-real") | .approval')" = rejected ] || fail "denied row changed"

step "approve galion-gaia-real"
D approve galion-gaia-real --note "real test: approve after deny" | jq -c .

step "join galion-gaia-real (approved: one TCP connect to 127.0.0.1:$GALION_PORT)"
D join galion-gaia-real | tee "$WORK/join2.json" | jq -c .
[ "$(jq -r .health "$WORK/join2.json")" = up ] || fail "join health not up"

step "probe 127.0.0.0/24 host .1 only, ports $GALION_PORT $EXO_PORT $OLLAMA_PORT $CLOSED_PORT, 10/s"
OUT=$(D find --no-mdns --probe 127.0.0.0/24 --probe-hosts 1-1 --rate 10 \
  --probe-ports "galion:$GALION_PORT,exo:$EXO_PORT,ollama:$OLLAMA_PORT,llamacpp:$CLOSED_PORT")
echo "$OUT" | jq -c '{probe, devices: [.devices[] | {provider_id, status, approval, endpoint, verified}]}'
[ "$(echo "$OUT" | jq '.probe.attempts')" = 4 ] || fail "probe attempts"
[ "$(echo "$OUT" | jq '.probe.open')" = 3 ] || fail "probe open count"
[ "$(echo "$OUT" | jq -r '.devices[] | select(.endpoint=="http://127.0.0.1:'"$GALION_PORT"'") | .provider_id + " " + .approval')" = "galion-gaia-real approved" ] || fail "probe hit on the announced port is not the same row"
[ "$(echo "$OUT" | jq -r '[.devices[] | select(.status=="new" and .approval=="pending")] | length')" = 1 ] || fail "expected 1 new pending probe row"

step "probe of a public /24 is refused before any socket"
if "$BIN" find --no-mdns --probe 8.8.8.0/24 --registry "$REG" 2>"$WORK/refuse.err"; then fail "public probe accepted"; fi
head -1 "$WORK/refuse.err"

step "final registry"
"$BIN" list --registry "$REG" | jq -c '.providers[] | {id, approval, health}'

step "approvals log ($LOG)"
cat "$LOG"
if grep -q 'Z"' "$LOG"; then fail "UTC Z timestamp in log"; fi

step "wait for the announcer to finish its 20 s"
wait "$ANN" || true
cat "$WORK/announce.json"
grep -c . "$WORK/announce.err" | sed 's/^/announcer log lines: /'
head -4 "$WORK/announce.err"

step "PASS (work dir $WORK)"
