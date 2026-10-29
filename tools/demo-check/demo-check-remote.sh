#!/bin/bash
# SPDX-License-Identifier: MIT
# demo-check-remote.sh: runs ON the VPS (piped through ssh by demo-check.sh).
# Read-only except: the demo chats themselves (River model, a few cents at most, usually $0)
# and the memory rows those chats save. Never prints keys; never prints the host address.
# Env: PORT (18080), MODE (full|quick), CODEWORD (default: unique per run), MODEL, PROVIDER.
set -u
PORT=${PORT:-18080}; MODE=${MODE:-full}
PROVIDER=${PROVIDER:-river}; MODEL=${MODEL:-deepseek-ai/DeepSeek-V4.1-Flash}
# Default: a fresh, unique codeword per run. Old test chats (aurora-omfix, blue-harbor-*, demo-test-*,
# TEST-T395...) cannot be deleted, so reusing one would make recall ambiguous. EXACT=1 uses blue-harbor-42.
# The model is always passed explicitly (River deepseek-ai/DeepSeek-V4.1-Flash, the model behind the
# gateway alias river-flash), because the gateway default is the paid OpenRouter Opus model.
if [ "${EXACT:-0}" = 1 ]; then CODEWORD=blue-harbor-42; else CODEWORD=${CODEWORD:-violet-pier-$(( (RANDOM % 9000) + 1000 ))}; fi
VAULT=/opt/galion/galion-light/docs/build-session
MCP_BIN=$(awk '/^  openmind:/{f=1} f&&/command:/{print $2; exit}' /opt/galion/hermes/config.yaml)
B="http://127.0.0.1:$PORT"; T=$(mktemp -d /tmp/demo-check.XXXXXX)
pass=0; fail=0; warn=0
ok(){ pass=$((pass+1)); printf 'PASS  %-34s %s\n' "$1" "${2:-}"; }
no(){ fail=$((fail+1)); printf 'FAIL  %-34s %s\n' "$1" "${2:-}"; }
wa(){ warn=$((warn+1)); printf 'WARN  %-34s %s\n' "$1" "${2:-}"; }
code(){ curl -s -o "$T/body" -w '%{http_code}' --max-time 20 "$@"; }
vault_hash(){ find "$VAULT" -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-16; }

echo "== services"
for u in console console-rust-primary hermes-rs openmind-memory galion-stt galion-nerve gaia-cortex; do
  st=$(systemctl is-active "$u"); nr=$(systemctl show -p NRestarts --value "$u")
  [ "$st" = active ] && ok "unit $u" "active NRestarts=$nr" || no "unit $u" "$st"
done
for t in guardian-tick.timer; do
  [ "$(systemctl is-active $t)" = active ] && ok "timer $t" active || wa "timer $t" "$(systemctl is-active $t)"
done

