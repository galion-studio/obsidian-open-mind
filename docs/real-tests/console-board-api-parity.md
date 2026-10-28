<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Console board API parity (wave-4 slot 303)

Branch `swarm/console-board-api-parity`, based on `2d846fb8` (source of the
live `/opt/galion/rust/bin/galion-console-serve-2d846fb8`). No live deploy.

## Gap found on the live VPS (read-only probes, 2026-10-27 21:02 +01:00)

Live `console.service` on :8080 runs the Rust serve with `--local-api`; the
Node srvx board (galion-control 3.8.3-board) was stopped at the t305u cutover.
`--local-api` only answered `health`, `ping`, `session`, `session-pack`:

| Probe | Live :8080 |
|---|---|
| `/api/v1/{services,domains,repos,catalog,ledger,docs,protocol,protocol/snapshot,markers}` Host 127.0.0.1 | 404 |
| `/api/v1/{cloudflare,pack,analytics,bank,keys,instances,laptop,security,awake,boot,command}` | 404 |
| `/api/v1/admin` | 404 |
| `/console`, `/services`, `/status` with Host `console.galion.app` (cloudflared path) | **503 "Console API is not answering"** |
| Same pages, Host 127.0.0.1 | 200 but empty data |

Root cause of the 503: the Rust UI page gate asks upstream (itself)
`GET /api/v1/admin`; it was 404, so the remote admin console is unusable
since the Rust cutover. Phones cannot pair or open any page.

## What this branch adds (console-static, std only, zero new deps)

- `src/board.rs`: board data from `board/fleet-board.json` (converted from the
  TS `lib/fleet.ts` + `lib/api-catalog.ts`, IPs/keys removed): `services`
  (16, `zone` added like Node), `domains` (15), `repos` (20), `catalog` (37,
  version `3.0.0-cpu`), `ledger`, `docs` (OpenAPI 3.1), `protocol`,
  `protocol/snapshot`, `markers` ring (seq from 41, cap 42, heartbeats not
  stored, kind whitelist, 200-char cut), `boot`.
- `src/admin.rs`: port of `admin-device.ts`: SHA-256 hashes only, code
  upper-cased, ≤4 token hashes, device hash resume, 5 fails → 15 min lock,
  constant-time compare, atomic 0600 writes + mirror, corrupt store → 503
  without overwrite, unwritable → 503 "store not writable" (fail closed).
- `src/api.rs`: Node phone-gate order: foreign host → 401 `phone required`;
  `/api/v1/admin` exempt; loopback Host only from a loopback peer; other
  console hosts need a valid `galion_admin` cookie. Bodies ≤16 KiB, chunked
  → 411, oversize → 413. `session`/`session-pack` stay 501 for admins.
- `src/config.rs`: `--admin-store FILE`, `--admin-mirror FILE`.
- `src/sha256.rs`: in-tree copy of `galion-one/src/sha256.rs`.

Intentional tightenings vs Node (D-303-2): unpaired remote `GET
/api/v1/health` is 401 (Node required admin there too, via the same gate);
foreign hosts cannot reach `/api/v1/admin` (Node let them POST codes).

## Results

| Test | When (+01:00) | Result |
|---|---|---|
| `cargo test -p console-static -p console-ui` (lib + 50 unit, serve, ui*, real_* gated skip) | 2026-10-27 21:08 | pass |
| `GALION_REAL=1 … --test real_board_api_parity` (box, 127.0.0.1:21730) | 2026-10-27 21:10 | pass (flow 24 ms) |
| same + `CONSOLE_BOARD_LIVE=1` (ssh -L 21731 → VPS :8080, prints only) | 2026-10-27 21:11 | pass, table below |
| Workspace gates: `cargo fmt --all --check`, `clippy --workspace --all-targets -D warnings` (excl. openmind-sandbox, cgroup-detect), `cargo test --workspace` (excl. openmind-sandbox, cgroup-detect, hermes-tools) 682 passed, `scripts/no-python.sh` | 2026-10-27 21:15 | pass |
| VPS throwaway: musl binary (sha256 prefix `1c8318746a5ddb41`) as `nobody` on 127.0.0.1:21732, `--dist /opt/galion/console-dist`, synthetic store in `/var/tmp`, killed + removed | 2026-10-27 21:12 | pass, table below |

Live vs branch (same requests, VPS 2026-10-27 21:12 +01:00):

| Path | Host | live :8080 | branch :21732 |
|---|---|---|---|
| /api/v1/health | 127.0.0.1 | 200 | 200 |
| /api/v1/services, domains, repos, catalog, ledger, docs, protocol, protocol/snapshot, markers | 127.0.0.1 | 404 | 200 |
| /console, /services, /status, /domains, /markers | 127.0.0.1 | 200 | 200 (now with rows) |
| /api/v1/admin | console.galion.app | 404 | 200 `{"ok":true,"paired":false,"you":false}` |
| /api/v1/services | console.galion.app | 404 | 401 until paired |
| /console, /services | console.galion.app | 503 | 200 pair page |
| `/_ui/pair` POST right code | console.galion.app | — | 303 + admin/device cookies |
| /services, /console with cookie | console.galion.app | — | 200, service rows |

Store after pairing held no code/token text, mirror identical. Live
`console.service` stayed active, health 200 throughout; nerve `/state` read.

Real test also covers: wrong code 401, form pairing, device resume (new
admin cookie), lockout 429 while device resume still works, foreign host
401, 411/413, store mode 0600.

## Owner decisions

- **D-303-1 deploy**: build from this branch, then in the console drop-in add
  `--admin-store /root/galion-secrets/admin-device.json --admin-mirror
  /workspace/galion-light/secrets/persist/admin-device.json` and
  `ReadWritePaths=/root/galion-secrets /workspace/galion-light/secrets/persist`
  (live unit has `ProtectSystem=strict`, `ProtectHome=read-only`). Not done
  here (row says no live deploy). Without ReadWritePaths, existing paired
  cookies verify and new pairing fails closed with 503.
- **D-303-2**: accept the two tightenings above.
- **D-303-3**: port order for the rest (each needs data or outbound calls
  this slot did not take): `pack`/`cloudflare` (compose/Caddy/origin script +
  machine targets; keep IPs out of git), `ping` fleet probe (outbound),
  `awake` (pulse :8091), `command`, `keys`, `analytics`, `security`,
  `instances`, `laptop`, `markers/stream` (SSE), `bank`/`arena`, `mesh`,
  `hermes/*` (desk :8646), `v3/guardian`, session docs. Until then those
  return 404 as today; the Rust UI shows its degraded state for them.

## Reproduce

```
export PATH=/workspace/swarm/bin:$PATH
GALION_REAL=1 cargo-slot test -j 2 --release -p console-static \
  --test real_board_api_parity -- --nocapture
# optional live table (prints only):
GALION_REAL=1 CONSOLE_BOARD_LIVE=1 cargo-slot test -j 2 --release \
  -p console-static --test real_board_api_parity -- --nocapture
```
