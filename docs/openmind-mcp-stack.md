<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# openmind-mcp-stack: Hermes's MCP server for OpenMind memory and the vault

`openmind-mcp-stack` is a stdio MCP server (JSON-RPC, one message per line,
protocol `2025-03-26`). Hermes (`hermes-rs`) starts it from `config.yaml`
`mcp_servers:`, and the tools show up as `mcp__<server>__<tool>`.

It replaces the old stub that only returned sample `stack_status` data. It has
no fixture data. Every answer comes from the live memory API or the vault on
disk.

## Tools

| Tool | Backend | Notes |
|---|---|---|
| `memory_search` | `POST /search/memories`, `/search/conversations` | `kind` = memories, conversations or both; `limit` up to 20 |
| `memory_recall` | `POST /recall` | `scope` = session or all. If the server doesn't echo the session scope back, the call fails closed |
| `memory_save` | `POST /capture` | `text` up to 8000 chars, saved under the session key (default `hermes-mcp`) |
| `memory_recent` | `POST /recent` | newest L0 rows first, `limit` up to 50; an older server without `/recent` gets an upgrade message |
| `vault_list` | vault dir | `*.md` notes only, with mtimes in Europe/Zagreb |
| `vault_read` | vault file | `.md` only; 64 KiB by default, 256 KiB maximum |
| `vault_search` | vault files | case-insensitive search over line text and file names |

## Safety

- The memory URL must be loopback http with an explicit port (`127.0.0.1`,
  `localhost` or `[::1]`). Anything else stops the server at start-up (exit 2).
- The Bearer key comes from an environment variable named by `--key-env`
  (default: `OPENMIND_MEMORY_API_KEY`, then `TDAI_GATEWAY_API_KEY`). The key is
  never printed or returned in a result.
- The vault is read-only. There are no write tools. A write-like tool name
  (`vault_write`, `*_delete`, …) gets a "refused: the vault is read-only"
  error. Paths are refused if they are absolute or contain `..`, dot
  components (`.git`, `.obsidian`), a symlink at any level, NUL, a backslash,
  or more than 1024 bytes. The canonical path must stay inside the root, and
  the inode is checked again after open.
- In production, Hermes's unit has `ProtectSystem=strict`, so the OS also keeps
  the child from writing the vault.

## Flags

```
openmind-mcp-stack [--memory-url URL | --no-memory] [--vault DIR | --no-vault]
                   [--session-key KEY] [--key-env NAME]... [--timeout-s N]
```

Environment fallbacks: `OPENMIND_MCP_MEMORY_URL` (default
`http://127.0.0.1:8420`), `OPENMIND_MCP_VAULT`, `OPENMIND_MCP_SESSION_KEY`.

## Hermes config

`hermes-rs` clears the child's environment. The `pass_env` list names the
extra variables to copy across (names only; values are never logged):

```yaml
mcp_servers:
  openmind:
    command: /opt/galion/rust/bin/openmind-mcp-stack
    args: ["--vault", "/opt/galion/galion-light/docs/build-session", "--session-key", "hermes-mcp"]
    timeout: 30
    connect_timeout: 15
    pass_env: [TDAI_GATEWAY_API_KEY]
```

The gateway logs `mcp: N tools: [...]` at start-up. `HERMES_RS_MCP=0` turns
every MCP server off.

## Tests

- Unit tests: argument clamps, write-like names, loopback-only URLs, flag
  parsing.
- `tests/stdio.rs`: runs the real binary over stdio against a real
  `openmind-memory` app on `127.0.0.1:0` and a temp vault that has symlinks
  pointing out, `.git`, `.obsidian` and a non-`.md` file. It covers save →
  recall → search → recent, session-scope fail-closed, a wrong key, memory
  being down, every vault refusal, and a byte-for-byte snapshot of the vault
  taken before and after.
