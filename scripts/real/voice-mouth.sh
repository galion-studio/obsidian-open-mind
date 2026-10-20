#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for swarm slot 06 (voice-mouth), ports 19060-19069 only.
# Builds galion-voice-serve (whisper + kitten) and galion-one (release),
# starts the voice service on 19060 and a galion-one with GALION_MOUTH pointed
# at it on 19061 (text) / 19062 (mouth), sends a real WAV in and text in,
# prints latency and CPU, then kills everything it started.
# usage: scripts/real/voice-mouth.sh [models-dir]   (default /workspace/oom-models)
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
cd "$(dirname "$0")/../.."
MODELS="${1:-/workspace/oom-models}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/swarm/target/voice-mouth}"
nice -n 10 cargo build --release -j 4 -p galion-voice-cpu --features whisper,kitten --bin galion-voice-serve
nice -n 10 cargo build --release -j 4 -p galion-one
BIN="$CARGO_TARGET_DIR/release"
WAV="$MODELS/whisper-tiny.en/samples_jfk.wav"
TMP=$(mktemp -d)
PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done; rm -rf "$TMP"; }
trap cleanup EXIT

wait_port() { for _ in $(seq 1 150); do curl -s -o /dev/null "http://127.0.0.1:$1/health" && return 0; sleep 0.2; done; echo "port $1 never came up" >&2; return 1; }
hdr() { grep -i "^$1:" "$TMP/h" | tr -d '\r' | cut -d' ' -f2-; }

echo "# $(TZ=Europe/Zagreb date '+%Y-%m-%d %H:%M:%S %Z (%:z)') on $(hostname), $(nproc) CPUs, load $(cut -d' ' -f1-3 /proc/loadavg)"
for threads in 1 4; do
  RAYON_NUM_THREADS=$threads nice -n 10 "$BIN/galion-voice-serve" --bind 127.0.0.1:19060 --models "$MODELS" 2>"$TMP/voice.log" &
  VP=$!; PIDS+=("$VP")
  wait_port 19060
  echo "## RAYON_NUM_THREADS=$threads"
  echo "health: $(curl -s -w ' -> %{http_code}' http://127.0.0.1:19060/health)"
  for i in 1 2 3; do
    out=$(curl -s -D "$TMP/h" -H 'Content-Type: audio/wav' --data-binary @"$WAV" -w ' -> %{http_code} %{time_total}s' http://127.0.0.1:19060/v1/audio/transcriptions)
    echo "stt #$i (11.0 s JFK WAV): $out"
  done
  for i in 1 2; do
    curl -s -D "$TMP/h" -o "$TMP/tts.wav" -H 'Content-Type: application/json' \
      -d '{"text":"Hello Slavik. The mouth can speak again, on the CPU only."}' \
      -w "tts #$i: %{http_code} %{size_download} B %{time_total}s" http://127.0.0.1:19060/synthesize
    echo " audio $(hdr X-Audio-S) s, wall $(hdr X-Wall-Ms) ms, cpu $(hdr X-Cpu-Ms) ms"
  done
  curl -s -D "$TMP/h" -o "$TMP/rt.wav" -H 'Content-Type: audio/wav' --data-binary @"$WAV" \
    -w "round trip /v1/voice: %{http_code} %{size_download} B %{time_total}s" http://127.0.0.1:19060/v1/voice
  echo " stt $(hdr X-Stt-Ms) ms, tts $(hdr X-Tts-Ms) ms, reply audio $(hdr X-Audio-S) s, cpu $(hdr X-Cpu-Ms) ms"
  echo "transcript header: $(hdr X-Transcript)"
  echo "process: $(grep -E 'VmHWM|VmRSS' /proc/$VP/status | tr -s ' \t' ' ' | tr '\n' ' ')"
  if [ "$threads" = 4 ]; then
    G1=(env -i PATH=/usr/bin:/bin HOME="$TMP" GALION_HOME="$TMP" ONE_NO_WARM=1 ONE_WARM_MIND=0
        ONE_DEVICE_STORE="$TMP/none.json" ONE_HERMES_DIR="$TMP" ONE_HERMES_ENV="$TMP/none.env"
        OPENROUTER_KEY_FILE="$TMP/none.env" GALION_MIND=127.0.0.1:19069 GALION_EAR=127.0.0.1:19060
        ONE_BIND=127.0.0.1:19061 ONE_MOUTH_BIND=127.0.0.1:19062)
    "${G1[@]}" GALION_MOUTH=127.0.0.1:19060 nice -n 10 "$BIN/galion-one" 2>"$TMP/one.log" &
    OP=$!; PIDS+=("$OP"); wait_port 19062
    echo "## galion-one (patched) with GALION_MOUTH=127.0.0.1:19060"
    echo "mouth /health: $(curl -s -w ' -> %{http_code}' http://127.0.0.1:19062/health)"
    curl -s -o "$TMP/one.wav" -H 'Content-Type: application/json' -d '{"text":"Galion One speaking through the CPU voice service."}' \
      -w "mouth /synthesize: %{http_code} %{size_download} B %{time_total}s\n" http://127.0.0.1:19062/synthesize
    kill "$OP"; wait "$OP" 2>/dev/null || true
    "${G1[@]}" nice -n 10 "$BIN/galion-one" 2>"$TMP/one.log" &
    OP=$!; PIDS+=("$OP"); wait_port 19062 || true
    echo "## galion-one (patched) without GALION_MOUTH (the live configuration)"
    echo "mouth /health: $(curl -s -w ' -> %{http_code}' http://127.0.0.1:19062/health)"
    kill "$OP"; wait "$OP" 2>/dev/null || true
  fi
  kill "$VP"; wait "$VP" 2>/dev/null || true
done
echo "# done; all started processes stopped"
