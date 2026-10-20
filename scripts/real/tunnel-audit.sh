#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Real-life test for tunnel-lint (slot 35, ports 19350-19359 only).
# Builds release tunnel-lint + tuner, starts real tuner origins on 19350-19353,
# rewrites the secrets-free VPS ingress copy onto those ports (dead VPS ports
# map onto 19354-19359, left closed), lints live, stages, kills an origin,
# revives a dead one, and tears everything down. Nothing touches the VPS.
# Times are Europe/Zagreb with an explicit offset.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/tunnel-audit}
export CARGO_INCREMENTAL=0
FIX=$ROOT/crates/tunnel-lint/tests/fixtures
WORK=$(mktemp -d /tmp/tunnel-audit-real.XXXXXX)
PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done; rm -rf "$WORK"; }
trap cleanup EXIT
ts() { TZ=Europe/Zagreb date -Iseconds; }
step() { printf '\n== %s %s\n' "$(ts)" "$*"; }
fail() { echo "FAIL: $*"; exit 1; }

step "build (release)"
( cd "$ROOT" && nice -n 10 cargo build --release -j 3 -p tunnel-lint -p tuner 2>&1 | tail -1 )
TL=$CARGO_TARGET_DIR/release/tunnel-lint
TU=$CARGO_TARGET_DIR/release/tuner
ls -l "$TL" "$TU" | awk '{print $5, $9}'

step "ports 19350-19359 must be free"
if ss -ltn | awk '{print $4}' | grep -qE ':1935[0-9]$'; then ss -ltn | grep -E ':1935[0-9] '; fail "slot ports busy"; fi
echo free

step "box config from the VPS copy (live 8671/8091/8080/8650 -> 19350-19353, dead -> 19354-19359)"
cp "$FIX/vps-config-20261020.yml" "$WORK/config.yml"
sed -i -E 's#:8671\b#:19350#; s#:8091\b#:19351#; s#:8080\b#:19352#g; s#:8650\b#:19353#' "$WORK/config.yml"
i=0
for p in 3004 3005 3010 3011 3015 3020 3080 3333 4000 4001 4003 4004 4020 4042 4050 4060 4444 5530 7681 8000 8889; do
  sed -i -E "s#:$p\b#:$((19354 + i % 6))#g" "$WORK/config.yml"; i=$((i + 1))
done
chmod 666 "$WORK/config.yml"   # same mode as the VPS file, to exercise the warning
grep -oE 'service: http://[^ ]+' "$WORK/config.yml" | sort | uniq -c

step "start real tuner origins on 19350-19353"
mkdir -p "$WORK/www"; echo '<!doctype html><title>slot35</title>ok' > "$WORK/www/index.html"
for port in 19350 19351 19352 19353; do
  TUNER_ADDR=127.0.0.1:$port TUNER_ROOT=$WORK/www nice -n 10 "$TU" 2>>"$WORK/tuner.log" & PIDS+=($!)
done
for _ in $(seq 1 50); do [ "$(ss -ltn | grep -cE '127.0.0.1:1935[0-3] ')" = 4 ] && break; sleep 0.1; done
for port in 19350 19351 19352 19353; do echo "GET :$port -> $(curl -s -o /dev/null -m 3 -w '%{http_code}' http://127.0.0.1:$port/)"; done

step "lint live (expect exit 1: 33 closed routes on 6 ports, 5 open)"
set +e; "$TL" "$WORK/config.yml" > "$WORK/out1.txt"; rc=$?; set -e
cat "$WORK/out1.txt"; echo "exit=$rc"
[ $rc = 1 ] || fail "want exit 1"
grep -q 'summary: 39 rules, 33 closed routes on 6 ports \[19354, 19355, 19356, 19357, 19358, 19359\]' "$WORK/out1.txt" || fail summary
[ "$(grep -c '^ *[0-9]* OPEN' "$WORK/out1.txt")" = 5 ] || fail "want 5 open"
grep -q 'world-writable (mode 666)' "$WORK/out1.txt" || fail "want world-writable warning"

step "stage, diff, re-lint staged (expect exit 0)"
"$TL" --stage "$WORK/staged.yml" --no-fail "$WORK/config.yml" > /dev/null
diff -u "$WORK/config.yml" "$WORK/staged.yml" | head -30 || true
chmod 640 "$WORK/staged.yml"
set +e; "$TL" "$WORK/staged.yml"; rc=$?; set -e; echo "exit=$rc"
[ $rc = 0 ] || fail "staged config should be clean"

step "kill the tuner on 19353 (tuner.galion.app) and re-lint"
kill "${PIDS[3]}"; wait "${PIDS[3]}" 2>/dev/null || true; PIDS[3]=""
set +e; "$TL" --tsv "$WORK/config.yml" > "$WORK/out2.tsv"; rc=$?; set -e
grep -E 'tuner.galion.app' "$WORK/out2.tsv"; echo "exit=$rc"
grep -qE 'tuner.galion.app.*19353	CLOSED' "$WORK/out2.tsv" || fail "19353 should be CLOSED"

step "revive dead port 19354 with a real tuner and re-lint"
TUNER_ADDR=127.0.0.1:19354 TUNER_ROOT=$WORK/www nice -n 10 "$TU" 2>>"$WORK/tuner.log" & PIDS+=($!)
for _ in $(seq 1 50); do ss -ltn | grep -q '127.0.0.1:19354 ' && break; sleep 0.1; done
set +e; "$TL" --tsv "$WORK/config.yml" > "$WORK/out3.tsv"; rc=$?; set -e
grep -E '	19354	' "$WORK/out3.tsv"
[ "$(grep -cE '	19354	OPEN' "$WORK/out3.tsv")" -ge 1 ] || fail "19354 should be OPEN"

step "offline replay of the VPS capture (ss -ltnp 2026-10-20 20:35 +02:00)"
set +e; "$TL" --ss "$FIX/vps-ss-20261020.txt" "$FIX/vps-config-20261020.yml" | tail -2; rc=${PIPESTATUS[0]}; set -e; echo "exit=$rc"

step "credentials guard (must refuse *.json, exit 2)"
set +e; "$TL" /nonexistent/tunnel.json; rc=$?; set -e; echo "exit=$rc"; [ $rc = 2 ] || fail guard

step "gated Rust real test (GALION_REAL=1)"
for p in "${PIDS[@]}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done; PIDS=()
( cd "$ROOT" && GALION_REAL=1 nice -n 10 cargo test --release -j 3 -p tunnel-lint --test real_tunnel_lint 2>&1 | grep -E 'test |test result' )

step "teardown"
cleanup; trap - EXIT
if ss -ltn | grep -qE ':1935[0-9] '; then fail "listeners left behind"; fi
echo "PASS $(ts)"
