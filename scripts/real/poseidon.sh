#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for POSEIDON (openmind-queue). Slot 12, ports 19120-19129:
#   19120  openmind-queue serve (release binary)
#   19122  tuner (release binary, a real box-local HTTP endpoint for the probe dry-run)
# Builds both release binaries, starts them on loopback, drives the queue over
# real HTTP (curl) and with the built-in worker, kills -9 and restarts the
# server, checks persistence, then tears everything down.
# No network beyond 127.0.0.1, no paid calls, nothing on the VPS.
# Output: PASS/FAIL lines; exit 1 if any check failed. Needs curl and jq.
set -u
export PATH=/workspace/swarm/bin:$PATH
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/poseidon}
PORT=19120
TPORT=19122
A="127.0.0.1:$PORT"
U="http://$A"
BIN="$CARGO_TARGET_DIR/release/openmind-queue"
TBIN="$CARGO_TARGET_DIR/release/tuner"
WORK=$(mktemp -d /tmp/poseidon-real.XXXXXX)
DATA="$WORK/data"
pass=0; fail=0
SPID=""; TPID=""

ts() { TZ=Europe/Zagreb date +%Y-%m-%dT%H:%M:%S%:z; }
log() { printf '%s %s\n' "$(ts)" "$*"; }
ok() { pass=$((pass+1)); log "PASS $*"; }
bad() { fail=$((fail+1)); log "FAIL $*"; }
check() { # check NAME ACTUAL EXPECTED
  if [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1: got '$2' want '$3'"; fi
}
cleanup() {
  [ -n "$SPID" ] && kill "$SPID" 2>/dev/null
  [ -n "$TPID" ] && kill "$TPID" 2>/dev/null
  wait 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

# --- HTTP helpers (body to stdout, status to $WORK/code)
req() { curl -s -o "$WORK/body" -w '%{http_code}' -X "$1" -H 'Content-Type: application/json' ${3:+--data "$3"} "$U$2" > "$WORK/code"; cat "$WORK/body"; }
code() { cat "$WORK/code"; }
post() { req POST "$1" "$2"; }
get() { req GET "$1"; }
submit() { post /jobs "$1" | jq -r '.id'; }

start_server() {
  "$BIN" serve --bind "$A" --data "$DATA" --providers "$WORK/providers.json" \
    --lease-ms 1500 --max-attempts 3 --backoff-base-ms 200 --backoff-max-ms 1000 \
    >> "$WORK/server.log" 2>&1 &
  SPID=$!
  for _ in $(seq 1 50); do
    curl -s "$U/healthz" >/dev/null 2>&1 && return 0
    sleep 0.1
  done
  bad "server did not come up"; cat "$WORK/server.log"; exit 1
}

log "== build (release, nice -n 10, -j 4)"
( cd "$ROOT" && nice -n 10 cargo build --release -j 4 -p openmind-queue -p tuner 2>&1 | tail -1 )
[ -x "$BIN" ] && [ -x "$TBIN" ] || { bad "release binaries missing"; exit 1; }
log "binary $BIN $(stat -c %s "$BIN") bytes; $("$BIN" version)"

for p in $PORT $TPORT; do
  if ss -ltn | grep -q "127.0.0.1:$p "; then bad "port $p already in use"; exit 1; fi
done

cat > "$WORK/providers.json" <<JSON
{"version":1,"providers":[
 {"id":"lan-box","kind":"lan","endpoint":"http://192.168.1.50:8080","capabilities":["llm"],"approval":"approved","health":"up"},
 {"id":"runpod-pod","kind":"remote","endpoint":"https://api.runpod.example/pod","capabilities":["llm"],"approval":"pending","health":"unknown"}
]}
JSON

log "== start server on $A"
start_server
h=$(get /healthz)
check "healthz function" "$(echo "$h" | jq -r .function)" "POSEIDON"
log "healthz: $h"
case "$(echo "$h" | jq -r .time)" in *+02:00|*+01:00) ok "Zagreb offset in time";; *) bad "time offset";; esac

log "== 1. ordering / priority (queue order)"
i1=$(submit '{"queue":"order","kind":"echo","priority":1}')
i9a=$(submit '{"queue":"order","kind":"echo","priority":9}')
i5=$(submit '{"queue":"order","kind":"echo","priority":5}')
i9b=$(submit '{"queue":"order","kind":"echo","priority":9}')
got=""
for _ in 1 2 3 4; do
  l=$(post /lease '{"queue":"order","worker":"cli"}')
  id=$(echo "$l" | jq -r .id); tok=$(echo "$l" | jq -r .token)
  got="$got $id"
  post "/jobs/$id/ack" "{\"token\":$tok}" >/dev/null
