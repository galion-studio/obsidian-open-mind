<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# No OpenRouter from the nerve / cortex (2026-10-21)

Policy (Mate, 2026-10-21 18:10 +02:00): the nerve (`galion-nerve`, binary
`openmind-nerve`) and the cortex (the `cortex` module + `sensors/` inside the
same crate) never call OpenRouter, directly or through hermes-rs, galion-one or
keep-warm pings. Hermes on the VPS is the only research/LLM path.

## Audit (source)

| Path | Finding |
|---|---|
| `src/util.rs` `http()` | std TcpStream HTTP/1.0, called only with `lo(port)` (127.0.0.1): `probe.rs` GET `/health`, `/metrics`; `collect.rs` galion-health `/status.json`; tests `/signal`, `/healthz`. No TLS, cannot reach a public host. |
| `src/alerts.rs` | `curl -K -` to `https://api.telegram.org/` only when `NERVE_TELEGRAM_ENV` is set (it is not set on the VPS). |
| `src/reflex.rs` | `systemctl restart` / `galion-deploy` rollback via `systemd-run`; dry-run on the VPS (`70-nerve-dryrun.conf`). Restarting galion-one cannot start keep-warm (galion-one has `ONE_NO_WARM=1` in its own drop-in). Rollback backups exist only for galion-health and tuner. |
| hermes-rs | only `GET /health` (unauthenticated, free). No POST to any hermes or galion-one route. |
| `gaia` exec | `gaia status` if a `gaia` binary exists (none on the VPS); gaia-live itself only GETs `/health` and POSTs to the loopback nerve. |
| strings | no `openrouter`, no `chat/completions` in `crates/openmind-nerve/src` or `crates/nerve-client/src` code (only a doc example string in nerve-client) on aa77864, cortex-merge-all 6bcb882, cortex-protected-hosts 60312ac, cortex-t53-replay-corpus 55f07ff. |

## Guard added on this branch (not deployed)

- `src/egress.rs`: `LLM_ENABLED = false` (const, no env switch);
  `check_addr` (loopback only), `check_path` (refuses `/v1/chat/completions`,
  `/v1/runs`, `/v1/responses`, `/v1/completions`, `/v1/messages`, `/api/chat`,
  `/api/generate`, the `/p/<profile>/` mirror, any path naming OpenRouter),
  `check_url` (external helper URLs: `https://api.telegram.org/` only; any URL
  naming OpenRouter refused), `llm_env_names` (names only).
- `util::http` runs `check_addr` + `check_path` before connecting.
- `alerts` checks the Telegram URL with `check_url` before spawning curl
  (error text never contains the URL, which carries the token).
- `main` logs a `<4>` warning naming (never printing values of) any
  OPENROUTER env var, which the nerve ignores.
- `tests/no_openrouter.rs`: source scan of `src/` (only `egress.rs` may name
  the provider or LLM paths) + the client refuses LLM paths and non-loopback
  addresses with zero connections reaching a local listener.

## Live (VPS) change

`/etc/systemd/system/galion-nerve.service.d/75-no-openrouter.conf`:
`UnsetEnvironment=` of OPENROUTER/OpenAI/Anthropic/Hermes key variable names
and `NERVE_TELEGRAM_ENV`, plus `IPAddressDeny=any` / `IPAddressAllow=localhost`.
The VPS is an LXC container: the cgroup BPF IP filter is not enforced there
(a transient unit with the same two lines still reached a public HTTPS host,
2026-10-21 18:16 +02:00), so the IP lines are intent only; the code is the guard.
Backup `/var/backups/galion-nerve.service-<ts>`; rollback: remove the drop-in,
`systemctl daemon-reload`, `systemctl restart galion-nerve`.
