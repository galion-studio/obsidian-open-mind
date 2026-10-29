# SPDX-License-Identifier: MIT
# Live-style deploy of the built hermes-gateway inside the sandbox:
# loopback only, --mock provider (no network, no spend), throwaway state.
set -eu
D=$PWD/.gate-tmp/deploy; rm -rf "$D"; mkdir -p "$D/home"
KEY=$(od -An -tx1 -N16 /dev/urandom | tr -d ' \n')
PORT=18642
env -i PATH=/usr/bin:/bin HOME="$D" HERMES_HOME="$D/home" API_SERVER_KEY="$KEY" \
  HERMES_RS_HOST=127.0.0.1 HERMES_RS_PORT=$PORT HERMES_RS_STATE_DB="$D/state.db" \
  HERMES_RS_SPEND_FILE="$D/spend.json" HERMES_RS_SESSION_USD_CAP=0.01 HERMES_RS_DAILY_USD_CAP=0.01 \
  HERMES_RS_MCP=0 ./target/debug/hermes-gateway --mock > "$D/gw.log" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
i=0; until curl -fsS -m1 http://127.0.0.1:$PORT/health > "$D/health.json" 2>/dev/null; do
  i=$((i+1)); [ $i -lt 100 ] || { echo "no health"; cat "$D/gw.log"; exit 1; }; sleep 0.1; done
echo "health: $(cat "$D/health.json")"
grep -q '"status":"ok"' "$D/health.json"
code=$(curl -s -o "$D/chat.json" -w '%{http_code}' -m10 -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d '{"model":"mock","messages":[{"role":"user","content":"gate smoke"}]}' http://127.0.0.1:$PORT/v1/chat/completions)
echo "chat (mock provider): HTTP $code $(head -c 200 "$D/chat.json")"
[ "$code" = 200 ]
noauth=$(curl -s -o /dev/null -w '%{http_code}' -m5 http://127.0.0.1:$PORT/v1/models)
echo "no-key /v1/models: HTTP $noauth"
[ "$noauth" = 401 ]
noauth_chat=$(curl -s -o /dev/null -w '%{http_code}' -m5 -H 'Content-Type: application/json' \
  -d '{"model":"mock","messages":[{"role":"user","content":"x"}]}' http://127.0.0.1:$PORT/v1/chat/completions)
echo "no-key chat: HTTP $noauth_chat"
[ "$noauth_chat" = 401 ]
echo "deploy smoke ok"
