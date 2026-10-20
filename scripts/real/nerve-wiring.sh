#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test of the nerve wiring on the BOX (wave-2 slot 30, ports
# 19300-19309). Release builds; nothing touches the VPS; no paid calls
# (hermes-gateway runs --mock, offline).
#
#   19300 openmind-nerve          19301 hermes-gateway --mock (NERVE_URL set)
#   19302 openmind-memory (NERVE_URL set)
#   19305 a stray openmind-memory (no unit, not in the table) -> "unknown owner"
#
# Steps: start-up events; ownership map; traffic with 5xx + slow requests;
# p95 latency with the nerve up, hung (SIGSTOP) and down (killed); kill
# hermes -> probe-down incident with its Gaia owner; restart it with a low
# call cap -> spend-cap events + incident; teardown.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
cd "$(dirname "$0")/../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/swarm/target/nerve-wiring}"
N_REQ="${N_REQ:-300}"
nice -n 10 cargo build -q -j 3 --release -p openmind-nerve -p hermes-gateway -p openmind-memory
B="$CARGO_TARGET_DIR/release"
W="$(mktemp -d /tmp/nerve-wiring-real-XXXX)"
PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do kill -CONT "$p" 2>/dev/null || true; kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true; }
trap cleanup EXIT
stamp() { date '+%Y-%m-%d %H:%M:%S %Z (%:z)'; }
N=http://127.0.0.1:19300 H=http://127.0.0.1:19301 M=http://127.0.0.1:19302
# Throwaway box-only key for the mock gateway (not a real credential; never printed).
KEY="boxtest-$(date +%s%N)-nerve-wiring"
export API_SERVER_KEY="$KEY"
mkdir -p "$W/data" "$W/hermes" "$W/mem" "$W/stray"
cat > "$W/components.json" <<'J'
[{"name":"hermes-rs","port":19301,"path":"/health","gaia":"ARTEMIS","system":"hermes-agents"},
 {"name":"openmind-memory","port":19302,"path":"/health","gaia":"APOLLO","system":"memory-gateway"}]
