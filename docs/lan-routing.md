<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# LAN routing: approved discovery rows in the fallback and the VRAM pool

Swarm wave 2, slot 31 (`swarm/lan-routing`). Builds on `swarm/lan-discovery`
(mDNS find/announce, pending provider rows, approve/deny/join) and
`swarm/poseidon` (job queue + VRAM pool).

## Rules

1. **Approved rows only.** `Registry::approved_rows()` (openmind-provider)
   returns only `approved` rows; pending and rejected rows never reach
   `fallback::select_provider` / `route::chain`, so no caller can route to
   them by mistake.
2. **Health comes from `join`.** A LAN row is routable only after
   `openmind-discovery join ID` recorded it `up` (one TCP connect, logged).
   Never joined (`unknown`) or `down` = skipped. Other kinds keep the old
   rule (usable unless `down`).
3. **Order GPU -> LAN -> CPU.** `route::chain(need, &registry, extra)` lists
   every usable row in that order (`fallback::ordered`); `extra` are the
   operator rows (`--gpu URL`, `--cpu URL`). Remote is never listed.
4. **A known device that moved updates only after re-approval.** When `find`
   sees a known row at a new endpoint it stores `pending_endpoint` and logs
   `moved`; routing keeps using the approved endpoint (usually down by
   then, so the CPU fallback serves). The next `approve ID` moves
   `pending_endpoint` into `endpoint`, resets health to `unknown` (log line
   has `moved_from` and `needs: join`), so a fresh `join` is required.
   `deny` never applies a move.
5. **POSEIDON's VRAM pool** (`POST /vram/sync-providers`, also at start):
   an approved LAN/GPU row with `vram_gb` (TXT `vram=24` on the announce)
   becomes a device with that id; pending and rejected rows never create a
   device. A device that names a row follows it (approved / pending ->
   planned / rejected), and a LAN device is healthy only when its row is
   `up`. A device posted by hand that names a non-approved row is kept
   `planned`, whatever its price.
6. The router is a **read-only** consumer of the registry: it never changes
   approval or health. On connect failure, timeout or 5xx it tries the next
   row for that request; a 4xx goes back to the client. Only `http://`
   endpoints on loopback/private/link-local addresses are dialed; the
   router binds loopback only. Payloads are not logged.

## Registry file

Version stays `1`. Two optional fields per row: `vram_gb` (1..=4096) and
`pending_endpoint`. Both are omitted when unset, so older files load and
are re-written byte-for-byte the same; an older binary (with
`deny_unknown_fields`) rejects a file that carries them.

## CLI

```
openmind-discovery route [--registry R] [--gpu URL ...] [--cpu URL] [--chat TEXT]
openmind-discovery serve-route --bind 127.0.0.1:19312 --registry R --cpu http://127.0.0.1:19311
```

`serve-route` serves `POST /v1/chat/completions`, `/v1/completions`,
`/v1/embeddings`, `GET /v1/models` (forwarded), `GET /v1/route` (the chain,
nothing dialed), `GET /healthz`. Answers carry `X-Galion-Provider` and
`X-Galion-Attempts` (e.g. `llamacpp-lan-gpu:connect-failed,cpu-local:ok`).
`stream: true` is refused (400).

## Open points

- POSEIDON does not watch the registry: call `POST /vram/sync-providers`
  after approve/deny/join (or restart). A periodic sync would be a small
  follow-up.
- If an old and a new announcement of the same instance are both live,
  `find` may see either; the real test stops the old announcer first.
- Streaming (`stream: true`) is not proxied.

## Gaia S6.3 real test (wave-4 slot 214, 2026-10-21 +02:00)

`crates/openmind-discovery/tests/real_lan_fallback.rs` (`GALION_REAL=1`,
Rust only, ports 20840-20849) runs two loopback providers, a fake GPU
(registry row kind `gpu`, seeded pending, admitted with `approve`) and a
fake LAN device (found over mDNS on port 20840), plus the CPU fallback and
POSEIDON: approve, route GPU -> LAN -> CPU, GPU hang (SIGSTOP) and death,
LAN death, CPU fallback, moved LAN re-approval, GPU recovery, deny, all
down -> 503. Results and findings (late duplicate on a hung provider,
health only from `join`, no discovery path for GPU rows):
`docs/real-tests/gaia-lan-fallback-test.md`.
