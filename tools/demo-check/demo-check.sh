#!/bin/bash
# SPDX-License-Identifier: MIT
# Demo health check for the Obsidian OpenMind stack. Runs every demo step through the real
# console API (the same path the phone uses) on the VPS and prints PASS/FAIL per check.
# Usage: demo-check.sh [--quick] [--exact] [--mobile] [--port 8080]
#   --quick  no model calls (services, pages, lists, MCP, error handling only)
#   --exact  use the real demo codeword blue-harbor-42 (default: a fresh unique codeword per run, because
#            old test codewords in memory cannot be deleted and make recall ambiguous)
#   --mobile also run mobile-check.mjs (headless Chrome, 390x844 Android UA) on both console ports
#            through an SSH tunnel; with full mode it sends one River chat through the phone UI.
# Exit code = number of failures. A copy of the output goes to /workspace/swarm/demo/logs/.
set -u
here=$(cd "$(dirname "$0")" && pwd); env=(); mobile=0; quick=0
while [ $# -gt 0 ]; do case $1 in
  --quick) env+=(MODE=quick); quick=1;; --mobile) mobile=1;; --exact) env+=(EXACT=1);;
  --port) env+=(PORT="$2"); shift;; *) echo "unknown option $1"; exit 2;; esac; shift; done
log="$here/logs/demo-check-$(date +%Y%m%d-%H%M%S).log"; mkdir -p "$here/logs"
{ echo "demo-check $(date '+%Y-%m-%d %H:%M %Z') ${env[*]}"
  ssh -o ConnectTimeout=15 slavik-vps "env ${env[*]:-MODE=full} bash -s" < "$here/demo-check-remote.sh"; rc=$?
  if [ "$mobile" = 1 ]; then
    a=$((20000 + RANDOM % 20000)); b=$((a + 1)); stamp=$(date +%H%M%S)
    ssh -o ExitOnForwardFailure=yes -o ControlMaster=no -fN -L "$a:127.0.0.1:18080" -L "$b:127.0.0.1:8080" slavik-vps
    tpid=$(pgrep -nf -- "-L $a:127.0.0.1:18080")
    for pp in "$a:18080" "$b:8080"; do lp=${pp%%:*}; rp=${pp##*:}; extra=(--fail)
      [ "$quick" = 0 ] && [ "$rp" = 18080 ] && extra+=(--send)
      echo "== phone UI :$rp (headless Chrome 390x844)"
      timeout 180 node "$here/mobile-check.mjs" "http://127.0.0.1:$lp" "$here/logs/mobile-$rp-$stamp.png" "${extra[@]}"; rc=$((rc + $?))
    done
    [ -n "$tpid" ] && kill "$tpid"
  fi
  echo "exit=$rc"; exit $rc; } 2>&1 | tee "$log"
exit "${PIPESTATUS[0]}"
