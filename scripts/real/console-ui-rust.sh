#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for the Rust console pages (`galion-console-static --ui rust`,
# swarm/console-ui-rust, ports 19370-19379). Builds the release binary, opens a
# read-only SSH forward to the live console API (VPS 127.0.0.1:8080 -> 19379)
# and desk pulse (8091 -> 19378), serves the Rust pages on 127.0.0.1:19375,
# then runs tests/real_console_ui.rs (GET only; never POSTs, never opens
# /api/v1/markers/stream). Optional headless-Chrome screenshots with SHOTS=DIR.
# Nothing is written on the VPS.
set -uo pipefail
export PATH=/workspace/swarm/bin:$PATH
here=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/console-ui}
export CARGO_INCREMENTAL=0
PORT=${CONSOLE_UI_PORT:-19375}; UP=19379; PULSE=19378
VPS=${CONSOLE_UI_VPS:-root@51.38.137.193}
pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; wait "$p" 2>/dev/null; done; }
trap cleanup EXIT

echo "real: start $(TZ=Europe/Zagreb date -Iseconds)"
(cd "$here" && nice -n 10 cargo build --release -j 3 -p console-static -q) || exit 1
bin="$CARGO_TARGET_DIR/release/galion-console-static"
ls -l "$bin" | awk '{print "real: binary bytes " $5}'

if ! curl -s -m 3 -o /dev/null "http://127.0.0.1:$UP/api/v1/ping"; then
  ssh -i "$HOME/.ssh/id_ed25519_vps" -o IdentitiesOnly=yes -o BatchMode=yes -o ExitOnForwardFailure=yes \
      -o ServerAliveInterval=15 -N -L "127.0.0.1:$UP:127.0.0.1:8080" -L "127.0.0.1:$PULSE:127.0.0.1:8091" "$VPS" &
  pids+=($!)
  for _ in $(seq 1 50); do curl -s -m 2 -o /dev/null "http://127.0.0.1:$UP/api/v1/ping" && break; sleep 0.2; done
fi

"$bin" --bind "127.0.0.1:$PORT" --ui rust --upstream "127.0.0.1:$UP" --pulse "127.0.0.1:$PULSE" \
  2> "${TMPDIR:-/tmp}/console-ui-rust-real.log" &
pids+=($!)
for _ in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:$PORT/_ui/favicon.svg" && break; sleep 0.1; done
head -1 "${TMPDIR:-/tmp}/console-ui-rust-real.log"

fail=0
for p in /status /services /agents; do
  out=$(curl -s -o "${TMPDIR:-/tmp}/cur$$.html" -m 10 -w "%{http_code} %{size_download} %{time_total}" "http://127.0.0.1:$PORT$p")
  scripts=$(grep -c "<script" "${TMPDIR:-/tmp}/cur$$.html")
  echo "real: GET $p -> $out s, <script> tags: $scripts"
  [ "${out%% *}" = 200 ] && [ "$scripts" = 0 ] || fail=1
done
rm -f "${TMPDIR:-/tmp}/cur$$.html"
css=$(grep -o '/_ui/console\.[0-9a-f]*\.css' <(curl -s "http://127.0.0.1:$PORT/agents") | head -1)
curl -s -o /dev/null -w "real: GET $css -> %{http_code} %{size_download} B\n" "http://127.0.0.1:$PORT$css"
for h in console.galion.app galion.app developer.galion.app; do
  curl -s -o /dev/null -m 10 -w "real: Host $h /agents -> %{http_code} %{size_download} B\n" -H "Host: $h" "http://127.0.0.1:$PORT/agents"
done
rss=$(awk '/VmRSS/{print $2}' "/proc/${pids[-1]}/status"); echo "real: front RSS ${rss} kB"

(cd "$here" && GALION_REAL=1 CONSOLE_UI_FRONT="127.0.0.1:$PORT" CONSOLE_UI_UPSTREAM="127.0.0.1:$UP" \
  CONSOLE_UI_PULSE="127.0.0.1:$PULSE" nice -n 10 cargo test --release -j 3 -p console-static \
  --test real_console_ui -q -- --nocapture 2>&1 | grep -E '^real:|test result|panicked|FAILED') || fail=1

if [ -n "${SHOTS:-}" ] && command -v google-chrome >/dev/null; then
  mkdir -p "$SHOTS"
  for p in status services agents; do
    for theme in dark light; do
      args=(--headless=new --no-sandbox --disable-gpu --hide-scrollbars --window-size=412,915)
      url="http://127.0.0.1:$PORT/$p"
      [ "$theme" = light ] && url="http://127.0.0.1:$PORT/_ui/theme?to=light&back=/$p"
      timeout 60 google-chrome "${args[@]}" --screenshot="$SHOTS/after-$p-$theme.png" "$url" 2>/dev/null
    done
  done
  # A public console host (console.* rule; .test because .app is HSTS-preloaded)
  # without the admin cookie gets the pairing page.
  timeout 60 google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars --window-size=412,915 \
    --host-resolver-rules="MAP console.galion.test 127.0.0.1" \
    --screenshot="$SHOTS/after-pair-console-host.png" "http://console.galion.test:$PORT/agents" 2>/dev/null
  ls -1 "$SHOTS" | sed 's/^/real: shot /'
fi
echo "real: end $(TZ=Europe/Zagreb date -Iseconds) fail=$fail"
exit $fail
