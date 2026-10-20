#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Run every cargo-fuzz target for SECS seconds (default 60), two at a time,
# under nice -n 15. Corpus: fuzz/corpus/<target> (git-ignored, grows), seeded
# from fuzz/seeds/<target> (committed). Crashes land in fuzz/artifacts/<target>.
# Nightly is used only here, never for release builds. See docs/fuzzing.md.
#   scripts/fuzz.sh [SECS] [TARGET...]
set -euo pipefail
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SECS=${1:-60}; shift || true
TARGETS=("$@")
[ ${#TARGETS[@]} -gt 0 ] || TARGETS=(http_parse_head base64_decode json_escape mdns_parse)
TOOLCHAIN=${FUZZ_TOOLCHAIN:-nightly}
cd "$ROOT/fuzz"
cargo "+$TOOLCHAIN" fuzz build -O >/dev/null
run() {
  local t=$1 rc=0
  mkdir -p "corpus/$t" "artifacts/$t"
  nice -n 15 cargo "+$TOOLCHAIN" fuzz run -O "$t" "corpus/$t" "seeds/$t" -- \
    -max_total_time="$SECS" -timeout=10 -print_final_stats=1 \
    -artifact_prefix="artifacts/$t/" >"artifacts/$t.log" 2>&1 || rc=$?
  printf '%-16s exit %s  %s\n' "$t" "$rc" "$(grep -E 'stat::number_of_executed_units|stat::average_exec_per_sec' "artifacts/$t.log" | awk '{printf "%s ", $0}')"
  return $rc
}
status=0
for ((i = 0; i < ${#TARGETS[@]}; i += 2)); do
  pids=()
  for t in "${TARGETS[@]:i:2}"; do run "$t" & pids+=($!); done
  for p in "${pids[@]}"; do wait "$p" || status=1; done
done
exit $status
