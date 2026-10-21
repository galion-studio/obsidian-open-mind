#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Real-life test for galion-one (swarm slug one-probe, ports 19050-19059).
# Builds the static x86_64 musl release binary, starts it on spare ports with a
# FIXTURE home (repo fixtures only, never live data), exercises every HTTP
# route, the ONE-PROBE self probe, fail-closed pairing and /api/v1/services,
# then kills everything it started.
#
#   19050 galion-one (main instance)      19051 its mouth
#   19052 memory-gateway stand-in (a second galion-one, fixture TDAI_DATA_DIR)
#   19053 stand-in's mouth                19054-19056 mind/ear/eye: nothing listens
#   19059 stand-in's own memory target: nothing listens
#
# Usage: scripts/real/one-probe.sh   (env: CARGO_TARGET_DIR, NO_BUILD=1)
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
target_dir=${CARGO_TARGET_DIR:-/workspace/swarm/target/one-probe}
triple=x86_64-unknown-linux-musl
bin=$target_dir/$triple/release/galion-one
fix=$repo/crates/galion-one/tests/fixtures

if [ "${NO_BUILD:-0}" != 1 ]; then
  CARGO_TARGET_DIR=$target_dir nice -n 10 cargo build -q -j 4 --release -p galion-one --target $triple
fi

echo "== binary"
readelf -h "$bin" | grep -E 'Type|Machine' | sed 's/^ */  /'
echo "  NEEDED entries: $(readelf -d "$bin" 2>/dev/null | grep -c NEEDED || true)"
echo "  INTERP: $(readelf -l "$bin" | grep -c 'program interpreter' || true)"
echo "  sha256: $(sha256sum "$bin" | cut -c1-16)  size: $(stat -c %s "$bin")"

work=$(mktemp -d "$target_dir/real-one-probe.XXXXXX")
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for pid in "${pids[@]}"; do wait "$pid" 2>/dev/null || true; done
  rm -rf "$work"
}
trap cleanup EXIT

# ---- fixture home (generated here; no live files)
home=$work/home
mkdir -p "$home/one/font" "$home/guardian" "$work/hermes/channels" "$work/mem/data" "$work/memhome"
for page in index nerve admin space; do
  printf '<!doctype html><html><head><title>fixture %s</title></head><body><p>fixture %s</p></body></html>\n' "$page" "$page" >"$home/one/$page.html"
done
printf '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>\n' >"$home/one/mark.svg"
printf 'wOF2fixture' >"$home/one/font/jost.woff2"
printf '[{"kind":"file","place":"box","name":"fixture.txt"}]\n' >"$home/one/catalog.json"
printf '[{"id": "fixture/model", "name": "Fixture model"}]\n' >"$home/one/models-cache.json"
cp "$fix/guardian-fixture.db" "$home/guardian/guardian.db"
cp "$fix/memory-wal-fixture.db" "$work/mem/data/vectors.db"
cp "$fix/memory-wal-fixture.db-wal" "$work/mem/data/vectors.db-wal"
printf '{"saved":[]}\n' >"$work/hermes/modes.json"
: >"$work/hermes.env"
code=FIXTURE42
code_hash=$(printf '%s' "$code" | sha256sum | cut -d' ' -f1)
store=$work/admin-device.json
printf '{"codeHash":"%s","tokenHash":"","tokenHashes":[],"deviceHash":"","fails":0,"lockedUntil":0}\n' "$code_hash" >"$store"
chmod 600 "$store"

common=(env -i PATH=/usr/bin:/bin ONE_NO_WARM=1 ONE_HERMES_ENV="$work/hermes.env"
  GALION_EAR=127.0.0.1:19055 GALION_EYE=127.0.0.1:19056 GALION_MIND=127.0.0.1:19054)

# memory-gateway stand-in: any 2xx /health plus the env the real gateway carries
"${common[@]}" HOME="$work/memhome" TDAI_DATA_DIR="$work/mem/data" \
  ONE_BIND=127.0.0.1:19052 ONE_MOUTH_BIND=127.0.0.1:19053 GALION_MEMORY=127.0.0.1:19059 \
  GALION_HOME="$work/memhome" ONE_HERMES_DIR="$work/memhome/hermes" ONE_DEVICE_STORE="$work/none.json" \
  "$bin" 2>"$work/mem.log" &
pids+=($!)

"${common[@]}" HOME="$work/home" GALION_HOME="$home" ONE_HERMES_DIR="$work/hermes" \
  ONE_DEVICE_STORE="$store" ONE_BIND=127.0.0.1:19050 ONE_MOUTH_BIND=127.0.0.1:19051 \
  GALION_MEMORY=127.0.0.1:19052 "$bin" 2>"$work/one.log" &
pids+=($!)

B=http://127.0.0.1:19050
for _ in $(seq 1 50); do curl -fsS -m 1 "$B/health" >/dev/null 2>&1 && break; sleep 0.2; done
sleep 6 # let sample() run a few ticks (guardian every 5th tick)

