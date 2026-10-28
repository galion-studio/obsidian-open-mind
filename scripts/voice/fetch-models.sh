#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Fetch the CPU voice models for galion-voice-serve, pinned by revision and
# sha256. Every file here is under an allowed licence (see
# docs/third-party/voice-mouth.md): Whisper tiny.en (Apache-2.0 card, MIT
# upstream), KittenTTS nano 0.8 safetensors (Apache-2.0), CMUdict (BSD-2-style).
# Nothing runs automatically: the service never downloads; a human runs this.
# usage: scripts/voice/fetch-models.sh <dest-dir>      (about 215 MB)
set -euo pipefail
dest="${1:?usage: fetch-models.sh <dest-dir>}"
W=https://huggingface.co/openai/whisper-tiny.en/resolve/87c7102498dcde7456f24cfd30239ca606ed9063
K=https://huggingface.co/idle-intelligence/kitten-tts-nano-safetensors/resolve/3e76949c4bf4521c22846372064a3ba2c2620921
C=https://raw.githubusercontent.com/cmusphinx/cmudict/7479086
M=https://raw.githubusercontent.com/huggingface/candle/a3b1699409def0bf1ea690c12f81774ce17384d2/candle-examples/examples/whisper
# dir|file|url|sha256
LIST="
whisper-tiny.en|README.md|$W/README.md|13627ce3d7dce389223eab7b6ed06738f0f1c786e5245e650b029e4e314c8ff1
whisper-tiny.en|config.json|$W/config.json|b5366317df280f79e6cc366f259f379674f81a8721d458acf2e00680ccb10b1d
whisper-tiny.en|tokenizer.json|$W/tokenizer.json|5eb60cec1e77aeeb6869a2bb5a8e01a84c3fe5d072d75369343021fe6f5310d0
whisper-tiny.en|model.safetensors|$W/model.safetensors|db59695928ded6043adaef491a53ef4e12da9611184d77c53baa691a60b958ad
whisper-tiny.en|melfilters.bytes|$M/melfilters.bytes|85818f156f7e189453901a515e4726d270d307f976e161cf9403e8caab405498
kitten-tts-nano|README.md|$K/README.md|425d2a7481b6b48b4de6cb6dc14ec12d9a5bd4effeaa9bc134483e743f2b3f96
kitten-tts-nano|config.json|$K/config.json|beb199ce5341650a73beb44140f90871b63616fe4db2a6d82abb2399043566e6
kitten-tts-nano|kitten-nano.safetensors|$K/kitten-nano.safetensors|b45307e7cbb820255e3c3e5b773ab1620258ef263a6eb59ad1e924c0acd6cb77
kitten-tts-nano|kitten-voices.safetensors|$K/kitten-voices.safetensors|034554611b2ebc8bdf8f119c78f45d55ed07b9d6ef59f56bbb207b4a59af8691
cmudict|LICENSE|$C/LICENSE|bd4ce8e44170a5f9f481310ca85c51de3c4f851a65e679b40e603b143bd3542a
cmudict|cmudict.dict|$C/cmudict.dict|81917843c7f44ce2b094ac63873c2c7a4cf802040792c455ba3ca406891c3d22
"
echo "$LIST" | while IFS='|' read -r dir file url sum; do
  [ -n "$dir" ] || continue
  mkdir -p "$dest/$dir"
  out="$dest/$dir/$file"
  if [ -f "$out" ] && echo "$sum  $out" | sha256sum -c --quiet - 2>/dev/null; then
    echo "ok   $dir/$file"; continue
  fi
  curl -fsSL --retry 2 -o "$out.part" "$url"
  echo "$sum  $out.part" | sha256sum -c --quiet - || { rm -f "$out.part"; echo "sha256 mismatch: $dir/$file" >&2; exit 1; }
  mv "$out.part" "$out"; echo "got  $dir/$file"
done
