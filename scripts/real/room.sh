#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Real-life test of room on the box: build release, start real sessions on
# slot-08 ports (19080-19089), probe them, check isolation, tear down.
# Never touches port 4020, the VPS, or any live service.
#
# usage: scripts/real/room.sh            (prints evidence to stdout)
# env:   CARGO_TARGET_DIR (default /workspace/swarm/target/room)
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
cd "$(dirname "$0")/../.."
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/room}
BIN="$CARGO_TARGET_DIR/release/room"
PORT_HOST=19083
PORT_NS=19084
ts() { TZ=Europe/Zagreb date +%Y-%m-%dT%H:%M:%S%:z; }
say() { printf '[%s] %s\n' "$(ts)" "$*"; }

pids=()
work=$(mktemp -d /tmp/room-real.XXXXXX)
cleanup() {
  for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

say "tools: unshare=$(command -v unshare || echo missing) strace=$(command -v strace || echo missing) nsenter=$(command -v nsenter || echo missing)"
if unshare --net -- true 2>/dev/null; then say "unshare --net: ok (privileged)"; else say "unshare --net: refused (needs CAP_SYS_ADMIN)"; fi
if unshare --user --map-root-user --net -- true 2>/dev/null; then say "unshare --user --map-root-user --net: ok"; else say "user namespaces: refused"; fi

say "build: nice -n 10 cargo build --release -p room -j 4"
nice -n 10 cargo build --release -p room -j 4 2>&1 | tail -1
say "binary: $(stat -c '%s bytes' "$BIN"), ELF interpreter: $(readelf -l "$BIN" 2>/dev/null | grep -o "interpreter: [^]]*" || echo none)"

say "cargo tests incl. namespace/strace tests and gated real tests (release, GALION_REAL=1)"
GALION_REAL=1 ROOM_REAL_PORT=19081 nice -n 10 cargo test --release -p room -j 4 -- --nocapture 2>&1 \
  | grep -E '^(test |REAL|SKIP|room |test result)' || true

# A fixture tree laid out like /workspace.
root="$work/root"
mkdir -p "$root/galion-light/room" "$root/galion-light/docs" "$root/galion-src/galion-app/src/api" "$root/outside"
printf '<!doctype html><title>room</title>\n' > "$root/galion-light/room/index.html"
printf '# Operator guide\n' > "$root/galion-light/docs/guide.md"
printf '# hidden\n' > "$root/galion-light/docs/secret-keys.md"
printf '{"name": "galion-app"}\n' > "$root/galion-src/galion-app/package.json"
printf '# private\n' > "$root/outside/private.md"
ln -s "$root/outside/private.md" "$root/galion-src/escape.md"

say "session 1: host loopback 127.0.0.1:$PORT_HOST"
ROOM_BIND=127.0.0.1:$PORT_HOST ROOM_ROOT="$root" "$BIN" --listen 2>"$work/s1.err" &
pids+=($!); p1=$!
for _ in $(seq 50); do grep -q 'listening on' "$work/s1.err" 2>/dev/null && break; sleep 0.1; done
say "  stderr: $(tr '\n' ' ' < "$work/s1.err")"
say "  ss: $(ss -ltn "sport = :$PORT_HOST" | tail -n +2 | awk '{print $4}')"
c() { curl -s -o "$work/body" -w '%{http_code}' "http://127.0.0.1:$PORT_HOST$1"; }
say "  GET /health -> $(c /health) $(cat "$work/body" | tr -d '\n')"
say "  GET /api/objects -> $(c /api/objects) $(wc -c < "$work/body") bytes, $(grep -o '"kind":"' "$work/body" | wc -l) objects, secret listed: $(grep -c secret-keys "$work/body" || true)"
say "  GET /api/read guide -> $(c "/api/read?path=$root/galion-light/docs/guide.md") $(head -c 40 "$work/body" | tr -d '\n')"
say "  GET /api/read secret -> $(c "/api/read?path=$root/galion-light/docs/secret-keys.md") $(cat "$work/body" | tr -d '\n')"
say "  GET /api/read symlink escape -> $(c "/api/read?path=$root/galion-src/escape.md") $(cat "$work/body" | tr -d '\n')"
say "  GET /api/read /etc/passwd -> $(c '/api/read?path=/etc/passwd') $(cat "$work/body" | tr -d '\n')"
say "  GET /api/read bad escape -> $(c '/api/read?path=%a%C3%A9%') (old decoder panicked = abort)"
say "  POST /api/objects -> $(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT_HOST/api/objects")"
say "  GET /health after hostile requests -> $(c /health)"
kill "$p1"; wait "$p1" 2>/dev/null || true
say "  teardown: pid $p1 alive=$(kill -0 "$p1" 2>/dev/null && echo yes || echo no), listener: $(ss -ltn "sport = :$PORT_HOST" | tail -n +2 | wc -l)"

say "session 2: inside a private network namespace on 127.0.0.1:$PORT_NS"
ROOM_BIND=127.0.0.1:$PORT_NS ROOM_ROOT="$root" unshare --user --map-root-user --net -- sh -c 'ip link set lo up && exec "$0" --listen' "$BIN" 2>"$work/s2.err" &
pids+=($!); p2=$!
for _ in $(seq 50); do grep -q 'listening on' "$work/s2.err" 2>/dev/null && break; sleep 0.1; done
say "  stderr: $(tr '\n' ' ' < "$work/s2.err")"
say "  host ss for :$PORT_NS: $(ss -ltn "sport = :$PORT_NS" | tail -n +2 | wc -l) listeners"
say "  host curl :$PORT_NS -> $(curl -s -m 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT_NS/health" || true) (000 = unreachable)"
inner=$(nsenter --target "$p2" --user --net --preserve-credentials -- curl -s -m 2 "http://127.0.0.1:$PORT_NS/health" 2>&1 || echo "nsenter failed")
say "  curl inside the namespace (nsenter) -> $(echo "$inner" | tr -d '\n')"
say "  links inside the namespace: $(nsenter --target "$p2" --user --net --preserve-credentials -- cat /proc/net/dev 2>/dev/null | awk -F: 'NR>2{gsub(/ /,"",$1); printf "%s ", $1}')"
kill "$p2"; wait "$p2" 2>/dev/null || true
say "  teardown: pid $p2 alive=$(kill -0 "$p2" 2>/dev/null && echo yes || echo no)"

say "session 3: no-flag page under strace in an empty netns (must show zero network syscalls)"
ROOM_ROOT="$root" unshare --user --map-root-user --net -- strace -f -e trace=network -o "$work/trace" "$BIN" > "$work/page.json"
say "  page bytes: $(wc -c < "$work/page.json"), network syscalls: $(grep -cE 'socket|bind|listen|connect|accept' "$work/trace" || true)"
ROOM_BIND=127.0.0.1:0 ROOM_ROOT="$root" unshare --user --map-root-user --net -- strace -f -e trace=network -o "$work/trace2" timeout -s TERM 1 "$BIN" --listen 2>/dev/null || true
say "  control (--listen, 1 s): $(grep -oE '^[0-9]+ +(socket|bind|listen)\(' "$work/trace2" | awk '{print $2}' | tr '\n' ' ')"

left=$(pgrep -f "$BIN" || true)
say "leftover room processes: ${left:-none}"
say "done"
