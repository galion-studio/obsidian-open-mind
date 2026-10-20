#!/bin/bash -eu
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# ClusterFuzzLite / OSS-Fuzz build: compile every cargo-fuzz target in fuzz/
# and copy it, with its committed seed corpus, to $OUT.
cd "$SRC/galion"
cargo fuzz build -O
release=fuzz/target/x86_64-unknown-linux-gnu/release
for t in http_parse_head base64_decode json_escape mdns_parse; do
  cp "$release/$t" "$OUT/"
  if [ -d "fuzz/seeds/$t" ]; then
    (cd "fuzz/seeds/$t" && zip -q -r "$OUT/${t}_seed_corpus.zip" .)
  fi
done
