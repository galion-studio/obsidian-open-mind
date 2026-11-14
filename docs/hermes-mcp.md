<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# MCP servers for Hermes

Written 2026-11-15T00:40:51+01:00 (Europe/Zagreb).

The MCP client lives in `crates/hermes-rs/src/mcp.rs`. Per-server configuration, including the read-only GitHub entry, lives in `crates/hermes-rs/src/config.rs`. `hermes-gateway` calls `hermes_rs::mcp::discover` at startup and registers whatever comes back. `crates/hermes-tools` is a different thing: the sandboxed `fs_read`, `fs_write`, `git`, and `cargo` tools. It is not the MCP client.

`HERMES_RS_MCP=0` (also `false`, `no`, `off`) drops every configured server, including GitHub. Nothing here edits `config.yaml`. The GitHub entry stays off until `HERMES_RS_MCP_GITHUB=1`.

## What the client speaks

Two transports, same JSON-RPC methods: `initialize`, `notifications/initialized`, paged `tools/list`, `tools/call`.

| transport | how a server is selected | notes |
|---|---|---|
| stdio | `command` and `args` | Newline-delimited JSON-RPC on the child's stdin/stdout. The child environment is cleared. Only `PATH`, `HOME`, `LANG`, `TZ`, `USER`, `HERMES_HOME`, and `token_env` (when set) are passed through. |
| streamable HTTP | `url` | Protocol 2025-03-26. One HTTP POST per message. `Accept` lists both `application/json` and `text/event-stream`. A JSON body or an SSE stream are both accepted. `Mcp-Session-Id` from the initialize response is sent on later requests. A later HTTP 404 drops the session and reconnects once. |

An entry has exactly one of `command` or `url`. Anything else is ignored.

`http://` is accepted only for `127.0.0.1`, `localhost`, and `::1`. Any other cleartext URL is refused before a socket is opened. `https://` is sent through the system `curl`, with the request config on stdin, so the URL and the token are not process arguments. Redirects are not followed.

The legacy HTTP+SSE transport (protocol 2024-11-05) is not implemented.

## Allowlist

`tools` is the allowlist: only those names are registered, and a call to any other name is refused before a request is sent. Omit `tools` to register every tool the server lists.

Tool names the model sees are `mcp__<server>__<tool>`, with characters other than ASCII letters, digits, and `_` replaced by `_`, clamped to 64 characters.

```yaml
mcp_servers:
  lab:
    command: /usr/local/bin/lab-mcp
    args: ["--stdio"]
    timeout: 30
    connect_timeout: 15
    tools: [list_notes, get_note]
```

A loopback HTTP server uses `url` instead of `command`:

```yaml
mcp_servers:
  lab:
    url: http://127.0.0.1/mcp
    tools: [list_notes, get_note]
```

Header values are strings. Quote `true` if you send it as a header. `Authorization` is not accepted as a header; put the variable name in `token_env` and the secret in the environment. A `token_env` that looks like a token, or a header that looks like one, drops the whole entry.

`timeout` and `connect_timeout` are seconds. Defaults are 30 and 15. The GitHub entry uses 60 and 15. `lazy` is accepted and stored. Discovery still runs at startup, which is the existing behaviour.

## GitHub, read-only

`HERMES_RS_MCP_GITHUB=1` (also `true`, `yes`, `on`) adds one server named `github` when config does not already have one:

| field | value |
|---|---|
| url | `https://api.githubcopilot.com/mcp/x/all/readonly` |
| header | `X-MCP-Readonly: true` |
| token | environment variable `GITHUB_TOKEN`, read when a request is sent |
| tools | `GITHUB_READONLY_TOOLS` in `crates/hermes-rs/src/config.rs` |

The URL and the header are the read-only mode documented by `github/github-mcp-server` (`docs/server-configuration.md` and `docs/remote-server.md`). The allowlist is the README `## Tools` headings whose summary is get, list, search, or read, as fetched 2026-11-15. Write tools are not in that list. `mark_all_notifications_read` is a write, so it is not in the list either.

The same clamp applies to any server named `github` and to any URL on `api.githubcopilot.com`. An explicit `tools` list keeps only the intersection. A write name in config is dropped. If that leaves the allowlist empty, discovery does not connect and reports `allowlist is empty`.

```yaml
mcp_servers:
  github:
    url: https://api.githubcopilot.com/mcp/x/all/readonly
    token_env: GITHUB_TOKEN
    tools: [get_file_contents, issue_read, pull_request_read, search_code]
```

Set the token in the environment that starts the gateway. Do not write it in `config.yaml`, in a header, or in the URL. If `GITHUB_TOKEN` is unset, discovery records the error and does not open a connection. `HERMES_RS_MCP=0` still wins over `HERMES_RS_MCP_GITHUB=1`.

`HERMES_RS_PROVIDER_LOOPBACK_ONLY=1` refuses every non-loopback MCP URL, including GitHub, before `curl` is started.

## Adding another server

1. Run the server yourself, on stdio or on loopback HTTP, or give an `https://` URL you mean to call.
2. Add an entry under `mcp_servers` with either `command` or `url`.
3. Set `tools` to the names that server should be allowed to expose. Leave it unset only when every tool it lists is acceptable.
4. If it needs a bearer token, set `token_env` to the variable name and export the variable. The client sends `Authorization: Bearer <value>` on HTTP, or passes that one variable through to a stdio child.
5. Start the gateway with `HERMES_RS_MCP` unset (or not `0`). A server that fails to start is logged and skipped. The gateway still starts.
6. Confirm the registered names. They look like `mcp__<server>__<tool>`.

Do not name a non-GitHub server `github`. That name is clamped to the GitHub read-only list.