echo "== health endpoints"
c=$(code http://127.0.0.1:8642/health); [ "$c" = 200 ] && ok "hermes-rs /health" 200 || no "hermes-rs /health" "$c"
c=$(code http://127.0.0.1:8420/health); [ "$c" = 200 ] && ok "openmind-memory /health" "fts=$(jq -r .fts "$T/body")" || no "openmind-memory /health" "$c"
c=$(code http://127.0.0.1:8651/health); [ "$c" = 200 ] && ok "galion-stt /health" "$(jq -r '.engine+" sessions "+(.sessions_active|tostring)+"/"+(.sessions_max|tostring)' "$T/body")" || no "galion-stt /health" "$c"
c=$(code http://127.0.0.1:8670/health); [ "$c" = 200 ] && ok "nerve /health" 200 || wa "nerve /health" "$c"

echo "== console pages (both ports)"
for p in 18080 8080; do
  c=$(code -H 'User-Agent: Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36' "http://127.0.0.1:$p/hermes")
  if [ "$c" = 200 ] && grep -q 'name="viewport"' "$T/body"; then
    css=$(grep -oE '/_ui/[a-z0-9_.-]+\.css' "$T/body" | head -1); js=$(grep -oE '/_ui/[a-z0-9_.-]+\.js' "$T/body" | head -3 | tr '\n' ' ')
    bad=""; for a in $css $js; do ac=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$p$a"); [ "$ac" = 200 ] || bad="$bad $a=$ac"; done
    [ -z "$bad" ] && ok ":$p /hermes (mobile UA)" "200, viewport meta, assets ok" || no ":$p /hermes assets" "$bad"
  else no ":$p /hermes" "$c"; fi
  c=$(code -H 'Host: console.galion.app' "http://127.0.0.1:$p/hermes")
  { [ "$c" = 200 ] && grep -q 'pair' "$T/body"; } && ok ":$p public host -> pairing gate" "200" || no ":$p public host gate" "$c"
  c=$(code -H 'Host: console.galion.app' "http://127.0.0.1:$p/api/hermes/health")
  [ "$c" = 401 ] && ok ":$p public API needs phone" 401 || no ":$p public API not gated" "$c"
  c=$(code "http://127.0.0.1:$p/api/hermes/health"); [ "$c" = 200 ] && ok ":$p /api/hermes/health" "auth_ok=$(jq -r .auth_ok "$T/body")" || no ":$p /api/hermes/health" "$c"
  c=$(code "http://127.0.0.1:$p/api/stt/health"); [ "$c" = 200 ] && ok ":$p /api/stt/health (dictation)" 200 || no ":$p /api/stt/health" "$c"
  for pr in river openrouter; do
    c=$(code "http://127.0.0.1:$p/api/hermes/models?provider=$pr"); n=$(jq -r '.count // (.models|length)' "$T/body" 2>/dev/null)
    { [ "$c" = 200 ] && [ "${n:-0}" -gt 0 ]; } && ok ":$p model list $pr" "$n models, stale=$(jq -r .stale "$T/body")" || no ":$p model list $pr" "$c n=$n"
  done
  c=$(code "http://127.0.0.1:$p/api/hermes/prefs"); [ "$c" = 200 ] && ok ":$p prefs" "$(jq -c '{provider,models}' "$T/body")" || no ":$p prefs" "$c"
done
pp=$(curl -s "$B/api/hermes/prefs" | jq -r '.provider+" "+(.models.river // "")')
[ "$pp" = "river $MODEL" ] && ok "phone opens on the demo model" "$pp" || wa "phone default model is not the demo model" "prefs: $pp (demo: river $MODEL; paid default = OpenRouter Opus)"
jq -e --arg m "$MODEL" '.models|map(.id)|index($m)' <(curl -s "$B/api/hermes/models?provider=$PROVIDER") >/dev/null \
  && ok "demo model on $PROVIDER list" "$MODEL" || no "demo model missing from $PROVIDER list" "$MODEL"

echo "== model routing (read-only)"
cl=$(jq -r '.llm|(.endpoint|sub("https://";""))+" "+.model' /etc/gaia-cortex/config.json 2>/dev/null)
grep -q 'river' <<<"$cl" && ok "cortex advisor on River AI" "$cl" || no "cortex advisor not on River" "${cl:-unreadable}"
gp=$(curl -s "$B/api/hermes/guard" | jq -r '.provider+" on="+(.on|tostring)')
grep -q '^river on=true' <<<"$gp" && ok "console guard pinned to River" "$gp" || no "console guard" "$gp"
hm=$(systemctl show gaia-hermes-handoff -p Environment --value | tr ' ' '\n' | sed -n 's/^HANDOFF_MODEL=//p')
ali=$(sed -n '/^model_aliases:/,/^[^ ]/p' /opt/galion/hermes/config.yaml | awk -v a="  $hm:" '$0==a{f=1;next} f&&/^  [^ ]/{exit} f&&/model:|provider:/{printf "%s ", $2}')
ok "cortex->Hermes handoff model" "HANDOFF_MODEL=${hm:-unset} ${ali:+($ali)} path=$(systemctl is-active gaia-hermes-handoff.path)"
fr=$(journalctl -u galion-nerve --since -1h --no-pager -o cat 2>/dev/null | grep -c 'memory-stale')
[ "$fr" = 0 ] && ok "memory freshness (nerve, last hour)" "no memory-stale incident" || wa "memory freshness" "$fr memory-stale lines in the last hour"

echo "== MCP tools (stdio tools/list on the live binary)"
if [ -x "$MCP_BIN" ]; then
  names=$(printf '%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo-check","version":"1"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
    | timeout 15 "$MCP_BIN" --vault "$VAULT" --memory-url http://127.0.0.1:8420 --key-file /opt/galion/hermes/.env 2>/dev/null | jq -r 'select(.id==2)|.result.tools[].name' 2>/dev/null | sort | tr '\n' ' ')
  n=$(wc -w <<<"$names")
  [ "$n" -eq 8 ] && ok "MCP tools/list" "$n: $names" || no "MCP tools/list" "$n: $names"
  echo "$names" | grep -qE 'write|delete|create' && no "MCP exposes a write tool" "$names"
else no "MCP binary" "not found: $MCP_BIN"; fi
last=$(journalctl -u hermes-rs -b --no-pager -o cat 2>/dev/null | grep -oE 'mcp: [0-9]+ tools' | tail -1)
[ "$last" = "mcp: 8 tools" ] && ok "hermes-rs loaded MCP" "$last" || no "hermes-rs MCP load" "${last:-none in journal}"

echo "== error handling (no model spend)"
chk(){ local want=$1 name=$2 body=$3; c=$(code -X POST -H 'Content-Type: application/json' "$B/api/hermes/ask" -d "$body"); m=$(jq -r '.error // empty' "$T/body" 2>/dev/null)
  [ "$c" = "$want" ] && ok "$name" "$c: $m" || no "$name" "got $c want $want: $m"; }
chk 400 "reject empty question" '{"input":"  ","provider":"river"}'
chk 400 "reject unknown provider" '{"input":"hi","provider":"nope"}'
chk 400 "reject model not on list" '{"input":"hi","provider":"river","model":"no-such/model-x"}'
chk 409 "runpod not set up -> 409" '{"input":"hi","provider":"runpod"}'
c=$(code -X POST -H 'Content-Type: application/json' "$B/api/hermes/ask" -d 'not json'); [ "$c" = 400 ] && ok "reject bad JSON" 400 || no "reject bad JSON" "$c"
c=$(code "$B/api/hermes/runs/run_doesnotexist0000"); { [ "$c" -ge 400 ] && [ "$c" -lt 500 ]; } && ok "unknown run id -> 4xx" "$c" || no "unknown run id" "$c"

[ "$MODE" = quick ] && { echo "== quick mode: demo chats skipped"; echo "RESULT pass=$pass fail=$fail warn=$warn"; rm -rf "$T"; exit $fail; }

# One chat turn. $1 label, $2 input, $3 session_id ('' = new chat). Writes $T/$1.{ask,ev,run}.
run(){ local lb=$1 inp=$2 sid=$3 body id try
  body=$(jq -nc --arg i "$inp" --arg p "$PROVIDER" --arg m "$MODEL" --arg s "$sid" '{input:$i,provider:$p,model:$m}+(if $s=="" then {} else {session_id:$s} end)')
  for try in 1 2; do   # one retry: River can answer 429 under parallel load
    curl -s --max-time 30 -X POST -H 'Content-Type: application/json' "$B/api/hermes/ask" -d "$body" > "$T/$lb.ask"
    id=$(jq -r '.run_id // empty' "$T/$lb.ask")
    if [ -n "$id" ]; then
      timeout 240 curl -sN "$B/api/hermes/runs/$id/events" > "$T/$lb.ev"
      curl -s "$B/api/hermes/runs/$id" > "$T/$lb.run"
      [ "$(jq -r .status "$T/$lb.run")" = completed ] && return 0
      echo "      try $try: run $(jq -r .status "$T/$lb.run"): $(jq -r '.error // ""' "$T/$lb.run" | cut -c1-160)"
    else echo "      try $try: ask failed: $(jq -c . "$T/$lb.ask" 2>/dev/null | cut -c1-200)"; echo '{}' > "$T/$lb.run"; : > "$T/$lb.ev"; fi
    [ "$try" = 1 ] && { echo "      retrying once in 8 s"; sleep 8; }
  done; return 1; }
tools(){ grep '^data:' "$T/$1.ev" | sed 's/^data: //' | jq -r 'select(.event=="tool.completed") | .tool+(if .error then "(ERR)" else "" end)' 2>/dev/null | tr '\n' ' '; }
deltas(){ grep -c '"event":"message.delta"' "$T/$1.ev"; }
out(){ jq -r '.output // ""' "$T/$1.run"; }
show(){ printf '      tools: %s| deltas: %s | session: %s\n      answer: %s\n' "$(tools $1)" "$(deltas $1)" "$(jq -r .session_id "$T/$1.run")" "$(out $1 | tr '\n' ' ' | cut -c1-260)"; }

echo "== demo chats ($PROVIDER $MODEL, codeword $CODEWORD)"
h0=$(vault_hash)
if run s1 "Remember this for later: my demo codeword is $CODEWORD. Save it to memory." ""; then
  t=$(tools s1); { grep -q 'memory_save ' <<<"$t " && grep -qi "$CODEWORD" <(out s1); } && ok "step 1 save to memory" || no "step 1 save to memory" "tools=[$t]"
else no "step 1 save to memory" "run status $(jq -r .status "$T/s1.run" 2>/dev/null)"; fi; show s1
[ "$(deltas s1)" -gt 0 ] && ok "streaming (message.delta events)" "$(deltas s1) deltas" || no "streaming" "0 message.delta"
s1sid=$(jq -r .session_id "$T/s1.run")
if run s2 "What is my demo codeword? Check your memory." ""; then
  s2sid=$(jq -r .session_id "$T/s2.run"); t=$(tools s2)
  if [ "$s2sid" = "$s1sid" ]; then no "step 2 recall (new chat)" "same session reused"
  elif grep -qi -- "$CODEWORD" <(out s2) && grep -qE 'memory_(search|recall|conversations)' <<<"$t"; then ok "step 2 recall in new chat" "tools=[$t]"
    others=$(out s2 | grep -oiE '\b[a-z]+-[a-z]+(-[a-z]+)?-[0-9]{2,}(-[0-9]+)?\b' | grep -viF -- "$CODEWORD" | sort -u | tr '\n' ' ')
    [ -z "$others" ] && ok "step 2 answer is unambiguous" || wa "step 2 answer names other codewords" "$others(old test data in memory)"
  else no "step 2 recall in new chat" "tools=[$t]"; fi
else no "step 2 recall" "run status $(jq -r .status "$T/s2.run" 2>/dev/null)"; fi; show s2
title=$(grep -m1 '^# ' "$VAULT/02-goals.md" | sed 's/^# //'); g1=$(grep -m1 '^1\. ' "$VAULT/02-goals.md" | sed 's/^1\. //; s/\..*//')
if run s3 "List the notes in the vault and read 02-goals.md. Give me its title and the first goal." ""; then
  t=$(tools s3); o=$(out s3)
  { grep -q vault_list <<<"$t" && grep -q vault_read <<<"$t" && grep -qi "$title" <<<"$o" && grep -qi "$g1" <<<"$o"; } \
    && ok "step 3 vault list+read" "title='$title' goal1~'$g1'" || no "step 3 vault list+read" "tools=[$t] want title='$title' goal1~'$g1'"
else no "step 3 vault read" "run status $(jq -r .status "$T/s3.run" 2>/dev/null)"; fi; show s3
if run s4 "Write a new note into the vault." ""; then
  t=$(tools s4); o=$(out s4); h1=$(vault_hash)
  if [ "$h0" != "$h1" ]; then no "step 4 vault unchanged" "VAULT HASH CHANGED $h0 -> $h1"
  elif grep -qiE "read[- ]only|can.?t|cannot|not able|unable|won.?t|refus|no write|not allowed|don.?t have" <<<"$o"; then ok "step 4 refuses vault write" "vault hash unchanged ($h0)"
  else no "step 4 refusal wording" "vault unchanged but answer did not clearly refuse"; fi
else no "step 4 refusal" "run status $(jq -r .status "$T/s4.run" 2>/dev/null)"; fi; show s4
tot=0; for s in s1 s2 s3 s4; do n=$(jq -r '.usage.total_tokens // 0' "$T/$s.run" 2>/dev/null); tot=$((tot + ${n:-0})); done
echo "      tokens used by demo chats: $tot"
echo "RESULT pass=$pass fail=$fail warn=$warn codeword=$CODEWORD"
rm -rf "$T"; exit $fail
