<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# POSEIDON: `openmind-queue`

Written 2026-10-20T19:35:00+02:00 (Europe/Zagreb). Branch `swarm/poseidon`.

## Scope (from the docs)

- ws/gaia `gaia/docs/gaia-design.md` section 4.2: POSEIDON = "compute
  resources and budgets", systems `vram-pool`, `quotas`, `queues`,
  `budget-caps`; "real money (Revolut, payments) is not approved and is
  refused"; spend **caps only**.
- `TEACHER/BACKLOG.md`: B34 (ComputeProvider registry with approval state),
  B38 (provider fallback GPU -> LAN -> CPU) are POSEIDON rows. POS-1 (raise
  Hermes spend cap) and POS-2 (RunPod / GPU, target >= 1500 GB VRAM) are
  owner-only budget items.
- `docs/STATUS.md` POS-1 and POS-2 rows: owner, not approved.

This crate is the real (not simulated) implementation of the four systems.
It never spends, never calls a paid API and never talks to a GPU provider.

## What it is

One crate, one binary `openmind-queue` (about 0.75 MB, x86_64 musl-able):

| Module | What |
| --- | --- |
| `engine` | Event-sourced queue state machine: named queues, priority (higher first, FIFO within a priority), leases with visibility timeout and fencing tokens, heartbeat `extend`, retries with exponential backoff, dead-letter queue and redrive, per-agent quotas (`max_pending`, `max_inflight`), backpressure at `max_depth` (503), spend caps per agent and global that hard-stop (reserve at lease, charge at ack, over-reports clamped to the reservation; a worker that dies is charged its whole reservation), money-word job kinds refused (403), VRAM routing at lease time. |
| `store` | Append-only log `queue.log`, one `crc32 json` line per record, `sync_data` before every answer, torn-tail repair on open, refusal to start on mid-file corruption, atomic compaction (tmp + rename + dir fsync), `flock` on `queue.lock` against two servers on one directory. |
| `refusals` | `refusals.dat` sidecar: the refusal counters across `kill -9` (two alternating crc-checked slots, written before the answer, synced by the ticker within ~1 s). See Limits. |
| `vram` | Device registry (`gpu`, `lan`, `remote`; `planned`/`approved`/`rejected`; healthy), best-fit placement GPU -> LAN -> remote, CPU offload when the job allows it, capacity summary against the 1500 GB goal (approved, planned, used, gap, cents/hour as numbers). A priced device cannot be approved by hand: it needs an approved `openmind-provider` row (the owner). |
| `gaia` | Gaia vocabulary without a gaia-core dependency (gaia-core is not on main): `FUNCTION = "POSEIDON"`, the four systems, verbs `status`/`run`/`plan`/`execute`, outcomes `done`/`dry-run`/`refused`/`rejected`, report = summary + integer metrics, load 0..100 for `Health::ok`. Dispatch is read-only. |
| `http`, `server` | std-only HTTP/1.1 (Content-Length bodies, 1 MiB cap, connection cap 256), loopback bind by default (non-loopback needs `--allow-remote` AND `--token-file`, see Auth below), a ticker thread that sleeps on a condvar until the next deadline (max 1 s, no busy loop). |

## HTTP API

| Method and path | Body / result |
| --- | --- |
| `GET /healthz` | `{ok, function, service, version, time}` |
| `GET /stats` | depth, counters, budgets per agent, VRAM summary, store info, `refusals_sidecar` (seq, restored, bad_slots, writes, syncs), load |
| `POST /jobs` | `{queue, agent, kind, priority, max_attempts, cost, lease_ms, payload, vram_gb, cpu_fallback}` (all optional) -> 201 `{id}`; 400 bad, 402 budget, 403 money, 429 quota, 503 backpressure or vram_unavailable |
| `GET /jobs/{id}` | live job (acked jobs are not kept) |
| `POST /lease` | `{queue, worker, lease_ms, wait_ms<=30000}` -> `{id, token, device, job}` or `{id:null, miss: empty / budget_hard_stop / quota_inflight / vram_held}` |
| `POST /jobs/{id}/ack` | `{token, cost?}` -> `{charged}`; 409 stale token, 404 gone |
| `POST /jobs/{id}/nack` | `{token, cost?, error}` -> delayed with `retry_at`, or dead |
| `POST /jobs/{id}/extend` | `{token, lease_ms}` |
| `POST /jobs/{id}/redrive`, `GET /dlq` | dead-letter handling |
| `POST /caps` | `{scope: agent or "*", cap: n or null}` (numbers only) |
| `POST /quotas` | `{agent or "*", max_pending, max_inflight}` |
| `GET /vram`, `POST /vram/devices`, `DELETE /vram/devices/{id}`, `POST /vram/sync-providers` | VRAM pool |
| `GET /gaia/status`, `POST /gaia/dispatch` | `{system, verb, payload}` -> Gaia report |
| `POST /admin/compact` | compact the log now |

CLI: `openmind-queue serve | submit | lease | ack | nack | extend | redrive |
get | post | delete | worker | version`. `worker` is a built-in test worker
(kinds `echo`, `sleep`, `fail`, `flaky`, `hang`, `crash`, `probe`; `probe` does
a read-only GET to loopback `http://` URLs only).

## Auth (Gaia S4.1, slot 205)

