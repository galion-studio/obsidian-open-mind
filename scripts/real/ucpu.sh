#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# DESK-ID: openmind.UCPU-1
#
# Real-life test for the universal-CPU gate: build the key services as static
# aarch64 and armv7 musl release binaries, run them under qemu-user on this
# x86_64 box, and probe their real HTTP endpoints. Everything is local:
# loopback only, temp data dirs, hermes-gateway runs --mock (no provider, no
# network, no spend), galion-one runs with ONE_NO_WARM=1.
#
# Ports (swarm slot 13): aarch64 19130-19134, armv7 19135-19139.
#   +0 galion-one  +1 galion-one mouth (TTS)  +2 tuner  +3 openmind-memory
#   +4 hermes-gateway
#
# Needs: qemu-aarch64-static + qemu-arm-static (apt: qemu-user-static), curl,
# zig (ZIG=... or on PATH) for the bundled SQLite C code, rustup targets
# aarch64-unknown-linux-musl and armv7-unknown-linux-musleabihf.
#
#   scripts/real/ucpu.sh            build, start, probe, stop
#   UCPU_SKIP_BUILD=1 scripts/real/ucpu.sh   reuse existing binaries
#
# Exit 0 when every probe passes. Started processes are always killed.
set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$root" || exit 1
export LC_ALL=C
command -v cargo >/dev/null 2>&1 || PATH=$HOME/.cargo/bin:$PATH

