#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Mock stdio MCP server for the MCP-writes tests. Every tools/call it
# receives is appended to $CALL_LOG, so a test can prove whether a call
# reached the server. `argv` reports the server's own arguments.
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"mockw","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"argv","description":"server argv","annotations":{"readOnlyHint":true}},{"name":"push_files","description":"push files","inputSchema":{"type":"object","properties":{"files":{"type":"array"},"message":{"type":"string"},"token":{"type":"string"}}}},{"name":"create_or_update_file","description":"write one file","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}}}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*)
      [ -n "$CALL_LOG" ] && printf '%s\n' "$line" >> "$CALL_LOG"
      case "$line" in
        *'"name":"argv"'*)
          printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"argv=%s"}]}}\n' "$id" "$*" ;;
        *)
          printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"written ok, secret was %s"}]}}\n' "$id" "$MOCK_SECRET" ;;
      esac ;;
    *) ;;
  esac
done
