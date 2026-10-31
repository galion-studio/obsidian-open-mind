<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Console phone home (`/phone`)

2026-10-31 (CET, +01:00). Owner-approved Android admin first screen: one
phone-sized page **inside the console** (`https://console.galion.app/phone`),
opened in the phone's browser. No app, no device flash; the phone is a
browser client paired like any other console device.

## What the page shows

Top, refreshed every 30 s (times in Europe/Zagreb):

| Tile | Source (read-only) |
| --- | --- |
| Hermes up/down + build id | hermes-rs `GET /health`; build id = effective `ExecStart` binary of `hermes-rs.service` (unit file + `.d/*.conf` drop-ins, systemd order, last non-empty wins), e.g. `hermes-gateway-6da98fdc`; nerve's `hermes-rs` component status |
| OpenRouter fresh spend vs $10 limit | `spend.json` snapshot from openmind-mcp-stack publish-status (`fresh_usd`, `limit_usd` 10, `stop_line_usd` 9.50, `baseline_usd` 3.449, today); bar with the stop-line marker; level computed from the numbers (ok / near = within $0.50 of the stop line / stop / over); snapshot older than 30 min is marked OLD. `top_sessions` / `sessions` are never passed through. |
| Watchdog: waiting for a decision | gaia-cortex `GET /health`, `GET /escalations` (open), `GET /approvals` (pending actions; older cortex answers 404 and is skipped). Newest 5 listed, actions first. Display only: approving a cortex action still needs its own bearer flow. |
| Health | cortex (`/health` ok + mode), nerve (`/health`: status, system state, score, state age), console (this answer + nerve's `console` component), backups line from `backup.json` |

Below: the **same approval cards as galion-one `/approvals`** (same markup,
CSS and JS, served from `galion_one::approvals::parts`), with the same
owner admin key flow (passkey RP `galion.app`, or Ed25519 key file),
the same signed-body checks and the same root-only deploy step.

## Routes (console-static `--phone`, needs `--local-api`)

- `GET /phone`: the page (no data in it; CSP `default-src 'none'`, inline
  sheet + script only, `connect-src 'self'`, `frame-ancestors 'none'`).
  Served on console hosts only.
- `GET /api/v1/phone/status`: the tiles' JSON.
- `/api/approvals*`, `/api/owner-key*`: `galion_one::approvals::route`
  in-process (galion-one now has a library half, `crates/galion-one/src/lib.rs`).

All API routes sit behind the existing console gate: console host, and a
loopback peer+host or a paired `galion_admin` cookie from the admin store.
Unpaired phones get 401 and the page says "pair this phone first".
POSTs also need an `Origin` in `--webauthn-origin` (default
`https://console.galion.app`, `https://galion.app`, `https://one.galion.app`).

## Flags

`--phone`, `--nerve ADDR` (127.0.0.1:8670), `--cortex ADDR` (127.0.0.1:8674),
`--hermes ADDR` (127.0.0.1:8642), `--status-dir DIR`
(/var/lib/openmind-mcp-stack/status), `--unit-dir DIR` (repeatable; systemd
dirs), `--hermes-unit NAME`, and the galion-one approval paths
`--approval-queue`, `--approval-state`, `--approval-out`, `--owner-pub`,
`--sandbox-pub`, `--stack-pub`, `--webauthn-rp-id`, `--webauthn-origin`.
Defaults are the server layout (same as galion-one's).

## Deploy notes (not done; owner's call)

1. Build `galion-console-serve` from this branch; install as a new
   `/opt/galion/rust/bin/galion-console-serve-<commit>`.
2. Console unit drop-in: add `--phone` to `ExecStart` (plus `--hermes
   127.0.0.1:8642` if not default).
3. Sandboxing: `ReadWritePaths=/var/lib/galion-one/approval` (shared with
   galion-one; challenge/approve writes are rename-based) and write access
   to the queue entry `owner/` dirs (or `--approval-out`); read of
   `/etc/hermes-self-modify/*.pub` (owner, stack, sandbox-v2); read of the
   status dir (files are `0640 root:nogroup`, so the console needs to run as
   root or get the group).
4. Passkeys: RP `galion.app` works on `console.galion.app`. Key-file users
   must import the key file once on the console origin (IndexedDB is per
   origin).
5. Restart the console unit, then check `/phone` from the paired phone.
