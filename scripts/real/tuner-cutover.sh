#!/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# scripts/real/tuner-cutover.sh: real-life test for the tuner cutover (slot 07).
#  1. builds release tuner for x86_64- and aarch64-unknown-linux-musl (static, rust-lld)
#  2. copies both to $ART (default /workspace/swarm/artifacts/tuner) with SHA256SUMS
#  3. starts the x86_64 musl binary on 127.0.0.1:19070 with the bundled www
#  4. sends every GET in tuner-requests.txt to it, and (LIVE=1, default) the same
#     GETs to the live tuner on VPS 127.0.0.1:8650 over ssh (read-only, no files
#     written there), then diffs every response byte for byte
#  5. runs the GALION_REAL=1 cargo tests against the release binary (ports 19071-19073)
#  6. kills what it started
# Never binds 8650, never touches a unit, never copies anything to the VPS.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../.." && pwd)
export PATH=/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/tuner-cutover}
ART=${ART:-/workspace/swarm/artifacts/tuner}
PORT=${PORT:-19070}
LIVE=${LIVE:-1}
VPS=${VPS:-slavik-vps}
stamp() { TZ=Europe/Zagreb date -Iseconds; }
log() { echo "[$(stamp)] $*"; }

cd "$repo"
for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
  up=$(echo "$t" | tr 'a-z-' 'A-Z_')
  export "CARGO_TARGET_${up}_LINKER=rust-lld"
  log "build $t"
  nice -n 10 cargo build -q -j 4 --release --locked -p tuner --target "$t"
done
mkdir -p "$ART"
install -m 0755 "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/tuner" "$ART/tuner-x86_64-unknown-linux-musl"
install -m 0755 "$CARGO_TARGET_DIR/aarch64-unknown-linux-musl/release/tuner" "$ART/tuner-aarch64-unknown-linux-musl"
(cd "$ART" && sha256sum tuner-x86_64-unknown-linux-musl tuner-aarch64-unknown-linux-musl > SHA256SUMS && cat SHA256SUMS)
for b in "$ART"/tuner-*; do
  if command -v readelf >/dev/null; then
    echo "$(basename "$b"): $(stat -c%s "$b") bytes, $(readelf -h "$b" | sed -n "s/.*Machine: *//p"), interp: $(readelf -l "$b" | grep -c "program interpreter") (0 = static)"
  else echo "$(basename "$b"): $(stat -c%s "$b") bytes"; fi
done
BIN="$ART/tuner-x86_64-unknown-linux-musl"

work=$(mktemp -d); pid=
cleanup() { [ -n "$pid" ] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null; rm -rf "$work"; }
trap cleanup EXIT
log "version: $("$BIN" --version)"
TUNER_ROOT="$repo/crates/tuner/www" "$BIN" --check
env -i TUNER_ADDR="127.0.0.1:$PORT" TUNER_ROOT="$repo/crates/tuner/www" "$BIN" 2>"$work/stderr" &
pid=$!
for _ in $(seq 1 50); do (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null && break; sleep 0.1; done
log "started pid $pid: $(cat "$work/stderr")"
ps -o pid=,rss=,args= -p "$pid"

grep -v '^#' "$here/tuner-requests.txt" | "$here/tuner-capture.sh" "$PORT" > "$work/box.b64"
if [ "$LIVE" = 1 ]; then
  log "live capture: ssh $VPS GET 127.0.0.1:8650 (read-only)"
  grep -v '^#' "$here/tuner-requests.txt" |
    ssh -o BatchMode=yes "$VPS" "bash -c $(printf %q "$(cat "$here/tuner-capture.sh")") capture 8650" > "$work/live.b64"
  live="$work/live.b64"
else
  live="$work/fixtures.b64"
  for f in "$repo"/crates/tuner/tests/fixtures/live/*.http; do
    echo "$(basename "$f" .http) $(base64 -w0 "$f")"
  done > "$live"
fi

fail=0; total=0
printf '%-20s %-26s %7s %7s %s\n' route status box live result
while read -r name b64; do
  total=$((total+1))
  echo "$b64" | base64 -d > "$work/box.http"
  grep "^$name " "$live" | cut -d' ' -f2 | base64 -d > "$work/live.http"
  st=$(head -1 "$work/box.http" | tr -d '\r' | cut -d' ' -f2-)
  if cmp -s "$work/box.http" "$work/live.http"; then r=IDENTICAL; else r=DIFF; fail=$((fail+1)); fi
  printf '%-20s %-26s %7s %7s %s\n' "$name" "$st" "$(stat -c%s "$work/box.http")" "$(stat -c%s "$work/live.http")" "$r"
done < "$work/box.b64"
log "routes: $total, identical: $((total-fail)), diff: $fail (against $([ "$LIVE" = 1 ] && echo "live VPS 8650" || echo fixtures))"

kill "$pid"; wait "$pid" 2>/dev/null || true; pid=
log "stopped; port $PORT listening: $(ss -ltn "sport = :$PORT" | tail -n +2 | wc -l)"

log "cargo GALION_REAL=1 tests against $BIN"
GALION_REAL=1 TUNER_REAL_BIN="$BIN" nice -n 10 cargo test -q -j 4 -p tuner --test real_tuner 2>&1 | grep -E '^(test |running|test result)' || true
[ "$fail" = 0 ]
