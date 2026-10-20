#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for swarm slug voice-rust, ports 19371-19377 (19370 was taken
# by another agent's console during the run). Everything runs on this box's
# CPU; nothing touches the VPS.
#
#   ear   19371  galion-voice-serve --stt whisper-large-v3-turbo --tts none
#   mouth 19372  galion-voice-serve --tts chatterbox --stt none  (MOUTH_ARGS)
#   one   19374  galion-one (integration-1 8fd26f6), GALION_MOUTH=19372,
#                GALION_EAR=19371, its own mouth proxy on 19375
#
# Steps: health while loading (must be 503 "loading"), health when loaded,
# Samantha lines -> /synthesize -> WAV -> ear -> text (word match, latency),
# then Galion One end to end: /hear (paired with a throw-away pairing store in
# a temp dir) and the :mouth proxy /synthesize + /health pass-through.
# Kills everything it started.
#
# usage: scripts/real/voice-rust.sh [models-dir] [voices-dir]
#   MOUTH_ARGS="--flow-steps 5 --prompt-seconds 4"   fast profile
#   ONE_BIN=/path/to/galion-one                       enables the Galion One stage
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
cd "$(dirname "$0")/../.."
MODELS="${1:-/workspace/oom-models}"
VOICES="${2:-/workspace/swarm/target/voice-rust/voices}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/swarm/target/voice-rust}"
export CARGO_INCREMENTAL=0
if [ "${SKIP_BUILD:-0}" != 1 ]; then
  nice -n 10 cargo build --release -j 3 -p galion-voice-cpu --features whisper,kitten,chatterbox --bin galion-voice-serve
fi
BIN="$CARGO_TARGET_DIR/release/galion-voice-serve"
TMP=$(mktemp -d)
PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done; rm -rf "$TMP"; }
trap cleanup EXIT
now() { date +%s%3N; }
hdr() { grep -i "^$1:" "$2" | tr -d '\r' | cut -d' ' -f2-; }
jtext() { sed -n 's/.*"text":"\([^"]*\)".*/\1/p'; }
norm() { tr 'A-Z' 'a-z' | tr -c 'a-z0-9\n' ' ' | tr -s ' ' | sed 's/^ //; s/ $//'; }
# Share of reference words found in order-insensitive bag of heard words.
match() {
  awk -v a="$1" -v b="$2" 'BEGIN{n=split(a,x," "); m=split(b,y," "); for(i=1;i<=m;i++) c[y[i]]++;
    h=0; for(i=1;i<=n;i++) if (c[x[i]]>0) {h++; c[x[i]]--}; printf "%.0f%%", (n?100*h/n:0)}'
}
wait_ok() { for _ in $(seq 1 600); do [ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$1/health")" = 200 ] && return 0; sleep 0.5; done; echo "port $1 never became healthy" >&2; return 1; }

echo "# voice-rust real test $(TZ=Europe/Zagreb date '+%Y-%m-%d %H:%M:%S %Z (%:z)'), $(nproc) CPUs, load $(cut -d' ' -f1-3 /proc/loadavg)"
echo "mouth args: ${MOUTH_ARGS:-<defaults: flow steps 10, prompt 10 s>}"
nice -n 5 "$BIN" --bind 127.0.0.1:19371 --models "$MODELS" --stt whisper-large-v3-turbo --tts none 2>"$TMP/ear.log" & PIDS+=($!)
# shellcheck disable=SC2086
nice -n 5 "$BIN" --bind 127.0.0.1:19372 --models "$MODELS" --stt none --tts chatterbox --voices "$VOICES" ${MOUTH_ARGS:-} 2>"$TMP/mouth.log" & PIDS+=($!)
sleep 1
echo "## health while loading"
for p in 19371 19372; do echo "- :$p -> $(curl -s -w ' HTTP %{http_code}' "http://127.0.0.1:$p/health" | sed 's/"rss_kb".*}/.../')"; done
t=$(now); wait_ok 19371; wait_ok 19372; echo "both healthy after $(( $(now) - t )) ms"
echo "## health when loaded"
for p in 19371 19372; do echo "- :$p -> $(curl -s -w ' HTTP %{http_code}' "http://127.0.0.1:$p/health" | sed 's/"rss_kb".*}/.../')"; done
grep -h "loaded\|ready" "$TMP/ear.log" "$TMP/mouth.log" | sed 's/^/    /'

