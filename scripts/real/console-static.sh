#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for galion-console-static (B61, slot 34, ports 19340-19349).
# Builds the release binary, serves the built console on 127.0.0.1:19340 and
# probes it with real HTTP. With CONSOLE_STATIC_LIVE=1 it also opens a
# read-only SSH port forward 127.0.0.1:19341 -> VPS 127.0.0.1:8080 (no files
# written on the VPS), uses it as the API upstream, and compares every GET
# with the live console. Only GET requests are ever sent.
#
# env: CONSOLE_STATIC_OUT  pre-built asset set: client/ (SPA build) + routes.txt (gen-routes)
#                          (default /workspace/swarm/artifacts/console-static/out)
#      CONSOLE_STATIC_LIVE=1  compare with live 8080 through the tunnel
#      CONSOLE_STATIC_PROBES  file with extra scanner paths (one per line)
set -uo pipefail
export PATH=/workspace/swarm/bin:$PATH
here=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out=${CONSOLE_STATIC_OUT:-/workspace/swarm/artifacts/console-static/out}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/console-static}
PORT=19340; UP=19341
VPS=${CONSOLE_STATIC_VPS:-root@51.38.137.193}
pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; wait "$p" 2>/dev/null; done; }
trap cleanup EXIT

echo "real: start $(TZ=Europe/Zagreb date -Iseconds)"
[ -f "$out/client/_shell.html" ] || { echo "real: no build at $out (need client/ and routes.txt; see docs/console-static.md)"; exit 2; }
(cd "$here" && CARGO_INCREMENTAL=0 nice -n 10 cargo build --release -j 3 -p console-static -q) || exit 1
bin="$CARGO_TARGET_DIR/release/galion-console-static"
ls -l "$bin" | awk '{print "real: binary bytes " $5}'

upstream_args=()
live=${CONSOLE_STATIC_LIVE:-0}
if [ "$live" = 1 ]; then
  ssh -i "$HOME/.ssh/id_ed25519_vps" -o IdentitiesOnly=yes -o BatchMode=yes -o ExitOnForwardFailure=yes \
      -N -L "127.0.0.1:$UP:127.0.0.1:8080" "$VPS" &
  pids+=($!)
  for _ in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:$UP/favicon.svg" && break; sleep 0.2; done
  upstream_args=(--upstream "127.0.0.1:$UP")
fi

"$bin" --bind "127.0.0.1:$PORT" --root "$out/client" --routes "$out/routes.txt" "${upstream_args[@]}" \
  2> "${TMPDIR:-/tmp}/console-static-real.log" &
pids+=($!)
for _ in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:$PORT/" && break; sleep 0.1; done
head -1 "${TMPDIR:-/tmp}/console-static-real.log"
rss=$(awk '/VmRSS/{print $2}' "/proc/${pids[-1]}/status"); echo "real: front RSS ${rss} kB"

probe() { curl -s -o /dev/null -m 10 -w "%{http_code} %{content_type}" "$1"; }
hdr() { curl -s -o /dev/null -m 10 -D - "$1" | tr -d '\r' | awk -v h="$2" 'tolower($0) ~ "^"h":" {sub(/^[^:]*: */,""); print}'; }