J
start_nerve() {
  NERVE_ADDR=127.0.0.1:19300 NERVE_INTERVAL=5 NERVE_DATA_DIR="$W/data" NERVE_COMPONENTS="$W/components.json" \
  NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_GALION_HEALTH_PORT=0 NERVE_REFLEX=0 NERVE_LISTEN_SCOPE=19300-19309 \
  NERVE_UNOWNED_PORTS=19305 nice -n 10 "$B/openmind-nerve" serve 2>>"$W/nerve.log" & NPID=$!; PIDS+=("$NPID")
}
start_hermes() { # $1 = daily call cap
  NERVE_URL=$N NERVE_QUEUE=8 NERVE_SLOW_MS=1000 NERVE_HEARTBEAT_S=10 HERMES_HOME="$W/hermes" HERMES_RS_PORT=19301 HERMES_RS_MCP=0 \
  HERMES_RS_DAILY_CALL_CAP="$1" HERMES_RS_MOCK_DELAY_MS=1500 "$B/hermes-gateway" --mock 2>>"$W/hermes.log" & HPID=$!; PIDS+=("$HPID")
}
start_nerve
start_hermes 100000
NERVE_URL=$N NERVE_SLOW_MS=1000 NERVE_HEARTBEAT_S=10 OPENMIND_MEMORY_PORT=19302 OPENMIND_MEMORY_DATA_DIR="$W/mem" "$B/openmind-memory" 2>>"$W/mem.log" & MPID=$!; PIDS+=("$MPID")
OPENMIND_MEMORY_PORT=19305 OPENMIND_MEMORY_DATA_DIR="$W/stray" "$B/openmind-memory" 2>>"$W/stray.log" & SPID=$!; PIDS+=("$SPID")
sleep 12
chat() { curl -s -o /dev/null -w '%{http_code} %{time_total}\n' -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d "{\"model\":\"hermes-agent\",\"messages\":[{\"role\":\"user\",\"content\":\"$1\"}]}" $H/v1/chat/completions; }
recall() { curl -s -o /dev/null -w '%{http_code} %{time_total}\n' -H 'Content-Type: application/json' -d '{"query":"nerve wiring test","session_key":"s1"}' $M/recall; }
p95() { awk '{print $2*1000}' "$1" | sort -n | awk '{a[NR]=$1} END {i=int(NR*0.95); if(i<1)i=1; printf "n=%d p50=%.2fms p95=%.2fms max=%.2fms", NR, a[int(NR*0.5)], a[i], a[NR]}'; }
load() { # $1 = label; N_REQ chats (every 10th mock-fail -> 502) + N_REQ recalls, 4 parallel
  : > "$W/$1-h.txt"; : > "$W/$1-m.txt"
  for i in $(seq "$N_REQ"); do if [ $((i % 10)) -eq 0 ]; then echo mock-fail; else echo "hi $i"; fi; done |
    xargs -P 4 -I{} bash -c "$(declare -f chat); KEY='$KEY' H=$H chat '{}'" >> "$W/$1-h.txt"
  seq "$N_REQ" | xargs -P 4 -I{} bash -c "$(declare -f recall); M=$M recall" >> "$W/$1-m.txt"
  echo "  hermes ($1): $(p95 "$W/$1-h.txt")  codes: $(awk '{print $1}' "$W/$1-h.txt" | sort | uniq -c | tr -s ' ' | tr '\n' ',')"
  echo "  memory ($1): $(p95 "$W/$1-m.txt")  codes: $(awk '{print $1}' "$W/$1-m.txt" | sort | uniq -c | tr -s ' ' | tr '\n' ',')"
}
echo "== $(stamp) started: nerve $NPID hermes $HPID memory $MPID stray $SPID; work dir $W"
echo "-- pushed sources (started events)"; curl -s $N/api/v1/signals | jq -c '.sources[] | {source,total,dropped,recent:[.recent[] | {kind,name,msg}]}'
echo "-- ownership (listen scope 19300-19309)"; curl -s $N/api/v1/cortex | jq -c '.exposure.ownership[]'
echo "-- unmanaged listener finding"; curl -s $N/api/v1/cortex | jq -c '.vulnerabilities[] | select(.id=="unmanaged-listeners") | {severity,title,evidence,gaia}'
curl -s -H 'Content-Type: application/json' -d '{"session_key":"s1","messages":[{"role":"user","content":"the nerve wiring test remembers this line"}]}' $M/capture >/dev/null
echo "== $(stamp) PHASE up: nerve running"; load up
echo "-- 3 slow chats (mock-slow, 1.5 s) and 3 memory 5xx (POST /seed -> 501)"
CP=(); for i in 1 2 3; do chat "mock-slow $i" & CP+=($!); done; wait "${CP[@]}"
for i in 1 2 3; do curl -s -o /dev/null -w '%{http_code}\n' -H 'Content-Type: application/json' -d '{}' $M/seed; done
sleep 11
echo "-- incidents (with owning Gaia function)"; curl -s $N/api/v1/incidents | jq -c '.open[] | {id,severity,cause,title,gaia,owners,signals:[.signals[].msg]}'
echo "-- cortex incident owners"; curl -s $N/api/v1/cortex | jq -c '.incidents[]'
echo "== $(stamp) PHASE hung: nerve SIGSTOPped (accepts connections, never answers)"
kill -STOP "$NPID"; load hung
kill -CONT "$NPID"; sleep 3
echo "== $(stamp) PHASE down: nerve killed"
kill "$NPID"; wait "$NPID" 2>/dev/null || true; load down
echo "== $(stamp) nerve restarted (fresh state)"; start_nerve; sleep 12
echo "-- client counters after hung + down phases (first heartbeats to the new nerve)"
curl -s $N/api/v1/signals | jq -c '.sources[] | {source, heartbeat: ([.recent[] | select(.kind=="heartbeat")] | first | .msg), dropped_metric: .metrics.nerve_client_dropped}'
echo "== $(stamp) PHASE up2: nerve running again (same load, warm processes)"; load up2
echo "-- nerve-side hub counters (per-source rate limit 120/min drops are counted there)"; curl -s $N/api/v1/signals | jq -c '.sources[] | {source,total,dropped}'
echo "== $(stamp) KILL hermes-gateway (pid $HPID)"; kill "$HPID"; wait "$HPID" 2>/dev/null || true; sleep 11
curl -s $N/api/v1/incidents | jq -c '.open[] | {id,severity,cause,title,gaia,owners}'
echo "== $(stamp) restart hermes with HERMES_RS_DAILY_CALL_CAP=5 (spend file already counts > 5 mock calls)"
start_hermes 5; sleep 2
for i in 1 2 3 4 5 6; do chat "after cap $i"; done
sleep 11
echo "-- hermes pushed signals"; curl -s $N/api/v1/signals | jq -c '.sources[] | select(.source=="hermes-rs") | {total,dropped,recent:[.recent[] | {kind,name,value,msg}]}'
echo "-- incidents"; curl -s $N/api/v1/incidents | jq -c '{open: [.open[] | {severity,cause,title,gaia}], resolved: [.resolved[] | {cause,title,gaia,duration_s}]}'
echo "-- cortex incident owners"; curl -s $N/api/v1/cortex | jq -c '.incidents[]'
echo "-- client queue counters (from the 10 s heartbeats: queued/sent/dropped/failed since process start)"
curl -s $N/api/v1/signals | jq -c '.sources[] | {source, heartbeat: ([.recent[] | select(.kind=="heartbeat")] | first | .msg), dropped_metric: .metrics.nerve_client_dropped}'
echo "-- RSS"; ps -o pid,rss,time,comm -p "$NPID,$HPID,$MPID,$SPID" | cat
echo "== $(stamp) done; tearing down"
