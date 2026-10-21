#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# READ-ONLY capture of the live VPS nerve and its inputs, for
# `openmind-nerve replay DIR`. Only HTTP GETs on loopback, `ss -ltn` and
# `journalctl` reads run on the VPS; every byte is streamed back over ssh
# stdout, nothing is written on the VPS, nothing is restarted.
# usage: scripts/real/nerve-capture-vps.sh OUTDIR [SNAPSHOTS=3] [GAP_S=35]
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
out="$1"; n="${2:-3}"; gap="${3:-35}"
host="${NERVE_VPS:-slavik-vps}"
mkdir -p "$out"
S=(ssh -o BatchMode=yes -o ConnectTimeout=10 "$host")
for i in $(seq 1 "$n"); do
  "${S[@]}" 'curl -s -m 10 http://127.0.0.1:8670/state' > "$out/state-$i.json"
  [ "$i" -lt "$n" ] && sleep "$gap"
done
"${S[@]}" 'curl -s -m 10 http://127.0.0.1:8670/self-eval' > "$out/live-self-eval.json.txt"
"${S[@]}" 'curl -s -m 10 http://127.0.0.1:8670/self-eval.md' > "$out/live-self-eval.md.txt"
"${S[@]}" 'curl -s -m 10 http://127.0.0.1:8660/status.json' > "$out/health_status.json"
"${S[@]}" 'curl -s -m 5 http://127.0.0.1:8002/health' > "$out/body-8002.json"
"${S[@]}" 'ss -ltn' > "$out/ss.txt"
"${S[@]}" 'journalctl --since "-35min" --no-pager -q -o json --output-fields=MESSAGE,PRIORITY,_SYSTEMD_UNIT,UNIT -u cloudflared -u galion-one -u openmind-memory -u hermes-rs -u link-watch-rs -u galion-desk -u tuner -u console -u galion-health -u tuner-quick' > "$out/journal.jsonl"
"${S[@]}" 'for x in 8091/health 8002/health 8642/health 8420/health 8646/health 8650/health 8080/ 8660/status.json 8670/ 8670/health; do printf "%s " $x; curl -s -m 5 -o /dev/null -w "%{http_code}\n" http://127.0.0.1:$x; done' > "$out/probes.txt"
date '+captured %Y-%m-%d %H:%M:%S %Z (%:z)' > "$out/CAPTURED"
ls -la "$out"
