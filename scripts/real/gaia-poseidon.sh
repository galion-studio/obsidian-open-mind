#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for the gaia-core POSEIDON adapter -> openmind-queue.
# Slot 24, ports 19240-19249:
#   19240  openmind-queue serve (release binary)
#   19249  nothing (used as the "unreachable queue" address)
# Builds the release `gaia` and `openmind-queue`, starts the queue on
# loopback, drives it THROUGH `gaia poseidon ...` (submit, inspect, quota and
# spend-cap refusals, VRAM pool, read-only budget), kills -9 and restarts the
# queue and checks persistence through gaia, then tears everything down.
# Operator actions that Gaia must never do (caps, quotas, devices, the worker
# that leases/acks) are done with the openmind-queue CLI, as an operator would.
# No network beyond 127.0.0.1, no paid calls, nothing on the VPS.
# Output: PASS/FAIL lines; exit 1 if any check failed. Needs curl and jq.
set -u
export PATH=/workspace/swarm/bin:$PATH
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/gaia-poseidon}
PORT=19240
DEAD_PORT=19249
A="127.0.0.1:$PORT"
U="http://$A"
QBIN="$CARGO_TARGET_DIR/release/openmind-queue"
GBIN="$CARGO_TARGET_DIR/release/gaia"
WORK=$(mktemp -d /tmp/gaia-poseidon-real.XXXXXX)
DATA="$WORK/data"
pass=0; fail=0
SPID=""