done
check "lease order prio 9,9,5,1 (FIFO within 9)" "$got" " $i9a $i9b $i5 $i1"
check "empty queue miss" "$(post /lease '{"queue":"order","worker":"cli"}' | jq -r .miss)" "empty"

log "== 2. refusals: money, budget hard stop, quota, bad input"
post /jobs '{"kind":"revolut-payout"}' >/dev/null; check "money kind refused" "$(code)" "403"
post /jobs '{"kind":"echo","nope":1}' >/dev/null; check "unknown field 400" "$(code)" "400"
post /caps '{"scope":"capped","cap":25}' >/dev/null; check "set cap" "$(code)" "200"
for _ in 1 2 3; do submit '{"queue":"cap","agent":"capped","kind":"echo","cost":10}' >/dev/null; done
post /lease '{"queue":"cap","worker":"cli"}' > "$WORK/c1"; post /lease '{"queue":"cap","worker":"cli"}' >/dev/null
check "third lease hits budget hard stop" "$(post /lease '{"queue":"cap","worker":"cli"}' | jq -r .miss)" "budget_hard_stop"
c1id=$(jq -r .id "$WORK/c1"); c1tok=$(jq -r .token "$WORK/c1")
check "over-report clamped to reservation" "$(post "/jobs/$c1id/ack" "{\"token\":$c1tok,\"cost\":999}" | jq -r .charged)" "10"
post /jobs '{"queue":"cap","agent":"capped","kind":"echo","cost":30}' >/dev/null; check "cost over cap refused 402" "$(code)" "402"
s=$(get /stats)
check "capped spent+reserved <= cap" "$(echo "$s" | jq '.agents.capped.spent + .agents.capped.reserved <= 25')" "true"
post /quotas '{"agent":"q1","max_pending":2,"max_inflight":1}' >/dev/null
submit '{"queue":"quota","agent":"q1","kind":"echo"}' >/dev/null; submit '{"queue":"quota","agent":"q1","kind":"echo"}' >/dev/null
post /jobs '{"queue":"quota","agent":"q1","kind":"echo"}' >/dev/null; check "max_pending quota 429" "$(code)" "429"

log "== 3. retry with backoff, dead letters, redrive (built-in worker)"
fl=$(submit '{"queue":"work","kind":"flaky","payload":"3","max_attempts":3}')
fa=$(submit '{"queue":"work","kind":"fail","max_attempts":2}')
ec=$(submit '{"queue":"work","kind":"sleep","payload":"50","priority":5}')
"$BIN" worker --addr "$A" --queue work --worker w1 --idle-exit-ms 2500 > "$WORK/w1.log" 2>&1
cat "$WORK/w1.log"
check "flaky acked on attempt 3" "$(grep -c "job=$fl kind=flaky .*attempt=3 .*result=ack" "$WORK/w1.log")" "1"
check "flaky nacked twice first" "$(grep -c "job=$fl kind=flaky .*result=nack" "$WORK/w1.log")" "2"
check "sleep job (prio 5) ran first" "$(grep -m1 -o 'job=[0-9]*' "$WORK/w1.log")" "job=$ec"
d=$(get /dlq)
check "fail job dead after 2 attempts" "$(echo "$d" | jq "[.dead[]|select(.id==$fa)|.attempts][0]")" "2"
post "/jobs/$fa/redrive" '' >/dev/null; check "redrive" "$(code)" "200"
check "redriven job ready" "$(get "/jobs/$fa" | jq -r .state)" "ready"