case "${CARGO_TARGET_DIR:-}" in
  "") TD=$root/target ;;
  /*) TD=$CARGO_TARGET_DIR ;;
  *) TD=$root/$CARGO_TARGET_DIR ;;
esac
export CARGO_TARGET_DIR=$TD
JOBS=${UCPU_JOBS:-4}
PKGS=(galion-one tuner openmind-memory hermes-gateway)
ARCHES=("aarch64-unknown-linux-musl:qemu-aarch64-static:19130" "armv7-unknown-linux-musleabihf:qemu-arm-static:19135")

ts() { TZ=Europe/Zagreb date '+%Y-%m-%dT%H:%M:%S%:z'; }
say() { printf '[%s] %s\n' "$(ts)" "$*"; }
pass=0
failn=0
ok() { pass=$((pass + 1)); printf '  PASS %s\n' "$*"; }
ko() { failn=$((failn + 1)); printf '  FAIL %s\n' "$*"; }

pids=()
work=$(mktemp -d /tmp/ucpu-real.XXXXXX)
cleanup() {
  local p
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
  sleep 0.5
  for p in "${pids[@]}"; do kill -9 "$p" 2>/dev/null; done
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

for q in qemu-aarch64-static qemu-arm-static curl readelf; do
  command -v "$q" >/dev/null 2>&1 || { say "missing $q"; exit 1; }
done

if [ "${UCPU_SKIP_BUILD:-0}" != 1 ]; then
  # Reuse the gate's zig cc wrappers (build stage sets CC_/AR_/linkers).
  # shellcheck disable=SC1091
  . <(sed -n '/^setup_zig()/,/^}/p' "$root/scripts/ucpu-gate.sh")
  bad() { printf 'setup: %s\n' "$*" >&2; exit 1; }
  zig_bin=""
  setup_zig
  for a in "${ARCHES[@]}"; do
    t=${a%%:*}
    say "build ${PKGS[*]} --target $t"
    args=()
    for p in "${PKGS[@]}"; do args+=(-p "$p"); done
    nice -n 10 cargo build -j "$JOBS" --release --locked "${args[@]}" --target "$t" || exit 1
  done
fi

# wait_port PORT: up to 60 s (emulated start-up is slower).
wait_port() {
  local i
  for i in $(seq 1 120); do
    curl -s -o /dev/null --max-time 1 "http://127.0.0.1:$1/" && return 0
    sleep 0.5
  done
  return 1
}

# probe NAME URL EXPECT [curl args...]: body must contain EXPECT.
probe() {
  local name=$1 url=$2 want=$3 body code
  shift 3
  body=$(curl -s --max-time 20 -w '\n%{http_code}' "$@" "$url") || { ko "$name: curl failed"; return; }
  code=${body##*$'\n'}
  body=${body%$'\n'*}
  printf '    %s -> HTTP %s %s\n' "$url" "$code" "$(printf '%s' "$body" | tr '\n' ' ' | cut -c1-160)"
  case "$body" in
    *"$want"*) ok "$name" ;;
    *) ko "$name (wanted '$want')" ;;
  esac
}

# Not a secret: a throwaway key for the local mock gateway only.
MOCK_KEY=ucpu-real-test-key-not-secret

for a in "${ARCHES[@]}"; do
  t=${a%%:*}; rest=${a#*:}; qemu=${rest%%:*}; base=${rest#*:}
  bin=$TD/$t/release
  say "=== $t under $qemu (ports $base-$((base + 4)))"
  for p in "${PKGS[@]}"; do
    f=$bin/$p
    [ -x "$f" ] || { ko "$t $p: binary missing"; continue; }
    m=$(readelf -hW "$f" | sed -n 's/^ *Machine: *//p')
    n=$(readelf -dW "$f" | grep -c '(NEEDED)')
    printf '    %-16s %8s bytes  machine=%s  NEEDED=%s\n' "$p" "$(stat -c %s "$f")" "$m" "$n"
  done
  # Proof that the host cannot run them natively (no binfmt involved):
  # the gate runs each one explicitly through qemu.
  d=$work/$t
  mkdir -p "$d/one" "$d/www" "$d/mem" "$d/hermes"
  printf '<!doctype html><title>tuner ucpu</title>\n' > "$d/www/index.html"

  GALION_HOME=$d/one ONE_BIND=127.0.0.1:$base ONE_MOUTH_BIND=127.0.0.1:$((base + 1)) ONE_NO_WARM=1 \
    "$qemu" "$bin/galion-one" > "$d/one.log" 2>&1 &
  pids+=($!)
  TUNER_ADDR=127.0.0.1:$((base + 2)) TUNER_ROOT=$d/www \
    "$qemu" "$bin/tuner" > "$d/tuner.log" 2>&1 &
  pids+=($!)
  OPENMIND_MEMORY_HOST=127.0.0.1 OPENMIND_MEMORY_PORT=$((base + 3)) OPENMIND_MEMORY_DATA_DIR=$d/mem \
    "$qemu" "$bin/openmind-memory" > "$d/mem.log" 2>&1 &
  pids+=($!)
  HERMES_HOME=$d/hermes HERMES_RS_CONFIG=$d/hermes/none.yaml HERMES_RS_HOST=127.0.0.1 HERMES_RS_PORT=$((base + 4)) \
    HERMES_RS_STATE_DB=$d/hermes/state.db HERMES_RS_SPEND_FILE=$d/hermes/spend.json HERMES_RS_CRON=0 HERMES_RS_MCP=off \
    API_SERVER_KEY=$MOCK_KEY "$qemu" "$bin/hermes-gateway" --mock > "$d/hermes.log" 2>&1 &
  pids+=($!)

  for port in "$base" "$((base + 2))" "$((base + 3))" "$((base + 4))"; do
    wait_port "$port" || ko "$t: nothing listening on $port"
  done
  # Each listener must belong to a qemu process running a foreign ELF.
  ss -ltnp 2>/dev/null | grep -E "127.0.0.1:($base|$((base + 1))|$((base + 2))|$((base + 3))|$((base + 4))) " |
    sed -E 's/^.*(127\.0\.0\.1:[0-9]+).*users:\(\("([^"]+)".*/    listen \1 by \2/'

  probe "$t galion-one /health" "http://127.0.0.1:$base/health" "ok"
  probe "$t galion-one /me" "http://127.0.0.1:$base/me" "{"
  probe "$t tuner /health" "http://127.0.0.1:$((base + 2))/health" '"ok":true'
  probe "$t tuner /" "http://127.0.0.1:$((base + 2))/" "tuner ucpu"
  probe "$t memory /health" "http://127.0.0.1:$((base + 3))/health" "status"
  probe "$t memory /capture (SQLite write)" "http://127.0.0.1:$((base + 3))/capture" '' \
    -H 'content-type: application/json' \
    -d '{"session_key":"ucpu-'"$t"'","user_content":"ucpu emulated write '"$t"'","assistant_content":"stored under qemu"}'
  probe "$t memory /search/conversations (SQLite read)" "http://127.0.0.1:$((base + 3))/search/conversations" "emulated" \
    -H 'content-type: application/json' -d '{"query":"emulated","session_key":"ucpu-'"$t"'"}'
  probe "$t hermes-gateway /health" "http://127.0.0.1:$((base + 4))/health" "ok"
  probe "$t hermes-gateway /v1/models" "http://127.0.0.1:$((base + 4))/v1/models" '"data"' \
    -H "authorization: Bearer $MOCK_KEY"
  probe "$t hermes-gateway /v1/chat/completions (mock, SQLite session)" "http://127.0.0.1:$((base + 4))/v1/chat/completions" '"choices"' \
    -H "authorization: Bearer $MOCK_KEY" -H 'content-type: application/json' \
    -d '{"model":"mock","messages":[{"role":"user","content":"hello from '"$t"'"}]}'
  # RSS of each emulated service (qemu + guest), from /proc.
  for p in "${pids[@]}"; do
    [ -r "/proc/$p/status" ] || continue
    printf '    pid %s %s VmRSS %s\n' "$p" "$(tr '\0' ' ' < "/proc/$p/cmdline" | awk '{print $2}' | xargs basename)" \
      "$(sed -n 's/^VmRSS: *//p' "/proc/$p/status")"
  done
  for l in "$d"/*.log; do printf '    log %s: %s\n' "$(basename "$l")" "$(head -c 300 "$l" | tr '\n' '|')"; done
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
  sleep 1
  pids=()
done

say "result: $pass passed, $failn failed"
[ "$failn" -eq 0 ]
