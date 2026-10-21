#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Real-life run for galion-desk + desk-hermes + link-watch, wired together on
# the BOX (never the VPS), on swarm slot 18 ports 19180-19189:
#
#   19180 galion-desk (release)          desk   (live role :8646)
#   19181 galion-one  (release)          one    (live role :8091)
#   19182 hermes-gateway --mock          hermes (live role :8642, no provider calls)
#   19183 galion-one mouth               voice  (live role :8002)
#   19189 nothing listens                Ollama/laptop, tunnel, console, origin, room, tulu
#
# link-watch (release, loop mode) probes 19189 as Ollama and 19182 as Hermes,
# GETs a few public pages (LINK_WATCH_URLS, rate-limited: one GET per URL per
# 60 s) and writes link.json into the desk's state dir. galion-desk reads that
# link.json and runs the release desk-hermes against a fixture Hermes home.
# Then the gated Rust real tests run against the stack, and everything is
# torn down. Outputs go to $RUN (default $CARGO_TARGET_DIR/real-desk-linkwatch).
#
# Usage: scripts/real/desk-linkwatch.sh            build, start, probe, stop
#        SKIP_BUILD=1 scripts/real/desk-linkwatch.sh
#        KEEP=1 ...                                leave the stack running
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/swarm/target/desk-linkwatch}"
REL="$CARGO_TARGET_DIR/release"
RUN="${RUN:-$CARGO_TARGET_DIR/real-desk-linkwatch}"
URLS="${REAL_URLS:-https://example.com/ https://one.galion.app/}"
PIDS=()

stamp() { TZ=Europe/Zagreb date '+%Y-%m-%dT%H:%M:%S%:z'; }
log() { echo "[$(stamp)] $*"; }

cleanup() {
  if [ "${KEEP:-0}" = "1" ]; then
    log "KEEP=1: leaving pids ${PIDS[*]:-} running"
    return
  fi
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && wait "$pid" 2>/dev/null || true
  done
  log "stopped: ${PIDS[*]:-none}"
  for p in 19180 19181 19182 19183; do
    if ss -ltn "sport = :$p" | grep -q LISTEN; then log "WARN port $p still listening"; fi
  done
}
trap cleanup EXIT

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  log "build (release)"
  (cd "$ROOT" && nice -n 10 cargo build -q -j 4 --release \
     -p galion-desk -p desk-hermes -p link-watch -p hermes-gateway -p galion-one \
     --bins --example fixture_home)
fi

for p in 19180 19181 19182 19183 19189; do
  if ss -ltn "sport = :$p" | grep -q LISTEN; then
    echo "port $p is busy; refusing to start" >&2
    exit 1
  fi
done

rm -rf "$RUN"
mkdir -p "$RUN"/{state,one,gw-home,galion-home/state,logs}
"$REL/examples/fixture_home" "$RUN/hermes" >/dev/null
echo '{"name": "Box test laptop", "gpu": "NVIDIA GeForce RTX 4060 Laptop GPU, 8188 MiB", "cpu": "13th Gen Intel(R) Core(TM) i7-13620H", "ram_gb": 32, "free_gb": 20}' > "$RUN/state/laptop.json"
echo '{"active_agents": 2, "code_version": "box-real"}' > "$RUN/hermes/gateway_state.json"
echo 'jarvis' > "$RUN/one/voice"
CLOSED=127.0.0.1:19189

# Minimal environments (env -i): no provider keys can leak into the children.
BASE_ENV=(PATH=/usr/bin:/bin HOME="$RUN" TZ=Europe/Zagreb OPENMIND_CONFIG="$RUN/no-openmind.toml")

log "start hermes-gateway --mock on 19182"
# hermes-gateway refuses to start without an inbound API key. This mock
# instance gets a throwaway random one that is never printed or stored.
GW_KEY="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d " \n")"
env -i "${BASE_ENV[@]}" HERMES_HOME="$RUN/gw-home" HERMES_RS_HOST=127.0.0.1 HERMES_RS_PORT=19182 \
  HERMES_RS_MCP=off HERMES_RS_CRON=0 API_SERVER_KEY="$GW_KEY" \
  "$REL/hermes-gateway" --mock >"$RUN/logs/hermes.log" 2>&1 &
PIDS+=($!)