log "== 4. lease timeout -> redelivery, stale token fenced"
to=$(submit '{"queue":"timeout","kind":"echo","lease_ms":700,"max_attempts":3}')
l1=$(post /lease '{"queue":"timeout","worker":"slow"}'); t1=$(echo "$l1" | jq -r .token)
sleep 1.2
j=$(get "/jobs/$to")
check "expired lease requeued (delayed by 200 ms backoff, then ready)" "$(echo "$j" | jq -r 'if .state=="ready" or .state=="delayed" then "requeued" else .state end')" "requeued"
check "last_error says expired" "$(echo "$j" | jq -r '.last_error|startswith("lease expired")')" "true"
l2=$(post /lease '{"queue":"timeout","worker":"fast","wait_ms":3000}')
check "redelivered same job" "$(echo "$l2" | jq -r .id)" "$to"
check "attempt 2" "$(echo "$l2" | jq -r .job.attempts)" "2"
post "/jobs/$to/ack" "{\"token\":$t1}" >/dev/null; check "old token fenced 409" "$(code)" "409"
post "/jobs/$to/ack" "{\"token\":$(echo "$l2" | jq -r .token)}" >/dev/null; check "new token acks" "$(code)" "200"
cr=$(submit '{"queue":"crash","kind":"crash","lease_ms":600,"max_attempts":2}')
"$BIN" worker --addr "$A" --queue crash --worker crasher > "$WORK/wc.log" 2>&1; rc=$?
check "worker crashed mid-lease (exit 86)" "$rc" "86"
"$BIN" worker --addr "$A" --queue crash --worker crasher2 >> "$WORK/wc.log" 2>&1
cat "$WORK/wc.log"
sleep 1
check "crashing job dead after 2 expired leases" "$(get "/jobs/$cr" | jq -r '.state + " " + (.attempts|tostring)')" "dead 2"

log "== 5. VRAM pool: provider-gated devices, GPU->LAN->CPU routing, goal"
post /vram/devices '{"id":"lan-gpu-24","kind":"lan","vram_gb":24,"provider":"lan-box"}' > /dev/null
check "lan device planned until provider sync" "$(get /vram | jq -r '.devices[0].device.state')" "planned"
check "sync-providers changed 1" "$(post /vram/sync-providers '' | jq -r .changed)" "1"
r=$(post /vram/devices '{"id":"runpod-a100-80","kind":"remote","vram_gb":80,"provider":"runpod-pod","cents_per_hour":199,"state":"approved"}')
check "paid device demoted to planned" "$(echo "$r" | jq -r .device.state)" "planned"
log "device note: $(echo "$r" | jq -r .note)"
v=$(get /vram)
check "approved/planned/gap GB" "$(echo "$v" | jq -r '.summary|"\(.approved_gb)/\(.planned_gb)/\(.gap_gb)"')" "24/80/1476"
post /jobs '{"queue":"gpu","kind":"echo","vram_gb":48}' >/dev/null; check "48 GB without fallback 503" "$(code)" "503"
g1=$(submit '{"queue":"gpu","kind":"echo","vram_gb":16,"priority":3}')
g2=$(submit '{"queue":"gpu","kind":"echo","vram_gb":16,"priority":2}')
g3=$(submit '{"queue":"gpu","kind":"echo","vram_gb":16,"priority":1,"cpu_fallback":true}')
a1=$(post /lease '{"queue":"gpu","worker":"g"}'); a2=$(post /lease '{"queue":"gpu","worker":"g"}')
check "first 16 GB job on lan-gpu-24" "$(echo "$a1" | jq -r '"\(.id) \(.device)"')" "$g1 lan-gpu-24"
check "second skipped (no room), fallback job offloaded to cpu" "$(echo "$a2" | jq -r '"\(.id) \(.device)"')" "$g3 cpu"
check "vram_held miss" "$(post /lease '{"queue":"gpu","worker":"g"}' | jq -r .miss)" "vram_held"
post "/jobs/$g1/ack" "{\"token\":$(echo "$a1" | jq -r .token)}" >/dev/null
check "freed VRAM goes to waiting job" "$(post /lease '{"queue":"gpu","worker":"g"}' | jq -r '"\(.id) \(.device)"')" "$g2 lan-gpu-24"
check "gaia vram-pool plan is dry-run" "$(post /gaia/dispatch '{"system":"vram-pool","verb":"plan","payload":"8"}' | jq -r .outcome)" "dry-run"
check "gaia money refused" "$(post /gaia/dispatch '{"system":"budget-caps","verb":"run","payload":"RunPod payment top-up"}' | jq -r .outcome)" "refused"
for sys in vram-pool quotas queues budget-caps; do
  r=$(post /gaia/dispatch "{\"system\":\"$sys\",\"verb\":\"status\"}")
  check "gaia $sys status done" "$(echo "$r" | jq -r .outcome)" "done"
  log "gaia $sys: $(echo "$r" | jq -r .summary)"
done