fail=0
printf "\n%-44s %-40s %s\n" "path" "ours (code type | cache)" "live"
check() { # path expect(any|404|200)
  local p=$1 want=$2 ours lv=""
  ours=$(probe "http://127.0.0.1:$PORT$p")
  [ "$live" = 1 ] && lv=$(probe "http://127.0.0.1:$UP$p")
  local cc; cc=$(hdr "http://127.0.0.1:$PORT$p" cache-control)
  printf "%-44s %-40s %s\n" "$p" "$ours | $cc" "$lv"
  if [ "$want" != any ] && [ "${ours%% *}" != "$want" ]; then echo "  FAIL: want $want"; fail=1; fi
}
pages=$(sed -n 's/^page //p' "$out/routes.txt")
for p in $pages; do check "$p" 200; done
for f in /favicon.svg /manifest.webmanifest /og.jpg /__grok/icon-180.png /__grok/install/styles.css; do check "$f" 200; done
asset=$(cd "$out/client" && ls assets/*.js | head -1); check "/$asset" 200
css=$(cd "$out/client" && ls assets/*.css | head -1); check "/$css" 200
echo "-- scanner probes (nerve-capture + dev-server leaks): all must be 404"
probes=(/.env /public/.env /phpinfo.php /public/phpinfo.php /.git/config /.git/HEAD
  /@fs/opt/galion/console-ide/.git/config /opt/galion/console-ide/.git/config
  /package.json /vite.config.ts /src/lib/host.ts /src/lib/env.server.ts /@vite/client
  /node_modules/.vite/ /%2e%2e/%2e%2e/etc/passwd /.vercel/project.json /_shell.html
  /wp-login.php /.aws/credentials /server-status /nope /assets/ /health)
if [ -n "${CONSOLE_STATIC_PROBES:-}" ] && [ -f "$CONSOLE_STATIC_PROBES" ]; then
  while IFS= read -r l; do [ -n "$l" ] && probes+=("$l"); done < "$CONSOLE_STATIC_PROBES"
fi
for p in "${probes[@]}"; do
  ours=$(curl -s -o /dev/null -m 10 -w "%{http_code}" --path-as-is "http://127.0.0.1:$PORT$p")
  printf "%-44s %s\n" "$p" "$ours"
  [ "$ours" = 404 ] || { echo "  FAIL: want 404"; fail=1; }
done
if [ "$live" = 1 ]; then
  echo "-- API GETs through the front vs direct live (read-only)"
  for p in /api/v1/health /api/v1/catalog /api/v1/services /api/v1/protocol /api/v1/markers /api/hermes/health /api/nope /api/.env; do
    a=$(curl -s -m 10 -o /tmp/cs-a -w "%{http_code} %{content_type}" "http://127.0.0.1:$PORT$p")
    b=$(curl -s -m 10 -o /tmp/cs-b -w "%{http_code} %{content_type}" "http://127.0.0.1:$UP$p")
    same=$(cmp -s /tmp/cs-a /tmp/cs-b && echo same-body || echo "body-differs($(wc -c </tmp/cs-a)/$(wc -c </tmp/cs-b))")
    printf "%-44s ours %-28s live %-28s %s\n" "$p" "$a" "$b" "$same"
    rm -f /tmp/cs-a /tmp/cs-b
  done
  if [ "${CONSOLE_STATIC_SSE:-0}" = 1 ]; then
  # Note: the live stream handler calls heartbeatMarker(), which bumps an
  # in-memory marker seq on the console. Opt-in only.
  echo "-- SSE through the front (first frame, 3 s cap)"
  curl -s -N -m 3 "http://127.0.0.1:$PORT/api/v1/markers/stream" | head -c 160; echo
  fi
fi
echo "-- method checks"
printf "POST /agents -> %s\n" "$(curl -s -o /dev/null -w "%{http_code}" -X POST -d x "http://127.0.0.1:$PORT/agents")"
printf "POST /.env -> %s\n" "$(curl -s -o /dev/null -w "%{http_code}" -X POST -d x "http://127.0.0.1:$PORT/.env")"
etag=$(hdr "http://127.0.0.1:$PORT/favicon.svg" etag)
printf "If-None-Match favicon -> %s\n" "$(curl -s -o /dev/null -w "%{http_code}" -H "If-None-Match: $etag" "http://127.0.0.1:$PORT/favicon.svg")"
grep -c ' 404$' "${TMPDIR:-/tmp}/console-static-real.log" | awk '{print "real: access-log 404 lines " $1}'
echo "real: end $(TZ=Europe/Zagreb date -Iseconds) fail=$fail"
exit $fail