log "start galion-one on 19181 (mouth 19183)"
env -i "${BASE_ENV[@]}" GALION_HOME="$RUN/galion-home" ONE_NO_WARM=1 \
  ONE_BIND=127.0.0.1:19181 ONE_MOUTH_BIND=127.0.0.1:19183 \
  GALION_MIND=127.0.0.1:19182 GALION_MOUTH=127.0.0.1:19183 GALION_EAR=$CLOSED GALION_EYE=$CLOSED \
  "$REL/galion-one" >"$RUN/logs/one.log" 2>&1 &
PIDS+=($!)

log "start galion-desk on 19180"
env -i "${BASE_ENV[@]}" DESK_BIND=127.0.0.1:19180 DESK_STATE_DIR="$RUN/state" DESK_ONE_DIR="$RUN/one" \
  DESK_HERMES_DIR="$RUN/hermes" DESK_HERMES_BIN="$REL/desk-hermes" HERMES_HOME="$RUN/hermes" \
  DESK_OLLAMA=$CLOSED DESK_PROBE_HERMES=127.0.0.1:19182 DESK_PROBE_ONE=127.0.0.1:19183 \
  DESK_PROBE_TUNNEL=$CLOSED DESK_PROBE_CONSOLE=$CLOSED DESK_PROBE_ORIGIN=$CLOSED \
  DESK_PROBE_ROOM=$CLOSED DESK_PROBE_TULU=$CLOSED \
  "$REL/galion-desk" >"$RUN/logs/desk.log" 2>&1 &
PIDS+=($!)

wait_port() {
  for _ in $(seq 1 100); do
    if curl -s -o /dev/null --max-time 1 "http://127.0.0.1:$1/health"; then return 0; fi
    sleep 0.1
  done
  echo "port $1 did not come up" >&2
  return 1
}
for p in 19180 19181 19182 19183; do wait_port "$p"; done

log "start link-watch (loop) -> $RUN/state/link.json; URLs: $URLS"
env -i "${BASE_ENV[@]}" LINK_WATCH_OUT="$RUN/state/link.json" LINK_WATCH_LAPTOP="$RUN/state/laptop.json" \
  LINK_WATCH_LOG="$RUN/state/link-log.json" LINK_WATCH_OLLAMA_URL="http://$CLOSED" \
  LINK_WATCH_HERMES_URL=http://127.0.0.1:19182 LINK_WATCH_URLS="$URLS" LINK_WATCH_URLS_EVERY=60 \
  "$REL/link-watch" >"$RUN/logs/link-watch.log" 2>&1 &
PIDS+=($!)

# link-watch fetches the public URLs on its first pass (up to ~20 s).
for _ in $(seq 1 60); do
  if [ -s "$RUN/state/link.json" ] && jq -e '.urls | length >= 1' "$RUN/state/link.json" >/dev/null 2>&1; then break; fi
  sleep 0.5
done
log "listeners:"; ss -ltnp | grep -E ':1918[0-9] ' | sed 's/^/  /'

probe() {
  local method=$1 path=$2 data=${3:-}
  if [ "$method" = POST ]; then
    curl -s -o "$RUN/last.body" -w '%{http_code} %{content_type} %{size_download}B %{time_total}s' \
      --max-time 10 -X POST --data "$data" "http://127.0.0.1:19180$path"
  else
    curl -s -o "$RUN/last.body" -w '%{http_code} %{content_type} %{size_download}B %{time_total}s' \
      --max-time 10 "http://127.0.0.1:19180$path"
  fi
}
log "desk route matrix (GET):"
for p in / /health /healthz /health/x /desk /desk/ /deskx '/desk?x=1' /nope /DESK; do
  printf '  %-10s %s\n' "$p" "$(probe GET "$p")"
done
log "one/hermes health:"
printf '  19181 %s\n' "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:19181/health)"
printf '  19182 %s\n' "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:19182/health)"
printf '  19183 %s\n' "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:19183/health)"

log "link.json from link-watch:"
jq -c '{laptop_port, model, hermes, hermes_ms, france, laptop: .laptop.name, events: (.events|length), urls: [.urls[] | {url, ok, status, ms, checked_at_local, error}]}' "$RUN/state/link.json" | sed 's/^/  /'

