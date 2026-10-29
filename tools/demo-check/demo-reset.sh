#!/bin/bash
# SPDX-License-Identifier: MIT
# Demo reset for the Obsidian OpenMind stack (non-destructive; nothing is deleted).
#  1. Backs up the console picker prefs, then sets the phone's default to River AI
#     deepseek-ai/DeepSeek-V4.1-Flash (starred), so the demo never starts on the paid default.
#  2. Checks that no Hermes run is in flight.
#  3. Reports memory noise: which codeword-like values a "demo codeword" memory search returns
#     (old test chats cannot be deleted; if noise is high, use a fresh codeword in the demo).
# Usage: demo-reset.sh [--dry-run]
set -u
dry=0; [ "${1:-}" = --dry-run ] && dry=1
ssh -o ConnectTimeout=15 slavik-vps "DRY=$dry bash -s" <<'REMOTE'
set -u
M=deepseek-ai/DeepSeek-V4.1-Flash; B=http://127.0.0.1:18080; P=/var/lib/galion-console-hermes/prefs.json
echo "prefs before: $(curl -s $B/api/hermes/prefs | jq -c '{provider,models,stars}')"
if [ "$DRY" = 0 ]; then
  D=/var/backups/demo-readiness-20261029-$(date -u +%Y%m%d-%H%M%S); mkdir -p "$D"; cp -a "$P" "$D/"; sha256sum "$D/prefs.json" > "$D/SHA256SUMS"
  echo "backup: $D (rollback: cat $D/prefs.json > $P; the console re-reads it on the next prefs call or restart)"
  curl -s -X POST -H 'Content-Type: application/json' $B/api/hermes/prefs -d "{\"action\":\"select\",\"provider\":\"river\",\"model\":\"$M\"}" >/dev/null
  curl -s $B/api/hermes/prefs | jq -e --arg m "$M" '.stars.river|index($m)' >/dev/null || \
    curl -s -X POST -H 'Content-Type: application/json' $B/api/hermes/prefs -d "{\"action\":\"star\",\"provider\":\"river\",\"model\":\"$M\"}" >/dev/null
  echo "prefs after:  $(curl -s $B/api/hermes/prefs | jq -c '{provider,models,stars}')"
fi
echo "active Hermes runs: $(curl -s $B/api/hermes/health | jq -r .active_agents)"
MCP=$(awk '/^  openmind:/{f=1} f&&/command:/{print $2; exit}' /opt/galion/hermes/config.yaml)
q(){ printf '%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo-reset","version":"1"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$1\"}}}" \
  | timeout 20 "$MCP" --vault /opt/galion/galion-light/docs/build-session --memory-url http://127.0.0.1:8420 --key-file /opt/galion/hermes/.env 2>/dev/null | jq -r 'select(.id==3)|.result.content[0].text'; }
hits=$(q "blue-harbor-42" | grep -o 'blue-harbor-42' | wc -l)
echo "memory: 'blue-harbor-42' appears $hits times in search results"
echo "memory: codeword-like values returned for 'demo codeword' / 'codeword':"
{ q "demo codeword"; q "codeword"; } | grep -oiE '\b[a-z]+-[a-z]+(-[a-z]+)?-[0-9]{2,}(-[0-9]+)?\b' | sort | uniq -c | sort -rn | sed 's/^/   /'
REMOTE
