<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# openmind-mcp-stack: Hermes MCP server for memory and the Obsidian vault

Written 2026-10-29 (+01:00) Europe/Zagreb on `swarm/openmind-mcp-rust`. Replaces the
Python `galion-stack` MCP entry (`stack-read.py`).

## What it is

A stdio MCP server (newline JSON-RPC, protocol 2025-03-26) that Hermes (`hermes-rs`
`mcp.rs`) starts as a child. Hermes names its tools `mcp__openmind__<tool>`.

| tool | does | limits |
|---|---|---|
| `memory_search` | `POST /search/memories` on openmind-memory; if 0 structured hits, conversation matches are attached | query 2000 chars, limit 1..20 |
| `memory_conversations` | `POST /search/conversations` | same |
| `memory_recall` | `POST /recall` (scope `session` with a session_key, else `all`) | same |
| `memory_save` | `POST /capture` into session `hermes-mcp-notes` (or `--save-session`) | 8000 chars |
| `vault_list` | list notes under a folder, with size and Zagreb mtime | 1000 entries |
| `vault_read` | read one note | default 32 KiB, max 64 KiB |
| `vault_search` | case-insensitive text search | 50 hits, 5000 files, 1 MiB per file, 32 MiB total |
| `openmind_status` | which parts are configured, memory /health | - |

Tools for a part that is not configured are not listed. Errors come back as MCP
`isError` results; the process does not exit.

## Safety

- Vault: read-only by design (no write API exists). Paths are relative; absolute
  paths, `..`, `.`, hidden components (`.obsidian`, `.git`), NUL and backslash are
  refused; the canonical path must stay under the canonical root; symlinks are not
  followed in walks and a symlink that resolves outside the root is refused. Only
  `.md`, `.markdown` and `.txt` are read. Root `/` is refused.
- Memory: only `http://` loopback URLs with no path or userinfo. The Bearer key comes
  from `--key-file` (only the three key names above are read) and is never printed
  (`Debug` redacts it). Replies are capped at 1 MiB; tool text at 256 KiB.
- Live: systemd `ReadOnlyPaths=` on the vault root (deploy/staged/openmind-mcp).

## Run

```
openmind-mcp-stack --vault <dir> [--memory-url http://127.0.0.1:8420] [--key-file <env>]
                   [--save-session <key>] [--timeout-ms N] [--no-memory] [--check] [--version]
```

`--check` prints `tools=...` and exits 0. Bad arguments exit 2.

## Tests

`cargo test -p openmind-mcp-stack`: 11 stdio tests (temp vault with symlink escapes,
hidden dirs, a 300 KiB note, a mock memory server that checks the Bearer header and
bodies) and 2 `hermes_roundtrip` tests that drive the real `hermes_rs::mcp` client
against the built binary.

## Third-party

No new third-party crates (serde_json is already in the workspace). See
docs/third-party/openmind-mcp-rust.md.

## Sandbox proof (VPS, 2026-10-29 14:03 +01:00)

Transient systemd unit as `nobody`, `PrivateNetwork=yes` (loopback only), seccomp
`@system-service` (`Seccomp: 2`), `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`,
`/opt/galion`, `/root` and the live memory/hermes/nerve state dirs inaccessible, private work
folder under /var/lib. hermes-gateway `--mock` (memory client on, MCP on) + this server + a
throwaway openmind-memory: `mcp: 8 tools`; vault_list/read/search returned the copied notes;
`../` was refused; memory_save -> `l0_recorded`; memory_conversations and memory_recall found the
saved turn; vault tree sha256 unchanged. No gate (`/opt/galion/gate`) was installed at the time.
