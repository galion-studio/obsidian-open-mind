<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: console-rust-serve (slot 302)

No new crate dependencies were added.

## Considered, not taken

| option | licence | why not |
|---|---|---|
| `axum` + `tower-http` `ServeDir` | MIT | Task allowed it, but in-tree `console-static` already indexes + serves static files with stricter path rules (no listing, no dotfiles, whitelist MIME). Prefer reuse (RULES: minimal deps). |
| `hyper` / `tokio` | MIT | Same: std threaded accept loop already in `console-static`. |

## Reused in-tree

- `crates/console-static` — static index, HTTP/1.1, proxy, new `--dist` / `--local-api`
- `crates/console-ui` — phone/host gate helpers, Rust pages when `--ui rust`

## External material (read-only)

- Live VPS `/opt/galion/console-dist/static` copied to box artifacts for parity tests (GET/scp only; no VPS write).
