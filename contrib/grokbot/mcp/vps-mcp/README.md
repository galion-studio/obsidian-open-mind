# vps-mcp

MCP (Model Context Protocol) stdio server + CLI, **pure Rust**, that connects over SSH/SFTP to
Slavik's VPS, inspects the project read-only, and copies persona / skill / workflow files to this
computer. No Python anywhere (build or runtime).

- MCP: [`rmcp`](https://crates.io/crates/rmcp) 3.5.1 (official Rust SDK, stdio transport)
- SSH: [`russh`](https://crates.io/crates/russh) 0.64.1 (`ring` backend, no OpenSSL/libssh2), SFTP: `russh-sftp` 3.0.1
- tokio 1.53, serde_json, sha2, humantime

## Build

```sh
. ~/.cargo/env            # rustup toolchain (stable)
cd ~/mcp/vps-mcp
cargo build --release     # -> target/release/vps-mcp
cargo test --release      # unit tests (config parser, allowlist, classifier)
```

Binary: `~/mcp/vps-mcp/target/release/vps-mcp`

## Configuration (one file)

Path resolution: `--config PATH` > `$VPS_MCP_CONFIG` > `~/workflows/vps/config.env`.

The default file is shared with the shell scripts in `~/workflows/vps/bin` and is **only read, never
written** by vps-mcp. It is parsed as shell-style `KEY=VALUE` (quotes, inline `# comments`,
`$HOME`/`${VAR}`/`~` expansion). The file is re-read on every tool call, so edits apply without
restarting the MCP server.

| Key | Meaning | Default |
|-----|---------|---------|
| `VPS_HOST` | host or IP (required) | placeholder |
| `VPS_USER` | SSH user (required) | placeholder |
| `VPS_PORT` | SSH port | `22` |
| `VPS_KEY` | private key (ed25519, no passphrase) | `~/.ssh/id_ed25519_vps` |
| `VPS_CONNECT_TIMEOUT` | seconds | `15` |
| `VPS_PROJECT_ROOTS` | search roots, comma/space/colon separated | `/opt /srv /home /var/www /root /etc/systemd /usr/local` |
| `VPS_KNOWN_HOSTS` | TOFU known_hosts file for vps-mcp | `~/.config/vps-mcp/known_hosts` |
| `VPS_ALLOW_WRITE` | permit `vps_exec` with `allow_write=true` | `false` |
| `VPS_EXEC_TIMEOUT` | seconds per command | `60` |
| `VPS_READ_MAX_BYTES` | cap for `vps_read_file` | `524288` |
| `VPS_PULL_MAX_FILE_BYTES` | per-file cap for pulls | `5242880` |
| `VPS_SKILLS_IMPORT_DIR` / `VPS_WORKFLOWS_IMPORT_DIR` | import bases | `~/skills/imported-from-vps`, `~/workflows/imported-from-vps` |

Aliases accepted: `SSH_HOST`, `SSH_USER`, `SSH_PORT`, `VPS_SSH_KEY`, `VPS_KEY_PATH`, `VPS_IDENTITY_FILE`.

Placeholder values (`CHANGE_ME`, `__FILL_ME_HOST__`, `<...>`, empty) are detected: the server
still starts and every remote tool returns
`VPS host/user not configured in <path> ...`. `vps-mcp init-config [PATH]` writes a placeholder file
(`VPS_HOST=CHANGE_ME`, `VPS_USER=CHANGE_ME`, `VPS_PORT=22`, `VPS_KEY=~/.ssh/id_ed25519_vps`) only if
none exists.

### Connection & host keys

- Public-key auth only, with `VPS_KEY` (no password, no agent).
- Host key: **trust on first use**. The first connection records the server key in
  `VPS_KNOWN_HOSTS` (OpenSSH format, `[host]:port` for non-22 ports). Later connections must match;
  on mismatch the connection is refused with the offered SHA256 fingerprint and the known_hosts
  line. If the server was really reinstalled, check the key some other way, then delete that line.
- Each tool call opens a new connection and closes it when done.

## Register as an MCP stdio server

Command: `~/mcp/vps-mcp/target/release/vps-mcp` (no args, or `serve`). Logs go to stderr.

```json
{
  "mcpServers": {
    "vps": {
      "command": "~/mcp/vps-mcp/target/release/vps-mcp",
      "args": [],
      "env": { "VPS_MCP_CONFIG": "~/workflows/vps/config.env" }
    }
  }
}
```

Manual handshake test (no client needed):

```sh
( printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"sh","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"vps_status","arguments":{}}}'; sleep 5 ) \
  | ~/mcp/vps-mcp/target/release/vps-mcp
```

## Tools

| Tool | What it does |
|------|--------------|
| `vps_config` | Show resolved config (does not connect) |
| `vps_status` | Connect; whoami, id, hostname, uname, uptime, load, nproc, memory, disk, OS, host-key status |
| `vps_exec` | Run a command. **Read-only allowlist** by default (ls, cat, grep, find w/o -exec/-delete, ps, ss, systemctl status/show/cat/list-*, journalctl, docker ps/logs/inspect/compose ps/config, git log/status/diff, ...; pipes OK; `; & > < $` and backticks rejected). `allow_write=true` skips the allowlist only if `VPS_ALLOW_WRITE=true` |
| `vps_list_dir` | SFTP directory listing (name, type, size, mtime) |
| `vps_read_file` | SFTP read with size cap + offset paging; binary detection |
| `vps_find` | `find` by name/path glob under roots (skips node_modules, .git, venvs, caches) |
| `vps_discover_stack` | Finds Galleon, Galleon1, Obsidian, OpenMind: matching paths, systemd units and unit files, compose files (by name or content), docker containers, listening ports, processes. Returns a summary per component |
| `vps_find_agent_files` | Persona (`persona*`, `SOUL*`, `*system_prompt*`, `*.persona.*`, `personas/`), skill (`SKILL.md`, `*.skill.*`, `skills/`), workflow (`workflows/`, `*.workflow.*`) files, flags secret-like ones |
| `vps_pull_agent_files` | SFTP copy to `~/skills/imported-from-vps/<host>/<remote path>`, persona to `~/skills/imported-from-vps/<host>/persona/<remote path>`, workflows to `~/workflows/imported-from-vps/<host>/<remote path>`. Writes `manifest.json` (category, remote/local path, size, sha256, UTC timestamp, status, skipped + reason) in both host dirs. Options: `paths`, `categories`, `roots`, `include_secrets`, `overwrite`, `dry_run` |

Pull safety: writes only inside `imported-from-vps/<host>/`. It never writes to the VPS, and a
local file that has changed is not overwritten unless `overwrite=true`. Secret-like files are
skipped and listed with a reason unless `include_secrets=true`. That covers `.env*`, `*.env`,
`id_rsa`/`id_ed25519`..., `*.pem`/`*.key`/`*.p12`..., `.netrc`, `authorized_keys`, names
containing secret/token/credential/password/private, and any content with a PEM `PRIVATE KEY`
block.

## CLI (same binary)

```
vps-mcp [--config PATH] config | status | discover [ROOT...] | find-agent-files [ROOT...]
vps-mcp pull [--include-secrets] [--overwrite] [--dry-run] [ROOT...]
vps-mcp exec [--allow-write] -- CMD
vps-mcp ls PATH | cat PATH | find GLOB [ROOT...]
vps-mcp init-config [PATH]
```

Output is JSON on stdout; exit code 1 on error.
