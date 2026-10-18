#!/usr/bin/env bash
# Fake stdio MCP server for hermes-rs tests (newline-delimited JSON-RPC).
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*'"cursor"'*)
      printf '{"jsonrpc":"2.0","method":"notifications/message","params":{}}\n'
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"crash","description":"exit"}]}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"stack-status","description":"Stack status","inputSchema":{"type":"object","properties":{"unit":{"type":"string"}}}}],"nextCursor":"p2"}}\n' "$id" ;;
    *'"name":"crash"'*) exit 3 ;;
    *'"method":"tools/call"'*)
      unit=$(printf '%s' "$line" | sed -n 's/.*"unit":"\([a-z.-]*\)".*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"%s active"}],"isError":false}}\n' "$id" "$unit" ;;
    *) ;;
  esac
done
