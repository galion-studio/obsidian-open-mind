#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for swarm/one-hardening (slot 29, ports 19290-19299).
#  1. Builds the static x86_64 musl release galion-one.
#  2. Runs crates/galion-one/tests/real_one_hardening.rs against it (pairing,
#     analytics `at`, session `at`, services, console probe, /proc units,
#     health with a fake nvidia-smi and with none, no store from a failed
#     pairing). Ports 19290-19296.
#  3. Starts it once more on 19297 (mouth 19298) with a fixture home and
#     lists every unpaired GET route, then the same GETs against the live
#     https://one.galion.app (read-only, no cookie), and diffs
#     `path status content-type`.
#  4. With OLD_BIN=<galion-one built from b44cd4f>: the same failed pairing
#     against the old binary on 19299 (mouth 19293), for a before/after.
# Kills everything it started. Usage: scripts/real/one-hardening.sh
#   env: CARGO_TARGET_DIR, NO_BUILD=1, NO_LIVE=1, OLD_BIN=path
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
target_dir=${CARGO_TARGET_DIR:-/workspace/swarm/target/one-hardening}
triple=x86_64-unknown-linux-musl
bin=$target_dir/$triple/release/galion-one
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

if [ "${NO_BUILD:-0}" != 1 ]; then
  CARGO_TARGET_DIR=$target_dir nice -n 10 cargo build -q -j 3 --release -p galion-one --target $triple
fi
echo "== start $(TZ=Europe/Zagreb date -Iseconds)"
echo "== binary: $(readelf -d "$bin" 2>/dev/null | grep -c NEEDED || true) NEEDED, sha256 $(sha256sum "$bin" | cut -c1-16), $(stat -c %s "$bin") bytes"

echo "== Rust real test (release binary)"
(cd "$repo" && GALION_REAL=1 GALION_ONE_BIN=$bin CARGO_TARGET_DIR=$target_dir \
  nice -n 10 cargo test -q -j 3 -p galion-one --test real_one_hardening -- --nocapture 2>&1 \
  | grep -vE '^\s*$|^running ')

work=$(mktemp -d "$target_dir/real-one-hardening.XXXXXX")
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for pid in "${pids[@]}"; do wait "$pid" 2>/dev/null || true; done
  rm -rf "$work"
}
trap cleanup EXIT

home=$work/home
mkdir -p "$home/one/font" "$work/hermes" "$work/secrets"
for page in index nerve admin space; do
  printf '<!doctype html><title>fixture %s</title>\n' "$page" >"$home/one/$page.html"
done
printf '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>\n' >"$home/one/mark.svg"
printf 'wOF2fixture' >"$home/one/font/jost.woff2"
printf '[]\n' >"$home/one/catalog.json"
: >"$work/hermes.env"

env -i PATH=/usr/bin:/bin HOME="$home" TZ=Europe/Zagreb ONE_NO_WARM=1 GALION_HOME="$home" \
  ONE_HERMES_DIR="$work/hermes" ONE_HERMES_ENV="$work/hermes.env" \
  ONE_DEVICE_STORE="$work/secrets/admin-device.json" \
  ONE_BIND=127.0.0.1:19297 ONE_MOUTH_BIND=127.0.0.1:19298 \
  GALION_MIND=127.0.0.1:19294 GALION_EAR=127.0.0.1:19294 GALION_EYE=127.0.0.1:19294 \
  GALION_MEMORY=127.0.0.1:19294 ONE_CONSOLE=127.0.0.1:19294 \
  "$bin" 2>"$work/one.log" &
pids+=($!)
for _ in $(seq 1 50); do curl -fsS -m 1 http://127.0.0.1:19297/health >/dev/null 2>&1 && break; sleep 0.2; done
sleep 2

routes=(/ /index.html /nerve /admin /space /mark.svg /font/jost.woff2 /health /link /me
  /api/v1/awake /api/v1/catalog /api/v1/services /api/v1/analytics /api/v1/analytics/services
  /api/v1/analytics/health /session /files /files/abc /model /models /pair /nope)
probe() { # base label
  local base=$1 out ct body
  for p in "${routes[@]}"; do
    out=$(curl -sS -m 15 -o "$work/body" -w '%{http_code} %{content_type}' "$base$p" 2>&1 || true)
    ct=$(echo "$out" | awk '{print $2, $3}' | sed 's/ $//')
    case "$ct" in
      application/json*|text/plain*) body=$(head -c 300 "$work/body" | tr '\n' ' ') ;;
      *) body="sha=$(sha256sum <"$work/body" | cut -c1-12) bytes=$(stat -c %s "$work/body")" ;;
    esac
    echo "GET $p -> $out | $body"
  done
}