echo "## Samantha lines: mouth -> WAV -> ear"
echo "| # | voice | text | audio s | synth ms | RTF | ear ms | heard | words |"
echo "|---|---|---|---|---|---|---|---|---|"
i=0
while IFS='|' read -r voice line; do
  i=$((i+1))
  body=$(printf '{"text":"%s","voice":"%s"}' "$line" "$voice")
  t=$(now); code=$(curl -s -D "$TMP/h" -o "$TMP/s$i.wav" -w '%{http_code}' -H 'Content-Type: application/json' -d "$body" http://127.0.0.1:19372/synthesize); ms=$(( $(now) - t ))
  [ "$code" = 200 ] || { echo "| $i | $voice | $line | synth HTTP $code | | | | | |"; continue; }
  t=$(now); heard=$(curl -s -F "file=@$TMP/s$i.wav;type=audio/wav" -F language=en -F fast=1 http://127.0.0.1:19371/v1/audio/transcriptions | jtext); ems=$(( $(now) - t ))
  echo "| $i | $voice | $line | $(hdr X-Duration "$TMP/h") | $ms | $(hdr X-RTF "$TMP/h") | $ems | $heard | $(match "$(echo "$line" | norm)" "$(echo "$heard" | norm)") |"
  [ -n "${KEEP_WAVS:-}" ] && cp "$TMP/s$i.wav" "$KEEP_WAVS/line$i-$voice.wav"
done <<'LINES'
samantha|Hi. I am right here with you, and I am listening.
samantha|That sounds like a lot to carry. Do you want to talk about it?
jarvis|Good evening. All systems are running normally.
samantha|I love the way the light looks this time of day.
LINES
echo "## contract checks"
echo "- empty text -> $(curl -s -w ' HTTP %{http_code}' -H 'Content-Type: application/json' -d '{"text":"  "}' http://127.0.0.1:19372/synthesize)"
echo "- non-JSON body -> $(curl -s -w ' HTTP %{http_code}' -d 'nope' http://127.0.0.1:19372/synthesize)"
echo "- ear without audio -> $(curl -s -w ' HTTP %{http_code}' -F language=en http://127.0.0.1:19371/v1/audio/transcriptions)"
grep -h "^chatterbox:" "$TMP/mouth.log" | sed 's/^/    /'

if [ -n "${ONE_BIN:-}" ] && [ -x "$ONE_BIN" ]; then
  echo "## Galion One (integration-1 8fd26f6) end to end"
  mkdir -p "$TMP/home" "$TMP/hermes"
  code=VOICERUSTTEST
  hash=$(printf '%s' "$code" | sha256sum | cut -d' ' -f1)
  printf '{"codeHash":"%s","tokenHash":"","tokenHashes":[],"deviceHash":"","fails":0,"lockedUntil":0}\n' "$hash" > "$TMP/store.json"
  GALION_HOME="$TMP/home" ONE_HERMES_DIR="$TMP/hermes" ONE_DEVICE_STORE="$TMP/store.json" \
  ONE_BIND=127.0.0.1:19374 ONE_MOUTH_BIND=127.0.0.1:19375 ONE_NO_WARM=1 ONE_WARM_MIND=0 \
  GALION_MOUTH=127.0.0.1:19372 GALION_EAR=127.0.0.1:19371 GALION_MIND=127.0.0.1:19376 GALION_EYE=127.0.0.1:19377 \
    "$ONE_BIN" 2>"$TMP/one.log" & PIDS+=($!)
  for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:19375/health && break; sleep 0.2; done
  echo "- one :mouth /health pass-through -> $(curl -s -w ' HTTP %{http_code}' http://127.0.0.1:19375/health | sed 's/"rss_kb".*}/.../')"
  t=$(now); c=$(curl -s -o "$TMP/one.wav" -w '%{http_code}' -H 'Content-Type: application/json' -d '{"text":"Hello. I can hear you now."}' http://127.0.0.1:19375/synthesize); ms=$(( $(now) - t ))
  echo "- one :mouth /synthesize -> HTTP $c, $(stat -c %s "$TMP/one.wav") bytes, $ms ms (Galion One's own timeout to the mouth is 30 s)"
  if [ "$c" = 200 ]; then
    heard=$(curl -s -F "file=@$TMP/one.wav;type=audio/wav" -F language=en http://127.0.0.1:19371/v1/audio/transcriptions | jtext)
    echo "  - that WAV through the ear: \"$heard\""
    [ -n "${KEEP_WAVS:-}" ] && cp "$TMP/one.wav" "$KEEP_WAVS/one-mouth.wav"
  fi
  curl -s -c "$TMP/jar" -H 'Content-Type: application/json' -d "{\"code\":\"$code\"}" http://127.0.0.1:19374/pair >/dev/null
  t=$(now); r=$(curl -s -b "$TMP/jar" -H 'Content-Type: audio/wav' --data-binary "@$TMP/s1.wav" http://127.0.0.1:19374/hear); ms=$(( $(now) - t ))
  echo "- one /hear (paired, line 1 WAV) -> $r in $ms ms (Galion One's ear timeout is 20 s)"
  grep -i "ear\|mouth" "$TMP/one.log" | tail -5 | sed 's/^/    /' || true
fi
echo "peak RSS: ear $(grep -o '"hwm_kb":[0-9]*' <(curl -s http://127.0.0.1:19371/health)), mouth $(grep -o '"hwm_kb":[0-9]*' <(curl -s http://127.0.0.1:19372/health))"
