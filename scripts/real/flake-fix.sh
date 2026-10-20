#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Flake hunt: run the whole workspace test suite N times back-to-back, then
# R rounds of C copies at once, each copy in its own CARGO_TARGET_DIR (same
# checkout), the way several agents/CI jobs share one box. Any failure is a
# flake to root-cause; never "fix" one with retries or sleeps.
#
# Usage:  scripts/real/flake-fix.sh            (SEQ=10 ROUNDS=3 COPIES=4)
# Env:    BASE_TARGET  parent of the target dirs (default /workspace/swarm/target)
#         SLUG         target dir name prefix (default flake-fix)
#         MIN_FREE_GB  refuse the parallel part below this (default 8)
# Exit 0 only when every run passed. Logs: $BASE_TARGET/$SLUG-logs/.
set -uo pipefail
export PATH=/workspace/swarm/bin:$PATH
cd "$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
SEQ="${SEQ:-10}" ROUNDS="${ROUNDS:-3}" COPIES="${COPIES:-4}"
BASE_TARGET="${BASE_TARGET:-/workspace/swarm/target}" SLUG="${SLUG:-flake-fix}"
MIN_FREE_GB="${MIN_FREE_GB:-8}"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
LOGS="$BASE_TARGET/$SLUG-logs"; mkdir -p "$LOGS"
fails=0

one() { # tag target_dir
  local log="$LOGS/$1.log"
  CARGO_TARGET_DIR="$2" nice -n 10 cargo test -j 3 --workspace --no-fail-fast >"$log" 2>&1
  local rc=$?
  local sum
  sum=$(awk '/^test result/{p+=$4; f+=$6} END{printf "passed=%d failed=%d", p, f}' "$log")
  echo "$(date '+%Y-%m-%dT%H:%M:%S%:z') $1 rc=$rc $sum"
  grep -h '^test .* FAILED$' "$log" | sed 's/^/    /'
  return $rc
}

echo "== $SEQ sequential runs =="
for i in $(seq 1 "$SEQ"); do one "seq-$i" "$BASE_TARGET/$SLUG" || fails=$((fails+1)); done

free_gb=$(df -BG --output=avail / | tail -1 | tr -dc 0-9)
if [ "$free_gb" -lt "$MIN_FREE_GB" ]; then
  echo "== parallel part skipped: ${free_gb} GB free < ${MIN_FREE_GB} GB =="
else
  echo "== $ROUNDS rounds x $COPIES parallel copies (own target dirs) =="
  for r in $(seq 1 "$ROUNDS"); do
    pids=()
    for k in $(seq 1 "$COPIES"); do
      d="$BASE_TARGET/$SLUG-p$k"; [ "$k" = 1 ] && d="$BASE_TARGET/$SLUG"
      one "par-r$r-c$k" "$d" & pids+=($!)
    done
    for p in "${pids[@]}"; do wait "$p" || fails=$((fails+1)); done
  done
fi
echo "== failing runs: $fails =="
[ "$fails" -eq 0 ]