echo "== box 19297, unpaired"
probe http://127.0.0.1:19297 | tee "$work/box.txt"
echo "== failed pairing on 19297 (no store): secrets dir entries before/after"
echo "  before: $(ls -A "$work/secrets" | wc -l)"
curl -sS -m 5 -X POST -d '{"code":"WRONG"}' http://127.0.0.1:19297/pair; echo
echo "  after: $(ls -A "$work/secrets" | wc -l)"

if [ "${NO_LIVE:-0}" != 1 ]; then
  echo "== live https://one.galion.app, unpaired, GET only ($(TZ=Europe/Zagreb date -Iseconds))"
  probe https://one.galion.app | tee "$work/live.txt"
  echo "== diff (path status content-type), box vs live"
  cut -d'|' -f1 "$work/box.txt" >"$work/box.k"
  cut -d'|' -f1 "$work/live.txt" >"$work/live.k"
  diff "$work/box.k" "$work/live.k" && echo "  no differences"
fi

if [ -n "${OLD_BIN:-}" ]; then
  echo "== old binary ($OLD_BIN) on 19299, same failed pairing, no store"
  mkdir -p "$work/old-secrets" "$work/oldhome/one"
  env -i PATH=/usr/bin:/bin HOME="$work/oldhome" TZ=Europe/Zagreb ONE_NO_WARM=1 GALION_HOME="$work/oldhome" \
    ONE_HERMES_DIR="$work/hermes" ONE_HERMES_ENV="$work/hermes.env" \
    ONE_DEVICE_STORE="$work/old-secrets/admin-device.json" \
    ONE_BIND=127.0.0.1:19299 ONE_MOUTH_BIND=127.0.0.1:19293 GALION_MIND=127.0.0.1:19294 \
    GALION_EAR=127.0.0.1:19294 GALION_EYE=127.0.0.1:19294 GALION_MEMORY=127.0.0.1:19294 \
    "$OLD_BIN" 2>/dev/null &
  pids+=($!)
  for _ in $(seq 1 50); do curl -fsS -m 1 http://127.0.0.1:19299/health >/dev/null 2>&1 && break; sleep 0.2; done
  curl -sS -m 5 -X POST -d '{"code":"WRONG"}' http://127.0.0.1:19299/pair; echo
  echo "  old: secrets dir entries after: $(ls -A "$work/old-secrets" | wc -l) $(ls -A "$work/old-secrets")"
  # Same pairing + analytics `at` on the old binary (fixture code FIXTURE42).
  printf '{"codeHash":"%s","tokenHash":"","tokenHashes":[],"deviceHash":"","fails":0,"lockedUntil":0}\n' \
    "$(printf FIXTURE42 | sha256sum | cut -d' ' -f1)" >"$work/old-secrets/admin-device.json"
  old_cookie=$(curl -sS -m 5 -D - -o /dev/null -X POST -d '{"code":"FIXTURE42"}' http://127.0.0.1:19299/pair \
    | tr -d '\r' | sed -n 's/^Set-Cookie: galion_admin=\([^;]*\);.*/\1/p')
  echo "  old analytics at: $(curl -sS -m 10 -H "Cookie: galion_admin=$old_cookie" http://127.0.0.1:19299/api/v1/analytics | grep -oE '"at":"[^"]*"')  (box date: $(TZ=Europe/Zagreb date -Iseconds))"
  echo "== new binary, same: analytics at on 19297"
  printf '{"codeHash":"%s","tokenHash":"","tokenHashes":[],"deviceHash":"","fails":0,"lockedUntil":0}\n' \
    "$(printf FIXTURE42 | sha256sum | cut -d' ' -f1)" >"$work/secrets/admin-device.json"
  new_cookie=$(curl -sS -m 5 -D - -o /dev/null -X POST -d '{"code":"FIXTURE42"}' http://127.0.0.1:19297/pair \
    | tr -d '\r' | sed -n 's/^Set-Cookie: galion_admin=\([^;]*\);.*/\1/p')
  echo "  new analytics at: $(curl -sS -m 10 -H "Cookie: galion_admin=$new_cookie" http://127.0.0.1:19297/api/v1/analytics | grep -oE '"at":"[^"]*"')  (box date: $(TZ=Europe/Zagreb date -Iseconds))"
fi
echo "== stderr 19297: $(tr '\n' ' ' <"$work/one.log")"
echo "== done $(TZ=Europe/Zagreb date -Iseconds)"
