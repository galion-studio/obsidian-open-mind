<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: console-rust-serve (wave-4 slot 302)

Europe/Zagreb (CET +01:00). Box only. **No** `console.service` change, no VPS write, no CF DNS/HADES.

## Goal

Rust static + API front for live `console-dist` cutover prep: serve `console-dist/static`, honest local `/api/v1/health`, unpaired session phone-gate parity with Node srvx, optional Rust UI pages. Prefer in-tree `console-static` (no axum/tower-http).

## Branch

- `swarm/console-rust-serve-impl` (slot 268 abandoned the name `swarm/console-rust-serve`)
- Base: `console-pairing-e2e` `45a0806d` + merge `console-v2-sprite-health` `58726e7c` (clean)

## Ports (slot 302)

| port | use |
|---|---|
| 21610 | `galion-console-serve` bind |
| 21611 | optional read-only SSH forward → VPS `127.0.0.1:8080` |

## How to run

```bash
export PATH=/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=/workspace/swarm/target/console-rust-serve-impl
cd /workspace/swarm/wt/console-rust-serve-impl
nice -n 10 cargo test --release -p console-static -p console-ui -j 4
GALION_REAL=1 CONSOLE_RUST_SERVE_LIVE=1 nice -n 10 cargo test --release -p console-static \
  --test real_console_rust_serve -- --nocapture
```

Fixture: `/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static` (scp of live static, 46 files).

## Live srvx baseline (read-only, 2026-10-27 ~18:17 CET +01:00)

`ssh slavik-vps` GET only against `127.0.0.1:8080` (Node srvx → `/opt/galion/console-dist`):

| path | live status | notes |
|---|---|---|
| `/` | 302 → `/console` | |
| `/health` | 404 | HTML error page (~3 KiB); **not** health JSON |
| `/api/v1/health` | 200 | Node board JSON (`galion-control` / `3.8.3-board`) |
| `/api/v1/session` | 401 | `{"error":"phone required"}` (26 B) |
| `/api/v1/session-pack` | 401 | same |
| `/favicon.svg` | 200 | from `static/` |
| `/package.json`, `/@vite/client`, `/.env` | 404 | good (no Vite leak) |

## Rust `--dist` results

Recorded in the GALION_REAL run (fill timestamps on execute):

| path | rust status | parity |
|---|---|---|
| `/` | 302 | match |
| `/health` | 404 plain | match status (body shorter; no HTML SSR 404) |
| `/api/v1/health` | 200 | status match; **body honest Rust** (`service=galion-console-static`, `implementation=rust`) — does **not** claim Node board |
| `/api/v1/session` (+pack) | 401 phone required | match unpaired |
| `/favicon.svg`, manifest | 200 | match |
| scanner paths | 404 | match |
| `/console` | 200 Rust HTML | **intentional difference**: live is TanStack SSR; Rust is console-ui (zero JS) |

## What this is / is not

- **Is:** box-ready binary (`galion-console-serve` alias) + staged unit proposal for owner cutover.
- **Is not:** live cutover. Remaining API surface (`/api/v1/services`, hermes, mesh, …) still needs `--upstream` to Node/Rust services or further ports.
- Session **documents** are not invented locally (paired → 501 pointing at `--upstream`).

## Owner cutover steps (NOT done here)

1. Build release `galion-console-serve` (x86_64-musl as usual).
2. Install binary under `/opt/galion/bin/` (owner).
3. Point staged unit at live `/opt/galion/console-dist` with `--dist` + `--ui rust` + optional `--upstream` for remaining API.
4. Dry-run on a spare port; compare nerve/console probes.
5. Only after written approval: swap `console.service` ExecStart (keep Node unit as rollback).

## GALION_REAL run (recorded)

- When: 2026-10-27 18:24:22 CET (+0100)
- Command: `GALION_REAL=1 CONSOLE_RUST_SERVE_LIVE=1 cargo test --release -p console-static --test real_console_rust_serve -- --nocapture`
- Result: **ok** (1 passed, 9.39 s) including live SSH forward status parity on 21611.
- Unit/integration: `cargo test --release -p console-static -p console-ui`: console-static lib 33 passed (incl. 6 api + dist config); console-ui 107 passed; 0 failed.