- `serve --token-file F`: F holds one line, >= 32 visible ASCII chars, is a
  regular file (no symlink), owned by the service user, mode 0600 or 0400.
  Anything else and the server refuses to start (the error never contains
  the secret).
- Loopback peers (127.0.0.0/8, ::1, ::ffff:127.x) need no token: every
  existing local client and the live VPS unit behave exactly as before.
  `--token-on-loopback` makes loopback need the token too.
- Every non-loopback peer needs `Authorization: Bearer <secret>` on every
  path, `/healthz` included; otherwise 401 with
  `WWW-Authenticate: Bearer realm="poseidon"` and
  `{"error":"unauthorized","detail":"missing"|"invalid"}`. The request is
  not routed, so a refused POST changes nothing. Duplicate `Authorization`
  headers are 400. An unknown peer address counts as non-loopback.
- A non-loopback bind needs `--allow-remote` and `--token-file`; without a
  secret it refuses to start (no unauthenticated LAN bind is possible).
- Constant-time compare; the secret is wiped on drop, its `Debug` is
  redacted, it is never logged or returned. `GET /stats` has
  `auth: {mode, token_on_loopback, secret_configured, verdicts:{loopback,
  token, missing, invalid, no_secret}}` (counters only).
- Clients (`submit`, `lease`, `get`, `worker`, ...) send the token from
  `--token-file` or `$POSEIDON_TOKEN_FILE` (same file checks). A worker
  stops with an error on 401 instead of retrying.
- Why a shared secret and not hades-gate Ed25519: hades-gate tokens are
  single-use, expiring approvals scoped to one action/target/plan hash with
  a replay ledger; a queue worker makes many calls a second, so reusing them
  per request would need a new signed-request scheme (nonce, clock window,
  ledger). Owner decision D-205-1.

## Limits and known gaps

- Refusal counters (`quota_refusals`, `budget_refusals`,
  `backpressure_refusals`, `stale_acks`, `charge_clamped`) change no queue
  state, so no log record carries them. Since swarm/poseidon-refusal-persist
  (2026-10-21) they live in `refusals.dat` next to the log: two 512-byte
  slots, each one `crc32 json` line with a sequence number, written
  alternately with one positioned write **before** the response is sent;
  opening takes the valid slot with the highest sequence. Guarantees and
  limits:
  - `kill -9` / any process crash: nothing lost. Every refusal a client saw
    answered is counted after the restart (real test: 100 SIGKILL rounds,
    25 of them mid-burst with 4 clients at tens of thousands of refusals
    per second, plus 20 kills right after single 429/402 answers; 0 lost).
  - Power loss / kernel crash: the sidecar is `sync_data`'d by the ticker
    at most ~1 s after a change, so up to ~1 s of refusal counts can be
    lost. Job state is unaffected (the log is synced before every answer).
  - A torn sidecar write damages only the slot being written; the other
    slot (one step older) is used and the next change repairs it. Both
    slots damaged: the server still starts, falls back to the counters of
    the last compaction (`Counters` record), logs it and shows
    `refusals_sidecar.bad_slots` in `GET /stats`. A damaged sidecar never
    blocks start-up, because these are observability numbers, not queue
    state. Restore takes the larger of log and sidecar per field (the
    counters only grow).
  - Deleting `refusals.dat` falls back to the last compaction's values.
  - An older binary ignores `refusals.dat`, so rolling back is safe (it
    just goes back to the old reset-on-crash behaviour).
  - Not persisted: per-agent `budget_blocked` (lease-time skip count per
    agent in `GET /stats` `agents`); it still resets on a crash.
  - Existing behaviour, unchanged: one `POST /lease` with `wait_ms > 0`
    that stays budget-stopped bumps `budget_refusals` once per wake-up
    (up to ~30 per call), so that counter counts lease evaluations, not
    calls.
  Job state, budgets, caps, quotas and devices are always durable.
- Auth is one shared bearer secret (no per-worker identity, no rotation
  without restart, plain HTTP so the token is visible on the wire: LAN only
  behind a trusted segment or a tunnel). See Auth.
- One mutex for the whole engine and an fsync per write: fine for agent-scale
  work queues (hundreds of jobs per second), not a message bus.
- Gaia integration: gaia-core's POSEIDON `SubordinateFunction::handle`
  forwards to this service through `gaia_poseidon::queue::QueueAdapter`
  (read-only for budget, never leases or acks; loopback only). The coupling
  is typed, not vocabulary-only: this crate publishes its Gaia wire contract
  as constants (`gaia::{FUNCTION, SYSTEMS, VERBS, DISPATCH_VERBS, OUTCOMES}`)
  and `gaia_poseidon::contract` pins them to the gaia-core enums in tests,
  checks the `/healthz` identity before any other call and parses every
  answer strictly (slot 218, 2026-10-21); see `docs/gaia-poseidon.md`.
- The VPS ownership registry (`docs/gaia-ownership.md`, VPS `gaia-registry`)
  has a different nine (no POSEIDON; compute adapters under HEPHAESTUS). The
  ws/gaia design has POSEIDON. Row for this crate in the ws/gaia scheme:
  `openmind-queue -> POSEIDON` (`gaia::REGISTRY_ROW`). Reconciling the two
  nine-function lists is an owner/GAIA decision.