show() { # method path [curl args...]
  local m=$1 p=$2; shift 2
  local out ct body
  out=$(curl -sS -m 30 -X "$m" -o "$work/body" -w '%{http_code} %{content_type} %{size_download}' "$@" "$B$p" 2>&1 || true)
  ct=$(echo "$out" | awk '{print $2}')
  case "$ct" in
    application/json*|text/plain*) body=$(head -c 600 "$work/body" | tr '\n' ' ') ;;
    *) body="sha=$(sha256sum <"$work/body" | cut -c1-12)" ;;
  esac
  echo "$m $p -> $out | $body"
}

echo "== unpaired (store present, no cookie)"
for p in / /index.html /nerve /admin /space /mark.svg /font/jost.woff2 /health /link /me \
  /api/v1/awake /api/v1/catalog /api/v1/services /api/v1/analytics /api/v1/analytics/services \
  /session /files /files/abc /model /models /pair /nope; do show GET "$p"; done
show POST /admin/say -H 'Content-Type: application/json' -d '{"text":"hi"}'
show POST /hear --data-binary 'RIFF'
show POST /reply --data-binary 'RIFF'
show POST /files --data-binary 'x'
show POST /model -d '{"model":"x"}'
show POST /voice -d '{"voice":"jarvis"}'

echo "== pairing"
show POST /pair -d '{"code":"WRONG1"}'
cookie=$(curl -sS -m 5 -D - -o /dev/null -X POST -d "{\"code\":\"$(echo $code | tr A-Z a-z)\"}" "$B/pair" \
  | tr -d '\r' | sed -n 's/^Set-Cookie: galion_admin=\([^;]*\);.*/\1/p')
echo "POST /pair (right code, lower-case) -> cookie issued: $([ -n "$cookie" ] && echo yes || echo NO)"
echo "  store after pairing: $(sed -E 's/"[0-9a-f]{64}"/"<hash>"/g' "$store")"
C=(-H "Cookie: galion_admin=$cookie")
show GET /me "${C[@]}"

echo "== paired"
for p in /api/v1/analytics/health /api/v1/analytics/infrastructure /api/v1/analytics/services \
  /api/v1/analytics/agents /api/v1/analytics/users /api/v1/analytics/workforce \
  /api/v1/analytics/overview /session /files /model /models; do show GET "$p" "${C[@]}"; done
full=$(curl -sS -m 10 "${C[@]}" "$B/api/v1/analytics")
echo "GET /api/v1/analytics -> keys: $(echo "$full" | grep -oE '"(at|server|source|health|infrastructure|services|agents|users|workforce|defence|edge|systems|line|nerve|cortex|memory)":' | sort -u | tr -d '":' | tr '\n' ' ')"
echo "  defence: $(echo "$full" | grep -oE '"defence":\[[^]]*\]')"
echo "  memory: $(echo "$full" | grep -oE '"memory":\{[^}]*\}[^}]*\}' | sed "s#$work#<work>#g")"
show POST /files?name=note.txt "${C[@]}" --data-binary 'fixture file body'
id=$(curl -sS -m 5 "${C[@]}" "$B/files" | grep -oE '"id":"[0-9a-f]+"' | head -1 | cut -d'"' -f4)
show GET "/files/$id" "${C[@]}"
show GET /files/zz "${C[@]}"
show POST /model "${C[@]}" -d '{"model":"anything"}'
show POST /voice "${C[@]}" -d '{"voice":"jarvis"}'
show POST /hear "${C[@]}" --data-binary 'RIFF'
show POST /reply "${C[@]}" --data-binary 'RIFF'
show POST /admin/say "${C[@]}" -d '{"text":"hello"}'

echo "== lockout: 5 wrong codes, then the right one"
for i in 1 2 3 4 5; do show POST /pair -d '{"code":"WRONG"}'; done
show POST /pair -d "{\"code\":\"$code\"}"
echo "  fails/lock: $(grep -oE '"fails":[0-9]+,"lockedUntil":[0-9]+' "$store" | sed -E 's/[0-9]{13}/<ms>/')"

echo "== fail closed: device store removed, old cookie"
mv "$store" "$store.gone"
for p in /me /api/v1/analytics /session; do show GET "$p" "${C[@]}"; done
show POST /pair -d "{\"code\":\"$code\"}"
echo "  store recreated by /pair: $([ -e "$store" ] && echo "yes: $(cat "$store")" || echo no)"

echo "== mouth 19051"
B=http://127.0.0.1:19051
show GET /health
show POST /synthesize -d '{"text":"hi"}'
show GET /nope

echo "== ONE-PROBE: dial targets (nothing listens on 8091/8002 on the box)"
ss -ltn | awk '{print $4}' | grep -E ':(8091|8002|8420)$' || echo "  no box listener on 8091/8002/8420"
echo "  stderr one: $(tr '\n' ' ' <"$work/one.log")"
echo "  stderr mem: $(tr '\n' ' ' <"$work/mem.log")"
echo "== done $(TZ=Europe/Zagreb date -Iseconds)"