log "== 6. crash-restart persistence (kill -9, torn tail, replay)"
for p in 2 7 4 7 1; do submit "{\"queue\":\"persist\",\"kind\":\"echo\",\"priority\":$p}" >/dev/null; done
lp=$(post /lease '{"queue":"persist","worker":"survivor","lease_ms":60000}')
lpid=$(echo "$lp" | jq -r .id); lptok=$(echo "$lp" | jq -r .token)
before=$(get /stats | jq -c '{depth, acked: .stats.acked, enq: .stats.enqueued, g: .global, vram: .vram.used_gb}')
kill -9 "$SPID"; wait "$SPID" 2>/dev/null; SPID=""
printf 'deadbeef {"t":"enqueue","id":99' >> "$DATA/queue.log"   # a torn write
log "killed -9; appended a torn line; log is $(wc -l < "$DATA/queue.log") lines"
start_server
log "restart: $(grep 'listening' "$WORK/server.log" | tail -1)"
after=$(get /stats | jq -c '{depth, acked: .stats.acked, enq: .stats.enqueued, g: .global, vram: .vram.used_gb}')
check "state identical after kill -9 + replay" "$after" "$before"
check "torn tail cut" "$(get /stats | jq -r '.store.truncated_bytes > 0')" "true"
post "/jobs/$lpid/ack" "{\"token\":$lptok}" >/dev/null; check "pre-crash lease acks after restart" "$(code)" "200"
got=""
for _ in 1 2 3 4; do l=$(post /lease '{"queue":"persist","worker":"cli"}'); got="$got $(echo "$l" | jq -r .job.spec.priority)"; post "/jobs/$(echo "$l" | jq -r .id)/ack" "{\"token\":$(echo "$l" | jq -r .token)}" >/dev/null; done
check "priority order kept after restart" "$got" " 7 4 2 1"
nid=$(submit '{"kind":"echo"}')
check "ids continue after restart (new id > pre-crash id)" "$([ "$nid" -gt "$lpid" ] && echo yes)" "yes"
pre=$(get /stats | jq -c '{depth, enq: .stats.enqueued, acked: .stats.acked}')
post /admin/compact '' > "$WORK/compact"; log "compact: $(cat "$WORK/compact")"
kill "$SPID"; wait "$SPID" 2>/dev/null; SPID=""
start_server
check "state after compaction + restart" "$(get /stats | jq -c '{depth, enq: .stats.enqueued, acked: .stats.acked}')" "$pre"

log "== 7. dry-run probe against a real local endpoint (tuner on $TPORT), read-only GET"
mkdir -p "$WORK/www"; echo '<!doctype html><title>tuner</title>' > "$WORK/www/index.html"
TUNER_ADDR="127.0.0.1:$TPORT" TUNER_ROOT="$WORK/www" "$TBIN" > "$WORK/tuner.log" 2>&1 & TPID=$!
for _ in $(seq 1 30); do curl -s "http://127.0.0.1:$TPORT/" >/dev/null && break; sleep 0.1; done
p1=$(submit "{\"queue\":\"probe\",\"kind\":\"probe\",\"payload\":\"http://127.0.0.1:$TPORT/index.html\",\"max_attempts\":1}")
p2=$(submit "{\"queue\":\"probe\",\"kind\":\"probe\",\"payload\":\"http://127.0.0.1:$PORT/healthz\",\"max_attempts\":1}")
p3=$(submit '{"queue":"probe","kind":"probe","payload":"http://10.0.0.1/","max_attempts":1}')
"$BIN" worker --addr "$A" --queue probe --worker prober --max-jobs 3 > "$WORK/wp.log" 2>&1
cat "$WORK/wp.log"
check "tuner probe 200" "$(grep -c "job=$p1 .*-> 200" "$WORK/wp.log")" "1"
check "self probe 200" "$(grep -c "job=$p2 .*-> 200" "$WORK/wp.log")" "1"
check "non-loopback probe refused" "$(grep -c "job=$p3 .*probe refused" "$WORK/wp.log")" "1"

log "== 8. gated Rust real-binary test (SIGKILL + restart on 127.0.0.1:19121)"
if ( cd "$ROOT" && GALION_REAL=1 nice -n 10 cargo test -q -j 4 -p openmind-queue --test real_binary 2>&1 | tail -3 | tee "$WORK/rt.log" ) && grep -q "1 passed" "$WORK/rt.log"; then ok "real_binary_survives_sigkill"; else bad "real_binary_survives_sigkill"; fi

log "== final stats"
get /stats | jq -c '{depth, stats, global, vram: .vram, store, load}'
log "server log:"; cat "$WORK/server.log"
log "RESULT pass=$pass fail=$fail"
[ "$fail" -eq 0 ]