log "desk snapshot (selected fields):"
probe GET /desk >/dev/null
jq -c '{foundation, line, hermes, one, tunnel, voice, services: [.services[]|"\(.name)=\(.up)"], board: {studio: .board.studio, gpu: .board.gpu, cpu: .board.cpu, laptop_ram: .board.laptop_ram, hermes_ms: .board.hermes_ms, agents: .board.agents, version: .board.version, france: .board.france}, mindpack: {sessions: .mindpack.mind.sessions, messages: .mindpack.mind.messages, line: .mindpack.mind.line, road: (.mindpack.road|length), tasks: [.mindpack.tasks[].id]}}' "$RUN/last.body" | sed 's/^/  /'

log "desk acts (POST):"
printf '  note   %s\n' "$(probe POST /desk '{"act":"note","text":"real-life note from slot 18"}')"
jq -c '.notes' "$RUN/last.body" | sed 's/^/    notes: /'
printf '  stamp  %s\n' "$(probe POST /desk '{"act":"stamp"}')"
jq -c '.notes[-1].text' "$RUN/last.body" | sed 's/^/    last note: /'
printf '  focus  %s\n' "$(probe POST /desk '{"act":"focus","text":"desk-linkwatch"}')"
jq -c '.focus' "$RUN/last.body" | sed 's/^/    focus: /'
printf '  hold   %s\n' "$(probe POST /desk '{"act":"hold"}')"
jq -c '.hold' "$RUN/last.body" | sed 's/^/    hold: /'
printf '  hold   %s\n' "$(probe POST /desk '{"act":"hold"}')"
jq -c '.hold' "$RUN/last.body" | sed 's/^/    hold: /'
printf '  drop   %s\n' "$(probe POST /desk '{"act":"drop"}')"
jq -c '.notes|length' "$RUN/last.body" | sed 's/^/    notes left: /'
printf '  voice  %s\n' "$(probe POST /desk '{"act":"voice","name":"samantha"}')"
jq -c '.voice' "$RUN/last.body" | sed 's/^/    voice: /'

log "gated Rust real tests against the stack:"
REAL_OUT="$(cd "$ROOT" && GALION_REAL=1 GALION_REAL_DESK=127.0.0.1:19180 GALION_REAL_STATE="$RUN/state" \
  GALION_DESK_LIVE_DESK_PORT=19180 GALION_DESK_LIVE_ONE_PORT=19181 GALION_DESK_LIVE_REQUIRED=1 \
  nice -n 10 cargo test -q -j 4 -p galion-desk --test real_wired --test live_ports -- --include-ignored 2>&1 \
  | grep -E 'test result|panicked|FAILED|SKIP' )" || true
echo "$REAL_OUT" | sed "s/^/  /"
if echo "$REAL_OUT" | grep -q -E "FAILED|panicked" || ! echo "$REAL_OUT" | grep -q "test result"; then REAL_FAIL=1; fi

log "link-watch rate limit (LINK_WATCH_URLS_EVERY=60):"
T0="$(jq -c "[.urls[].checked_unix]" "$RUN/state/link.json")"
sleep 9
T1="$(jq -c "[.urls[].checked_unix]" "$RUN/state/link.json")"
LOOPS="$(jq -r ".at_local" "$RUN/state/link.json")"
echo "  checked_unix at start: $T0; after 9 s (3 more loop passes, last at $LOOPS): $T1"
if [ "$T0" != "$T1" ]; then log "FAIL: URLs re-fetched inside the interval"; REAL_FAIL=1; fi
if [ "${REAL_LONG:-0}" = 1 ]; then
  sleep 55
  T2="$(jq -c "[.urls[].checked_unix]" "$RUN/state/link.json")"
  echo "  after 64 s: $T2"
  jq -c "[.urls[] | {url, ok, status, ms, checked_at_local}]" "$RUN/state/link.json" | sed "s/^/  /"
  if [ "$T2" = "$T1" ]; then log "FAIL: URLs not refreshed after the interval"; REAL_FAIL=1; fi
fi
log "process RSS:"
for pid in "${PIDS[@]}"; do
  printf '  %s %s kB\n' "$(tr '\0' ' ' < /proc/$pid/cmdline | awk '{print $1}' | xargs basename)" "$(awk '/VmRSS/{print $2}' /proc/$pid/status)"
done
if [ "${REAL_FAIL:-0}" = 1 ]; then log "FAIL: real tests failed"; exit 1; fi
log "done: PASS"
