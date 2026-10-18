# contrib/grokbot — VPS ops tooling and study docs (MIT)

Contributed by Grok Bot for the Obsidian Open Mind open-source release. MIT, see `LICENSE`.

| Path | What |
|---|---|
| `mcp/vps-mcp/` | Rust MCP server for read-mostly VPS ops over SSH (build: `cargo build --release`; no `target/` shipped) |
| `skills/` | six operator skills (SSH access, stack mapping, health check, logs/error fixing, deploy, routine checks) |
| `workflows/` | four workflow runbooks + `vps/bin/*.sh` scripts, `services.list`, `config.env.example`, `stack-map.template.md` |
| `docs/` | `main.md` (master plan), `study/A-foundation.md`, `quant-office-design.md`, `hermes-openrouter-proposal.md` |

Placeholders: `__FILL_ME_HOST__` / `__FILL_ME_USER__` in `config.env.example`; real host/IP and hostname in docs are replaced by `__VPS_HOST__` / `__VPS_HOSTNAME__`.
Copy `workflows/vps/config.env.example` to `config.env` and fill it in; never commit `config.env` or any key.
Docs are point-in-time snapshots (2026-10-18, Europe/Zagreb CEST +02:00) of one deployment; facts in them are not guarantees about your box.
See `MANIFEST.md` for the file list and sha256 sums.
