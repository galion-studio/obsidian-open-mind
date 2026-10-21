#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test of openmind-nerve on the BOX (slot 10, ports 19100-19109).
# Builds release binaries, starts real box test services (tuner, room),
# starts the nerve against them, then:
#   1. checks the live state / incidents for a dead component (:19103),
#   2. kills tuner -> incident opens; restarts it -> incident resolves,
#   3. writes scanner lines into a simulated reverse-proxy access log,
#   4. floods the nerve's own API -> the DDoS detector fires,
#   5. checks gaia-live compat (POST /event, GET /events, GET /health),
# and tears everything down. CPU-polite: nice, -j 4, short bursts.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
cd "$(dirname "$0")/../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/swarm/target/nerve}"
WARMUP="${WARMUP:-200}"
nice -n 10 cargo build -q -j 4 --release -p openmind-nerve -p tuner -p room
B="$CARGO_TARGET_DIR/release"
W="$(mktemp -d /tmp/nerve-real-XXXX)"
PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true; }
trap cleanup EXIT
stamp() { date '+%Y-%m-%d %H:%M:%S %Z (%:z)'; }
N=http://127.0.0.1:19100
mkdir -p "$W/www" "$W/data" && echo '<h1>box tuner</h1>' > "$W/www/index.html"
cat > "$W/components.json" <<'J'
[{"name":"tuner","port":19101,"path":"/health","gaia":"DEMETER","system":"tuner"},
 {"name":"room","port":19102,"path":"/health","gaia":"HEPHAESTUS","system":"room"},
 {"name":"ghost","port":19103,"path":"/health","gaia":"APOLLO","system":"ghost","note":"nothing listens here on purpose"}]
J
: > "$W/access.log"
start_tuner() { TUNER_ADDR=127.0.0.1:19101 TUNER_ROOT="$W/www" "$B/tuner" 2>>"$W/tuner.log" & TUNER_PID=$!; PIDS+=("$TUNER_PID"); }
start_tuner
ROOM_BIND=127.0.0.1:19102 "$B/room" --listen 2>>"$W/room.log" & PIDS+=($!)
NERVE_ADDR=127.0.0.1:19100 NERVE_INTERVAL=5 NERVE_DATA_DIR="$W/data" NERVE_COMPONENTS="$W/components.json" \
NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_GALION_HEALTH_PORT=0 NERVE_REFLEX=0 NERVE_LISTEN_SCOPE=19100-19109 \
NERVE_ACCESS_LOGS="proxy=$W/access.log@tuner" NERVE_EVENT_LOG="$W/events.jsonl" \
  nice -n 10 "$B/openmind-nerve" serve 2>"$W/nerve.log" & PIDS+=($!); NPID=$!
sleep 12
echo "== $(stamp) start: nerve pid $NPID, work dir $W"
echo "-- GET /health"; curl -s $N/health; echo
echo "-- system after 2 cycles"; curl -s $N/api/v1/state | jq -c '.system | {status,score,grade,pass,open_incidents}'
echo "-- incidents"; curl -s $N/api/v1/incidents | jq -c '.open[] | {id,severity,title}'
echo "-- components"; curl -s $N/api/v1/state | jq -c '.components[] | {name,status,http,reasons}'
echo "== $(stamp) kill tuner (pid $TUNER_PID)"; kill "$TUNER_PID"; sleep 11
curl -s $N/api/v1/incidents | jq -c '.open[] | {id,severity,title,components}'
echo "== $(stamp) restart tuner"; start_tuner; sleep 16
curl -s $N/api/v1/incidents | jq -c '{open: [.open[].title], resolved: [.resolved[] | {title, resolved, duration_s}]}'
echo "== $(stamp) scanner lines in the proxy access log"
for p in /.env /.git/config /wp-login.php /index.html; do echo "203.0.113.9 - - [$(date '+%d/%b/%Y:%H:%M:%S %z')] \"GET $p HTTP/1.1\" 404 0 \"-\" \"scan\"" >> "$W/access.log"; done
sleep 6
curl -s $N/api/v1/ddos | jq -c '.streams[] | select(.stream=="proxy") | {level,requests_60s,scan_hits_60s,top_ips}'
echo "== $(stamp) warm-up: light polling for ${WARMUP}s to build a request baseline"
end=$((SECONDS+WARMUP)); while [ $SECONDS -lt $end ]; do curl -s -o /dev/null $N/healthz; sleep 2; done
curl -s $N/api/v1/ddos | jq -c '.streams[] | select(.stream=="nerve-api") | {level,rps,baseline_rps,baseline_buckets,threshold_rps}'
echo "== $(stamp) FLOOD: 6000 GETs, 8 parallel, against the nerve API"
t0=$(date +%s%N)
seq 6000 | xargs -P 8 -n 50 sh -c 'for i in "$@"; do curl -s -o /dev/null http://127.0.0.1:19100/healthz; done' _
t1=$(date +%s%N); echo "flood took $(( (t1 - t0) / 1000000 )) ms"
sleep 6
echo "-- ddos verdict"; curl -s $N/api/v1/ddos | jq '.streams[] | select(.stream=="nerve-api") | {level,rps,requests_60s,baseline_rps,threshold_rps,factor,shape,top_ips,flood_since,explain}'
echo "-- system + incidents"; curl -s $N/api/v1/state | jq -c '.system | {status,score,grade,pass,ddos_firing,verdict}'
curl -s $N/api/v1/incidents | jq -c '.open[] | {id,severity,title,next}'
echo "-- nerve CPU/RSS during the run"; ps -o pid,rss,time,etime,cmd -p "$NPID" | cat
echo "== $(stamp) gaia-live compat"
curl -s -X POST -H 'Content-Type: application/json' -d '{"source":"gaia-live","kind":"probe","severity":"error","data":{"port":19103,"ok":false}}' $N/event; echo
curl -s "$N/events?n=5"; echo
echo "== $(stamp) quiet 75 s: flood ends, verdict returns to normal"
sleep 75
curl -s $N/api/v1/ddos | jq -c '.streams[] | select(.stream=="nerve-api") | {level,rps,floods_total,peak_rps}'
curl -s $N/api/v1/incidents | jq -c '{open: [.open[].title], resolved: [.resolved[].title]}'
curl -s $N/api/v1/self-eval.md > "$W/self-eval.md"; head -12 "$W/self-eval.md"
echo "-- nerve log"; cat "$W/nerve.log"
echo "== $(stamp) done; tearing down"
