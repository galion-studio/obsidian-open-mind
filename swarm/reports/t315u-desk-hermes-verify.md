<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# t315u — desk-hermes live verify (Rust binary, not Python)

Written **2026-10-27 19:59 CET** (Europe/Zagreb). Mate t315u + Titanaxe screenshot t317u
(CPU 0%, RAM 119 MB / 16 GB, IP 51.38.137.193). Coordinated with sand-subagent cutover
(`t314u-desk-hermes-cutover.md`); did **not** fight wiring — read state, then independently
proved live `/desk` path.

## Result: WIRED = YES

Live `GET http://127.0.0.1:8646/desk` forks **`/opt/galion/rust/bin/desk-hermes`** (Rust ELF).
`python3 …/desk-hermes.py` is **not** exec'd. `desk-hermes.py` still on disk (rollback).

## Independent proof (this agent, ~18:58:52 UTC / 19:58 CET)

| Check | Result |
|---|---|
| `systemctl is-active galion-desk` | `active` |
| MainPID | `264677` → `/opt/galion/rust/bin/galion-desk` |
| Drop-in | `/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf` |
| Process environ | `DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes`, `DESK_BIND=127.0.0.1:8646` |
| Listen | `127.0.0.1:8646` |
| **strace** `-e execve -p MainPID` during `/desk` | `execve("/opt/galion/rust/bin/desk-hermes", …) = 0` (pid 267899) |
| pgrep during request | `267899 /opt/galion/rust/bin/desk-hermes` |
| `LIVE_PY` | **NO** |
| `LIVE_RUST_DH` | **YES** |
| `STRACE_RUST` / `STRACE_PY` | **YES** / **NO** |

## Smoke HTTP codes

| Path | Code |
|---|---|
| `GET /desk` | **200** |
| `GET /health` | **200** |
| `GET /` | **404** (expected; no root handler) |

Response shape: `foundation=rust`; mindpack dict with **13** keys
(`cache, cached, context, history, mind, models, recent, road, roles, sessions, stores, tasks, tools`).
Top-level keys include `at, board, ctx, events, focus, foundation, heard, hermes, hold, line, mindpack, model, notes, one, resting, said, services, tunnel, turns, voice` (+2).

## SHAs (sha256)

| Path | SHA256 |
|---|---|
| `/opt/galion/rust/bin/desk-hermes` | `89bc45cd42db326c0ce1ff0cadd3be21649b641fa7ada232c9af3b5aa4889762` |
| `/opt/galion/rust/bin/galion-desk` | `889737d0337e7a3a087348566be29f0c2c57ee79db6eece61ea5463ff72adb5f` |

`.py` rollback present: `/opt/galion/bin/desk-hermes.py` (7889 B) — **not deleted**.

## Wiring (unchanged this ticket)

Already set by t314u cutover (~19:56 CET restart):

```
# galion-desk.service.d/rust-desk-hermes.conf
[Service]
ExecStart=
ExecStart=/opt/galion/rust/bin/galion-desk
Environment=DESK_BIND=127.0.0.1:8646
Environment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes
```

No further restart required — live proof matched cutover claims.

## Constraints honored

- No force-push; no secrets in report; no delete of `desk-hermes.py`.
- Did not race cutover agent; verified after unit already active with Rust ExecStart.
- Titanaxe panel (t317u): guest CPU 0% — corroborates idle guest vs host loadavg noise.

## Summary line for parent

`wired=YES smoke=/desk:200 /health:200 /:404 SHA_desk-hermes=89bc45cd42db326c0ce1ff0cadd3be21649b641fa7ada232c9af3b5aa4889762`