ts() { TZ=Europe/Zagreb date +%Y-%m-%dT%H:%M:%S%:z; }
log() { printf '%s %s\n' "$(ts)" "$*"; }
ok() { pass=$((pass+1)); log "PASS $*"; }
bad() { fail=$((fail+1)); log "FAIL $*"; }
check() { # check NAME ACTUAL EXPECTED
  if [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: got '$2' want '$3'"; fi
}
cleanup() {
  [ -n "$SPID" ] && kill "$SPID" 2>/dev/null
  wait 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

# gaia CLI: JSON to $WORK/g.json, exit code to $WORK/g.code; prints the summary.
G() {
  "$GBIN" "$@" --compact > "$WORK/g.json" 2> "$WORK/g.err"
  echo $? > "$WORK/g.code"
  log "  gaia $* -> exit $(cat "$WORK/g.code"): $(jq -r '.result.report.summary // .result.state // empty' "$WORK/g.json" 2>/dev/null)"
}
gx() { cat "$WORK/g.code"; }
gj() { jq -r "$1" "$WORK/g.json"; }
Q() { "$QBIN" "$@" --addr "$A"; }
stats() { curl -s "$U/stats"; }

start_server() {
  "$QBIN" serve --bind "$A" --data "$DATA" >> "$WORK/server.log" 2>&1 &
  SPID=$!
  for _ in $(seq 1 50); do
    curl -s "$U/healthz" >/dev/null 2>&1 && return 0
    sleep 0.1
  done
  bad "queue did not come up"; cat "$WORK/server.log"; exit 1
}

log "== build (release, nice -n 10, -j 3)"
( cd "$ROOT" && CARGO_INCREMENTAL=0 nice -n 10 cargo build --release -j 3 -p gaia -p openmind-queue 2>&1 | tail -1 )
[ -x "$QBIN" ] && [ -x "$GBIN" ] || { bad "release binaries missing"; exit 1; }
log "gaia $(stat -c %s "$GBIN") bytes; openmind-queue $(stat -c %s "$QBIN") bytes; $("$QBIN" version)"
for p in $PORT $DEAD_PORT; do
  if ss -ltn | grep -q "127.0.0.1:$p "; then bad "port $p already in use"; exit 1; fi
done
export GAIA_POSEIDON_QUEUE="$A"

log "== ownership: the single config point"
"$GBIN" systems --compact > "$WORK/sys.json"
check "compute systems owned by POSEIDON (FunctionId::COMPUTE)" \
  "$(jq -r '[.result[] | select(.system=="vram-pool" or .system=="quotas" or .system=="queues" or .system=="budget-caps") | .function] | unique | join(",")' "$WORK/sys.json")" "POSEIDON"

log "== unreachable queue: refused, no stand-in fallback"
G poseidon status --queue "127.0.0.1:$DEAD_PORT"
check "unreachable exit" "$(gx)" "3"
check "POSEIDON degraded when queue down" "$(gj .result.state)" "degraded"
G poseidon submit --queue 10.1.2.3:19240 --agent x
check "non-loopback queue refused (usage error)" "$(gx)" "2"

log "== start openmind-queue on $A"
start_server
G poseidon status
check "status exit" "$(gx)" "0"
check "POSEIDON terraforming" "$(gj .result.state)" "terraforming"
check "backend" "$(gj .result.adapter.backend)" "openmind-queue"
check "four systems done" "$(gj '[.result.systems[] | select(.outcome=="done")] | length')" "4"

log "== operator sets caps, quotas and VRAM devices (openmind-queue CLI, not Gaia)"
Q post /caps '{"scope":"agent-a","cap":100}' >/dev/null
Q post /caps '{"scope":"*","cap":1000}' >/dev/null
Q post /quotas '{"agent":"agent-b","max_pending":2,"max_inflight":1}' >/dev/null
Q post /vram/devices '{"id":"gpu0","kind":"gpu","vram_gb":24,"state":"approved"}' >/dev/null
Q post /vram/devices '{"id":"lan-4090","kind":"lan","vram_gb":24,"state":"planned"}' >/dev/null
Q post /vram/devices '{"id":"rented-h100","kind":"remote","vram_gb":80,"cents_per_hour":250,"state":"approved"}' > "$WORK/h100.json"
check "priced device without approved provider stays planned" "$(jq -r "objects | .device.state" "$WORK/h100.json")" "planned"

log "== submit and inspect jobs through gaia"
G poseidon submit --agent agent-a --kind echo --cost 40 --priority 5 --job-payload hello
check "submit 1 done" "$(gx)" "0"; J1=$(gj .result.report.metrics.job_id)
G poseidon submit --agent agent-a --kind echo --cost 40
check "submit 2 done" "$(gx)" "0"; J2=$(gj .result.report.metrics.job_id)
G poseidon job "$J1"
check "inspect job $J1 live" "$(gj .result.report.metrics.live)" "1"
check "inspect job $J1 cost" "$(gj .result.report.metrics.cost)" "40"
check "inspect job $J1 priority" "$(gj .result.report.metrics.priority)" "5"
G poseidon job 999999
check "unknown job not live" "$(gj .result.report.metrics.live)" "0"
G dispatch queues run --payload '{"agent":"agent-a","kind":"echo","cost":1,"token":7}' --queue "$A"
check "unknown job field rejected by adapter" "$(gx)" "5"

log "== Gaia never spends: snapshot budget, run every gaia read command, compare"
before=$(stats | jq -c '{g:.global,a:.agents["agent-a"]|{spent,reserved},acked:.stats.acked,leased:.depth.leased}')
for c in status vram caps quotas dlq "plan-vram 16" "job $J1"; do G poseidon $c; done
for v in run execute; do
  G dispatch budget-caps $v --payload 100000 --queue "$A"
  check "budget-caps $v refused (read-only)" "$(gx)" "4"
  G dispatch quotas $v --payload 99 --queue "$A"
  check "quotas $v refused (read-only)" "$(gx)" "4"
  G dispatch vram-pool $v --payload 80 --queue "$A"
  check "vram-pool $v refused (read-only)" "$(gx)" "4"
done
after=$(stats | jq -c '{g:.global,a:.agents["agent-a"]|{spent,reserved},acked:.stats.acked,leased:.depth.leased}')
check "budget, caps, acks and leases unchanged by gaia" "$after" "$before"
check "agent-a cap still 100" "$(stats | jq -r '.agents["agent-a"].cap')" "100"

log "== operator worker spends 80 units in the queue (lease + ack; Gaia never does this)"
Q worker --worker op-w1 --max-jobs 2 --idle-exit-ms 1500 > "$WORK/worker.log" 2>&1
log "  worker: $(grep -c 'result=ack' "$WORK/worker.log") ack(s)"
G poseidon caps
check "caps: agent-a spent 80" "$(gj '.result.report.metrics["spent:agent-a"]')" "80"
check "caps: agent-a cap 100" "$(gj '.result.report.metrics["cap:agent-a"]')" "100"
check "caps: global cap 1000" "$(gj '.result.report.metrics.global_cap')" "1000"

log "== spend-cap refusal surfaced through gaia"
G poseidon submit --agent agent-a --kind echo --cost 40
check "spend cap -> exit 4" "$(gx)" "4"
check "spend cap -> refused" "$(gj .result.report.outcome)" "refused"
check "spend cap -> 402" "$(gj .result.report.metrics.http_status)" "402"
G poseidon submit --agent agent-a --kind echo --cost 20
check "cost that fits under the cap accepted" "$(gx)" "0"; J3=$(gj .result.report.metrics.job_id)

log "== quota refusal surfaced through gaia"
G poseidon submit --agent agent-b --kind echo
check "agent-b job 1" "$(gx)" "0"
G poseidon submit --agent agent-b --kind echo
check "agent-b job 2" "$(gx)" "0"
G poseidon submit --agent agent-b --kind echo
check "quota -> exit 4" "$(gx)" "4"
check "quota -> 429" "$(gj .result.report.metrics.http_status)" "429"
G poseidon quotas
check "quotas: agent-b pending 2" "$(gj '.result.report.metrics["pending:agent-b"]')" "2"
check "quotas: agent-b max_pending 2" "$(gj '.result.report.metrics["max_pending:agent-b"]')" "2"
check "quotas: refusals counted" "$(gj '.result.report.metrics.quota_refusals')" "1"

log "== money refused (adapter, before the queue)"
G poseidon submit --agent agent-a --kind payout
check "money kind -> exit 4" "$(gx)" "4"

log "== VRAM pool through gaia"
G poseidon vram
check "vram: 24 GB approved" "$(gj .result.report.metrics.approved_gb)" "24"
check "vram: 3 devices listed" "$(gj .result.report.metrics.devices)" "3"
check "vram: planned GB counts lan + h100" "$(gj .result.report.metrics.planned_gb)" "104"
G poseidon plan-vram 16
check "plan 16 GB dry-run" "$(gj .result.report.outcome)" "dry-run"
check "plan 16 GB -> gpu0" "$(gj '.result.report.summary | contains("gpu0")')" "true"
G poseidon plan-vram 48
check "plan 48 GB fits nothing" "$(gj '.result.report.summary | contains("fits no approved device")')" "true"
G poseidon submit --agent agent-c --kind echo --vram-gb 48
check "48 GB job without cpu fallback -> refused" "$(gx)" "4"
check "48 GB job -> 503" "$(gj .result.report.metrics.http_status)" "503"
G poseidon submit --agent agent-c --kind echo --vram-gb 48 --cpu-fallback
check "48 GB job with cpu fallback accepted" "$(gx)" "0"; J4=$(gj .result.report.metrics.job_id)

log "== dead-letter queue through gaia"
G poseidon submit --agent agent-d --queue-name dl --kind fail --max-attempts 1
JD=$(gj .result.report.metrics.job_id)
Q worker --worker op-w2 --queue dl --max-jobs 1 --idle-exit-ms 1500 >> "$WORK/worker.log" 2>&1
G poseidon dlq
check "dlq lists job $JD" "$(gj ".result.report.summary | contains(\"$JD\")")" "true"

log "== restart persistence: kill -9 the queue"
pre=$(stats | jq -c '{depth, g:.global, a:.agents["agent-a"]|{cap,spent}}')
kill -9 "$SPID"; wait "$SPID" 2>/dev/null; SPID=""
G poseidon job "$J3"
check "queue down -> gaia exit 3" "$(gx)" "3"
start_server
post=$(stats | jq -c '{depth, g:.global, a:.agents["agent-a"]|{cap,spent}}')
check "depth, budget and caps survive kill -9" "$post" "$pre"
for j in "$J3" "$J4"; do
  G poseidon job "$j"
  check "job $j still live after restart" "$(gj .result.report.metrics.live)" "1"
done
G poseidon caps
check "spent 80 persisted" "$(gj '.result.report.metrics["spent:agent-a"]')" "80"
G poseidon submit --agent agent-a --kind echo --cost 40
check "spend cap still enforced after restart" "$(gx)" "4"
G poseidon submit --agent agent-b --kind echo
check "quota still enforced after restart" "$(gx)" "4"
G poseidon vram
check "VRAM devices persisted" "$(gj .result.report.metrics.devices)" "3"
G poseidon dlq
check "dead letter persisted" "$(gj ".result.report.summary | contains(\"$JD\")")" "true"
G poseidon status
check "status after restart" "$(gx)" "0"
log "  store: $(stats | jq -c .store)"

log "== teardown"
kill "$SPID" 2>/dev/null; wait "$SPID" 2>/dev/null; SPID=""
for p in $PORT; do
  if ss -ltn | grep -q "127.0.0.1:$p "; then bad "port $p still open"; else ok "port $p closed"; fi
done
log "RESULT pass=$pass fail=$fail"
[ "$fail" -eq 0 ]
