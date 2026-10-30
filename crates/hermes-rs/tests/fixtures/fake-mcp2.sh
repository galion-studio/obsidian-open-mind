#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Fake stdio MCP server for the generic-client tests: env passing, secret
# scrubbing, writes-off filtering, lazy start counting, server ping.
[ -n "$START_LOG" ] && echo start >> "$START_LOG"
echo "fake2 starting (stderr line)" >&2
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fake2","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"env_names","description":"List env \\u001b[31mnames","annotations":{"readOnlyHint":true}},{"name":"secret_echo","description":"echo"},{"name":"create_thing","description":"writes"},{"name":"big","description":"big output"},{"name":"pinger","description":"server pings first"}]}}\n' "$id" ;;
    *'"name":"env_names"'*)
      names=$(env | cut -d= -f1 | grep -vE '^(PWD|SHLVL|_|OLDPWD)$' | sort | tr '\n' ',')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"%s uid=%s cwd=%s"}]}}\n' "$id" "$names" "$(id -u)" "$(pwd)" ;;
    *'"name":"secret_echo"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"token is %s"}]}}\n' "$id" "$FAKE_TOKEN" ;;
    *'"name":"big"'*)
      big=$(head -c 5000 /dev/zero | tr '\0' 'x')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"%s"}]}}\n' "$id" "$big" ;;
    *'"name":"pinger"'*)
      printf '{"jsonrpc":"2.0","id":"srv-1","method":"ping"}\n'
      IFS= read -r reply
      case "$reply" in
        *'"id":"srv-1"'*'"result"'*) ok=pong-ok ;;
        *) ok=pong-missing ;;
      esac
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"%s"}]}}\n' "$id" "$ok" ;;
    *) ;;
  esac
done
