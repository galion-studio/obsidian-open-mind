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
| `vram` | Device registry (`gpu`, `lan`, `remote`; `planned`/`approved`/`rejected`; healthy), best-fit placement GPU -> LAN -> remote, CPU offload when the job allows it, capacity summary against the 1500 GB goal (approved, planned, used, gap, cents/hour as numbers). A priced device cannot be approved by hand: it needs an approved `openmind-provider` row (the owner). |
| `gaia` | Gaia vocabulary without a gaia-core dependency (gaia-core is not on main): `FUNCTION = "POSEIDON"`, the four systems, verbs `status`/`run`/`plan`/`execute`, outcomes `done`/`dry-run`/`refused`/`rejected`, report = summary + integer metrics, load 0..100 for `Health::ok`. Dispatch is read-only. |
| `http`, `server` | std-only HTTP/1.1 (Content-Length bodies, 1 MiB cap, connection cap 256), loopback bind by default (non-loopback needs `--allow-remote`), a ticker thread that sleeps on a condvar until the next deadline (max 1 s, no busy loop). |

## HTTP API

| Method and path | Body / result |
| --- | --- |
| `GET /healthz` | `{ok, function, service, version, time}` |
| `GET /stats` | depth, counters, budgets per agent, VRAM summary, store info, load |
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

## Limits and known gaps

- Process-local refusal counters (`quota_refusals`, `budget_refusals`,
  `stale_acks`) are only persisted at compaction; a `kill -9` resets them.
  Job state, budgets, caps, quotas and devices are always durable.
- No authentication: loopback only by default. Putting it on a LAN needs a
  token layer first.
- One mutex for the whole engine and an fsync per write: fine for agent-scale
  work queues (hundreds of jobs per second), not a message bus.
- Gaia integration: on `swarm/gaia-poseidon` the gaia-core POSEIDON function
  forwards to this service through `gaia_poseidon::queue::QueueAdapter`
  (read-only for budget, never leases or acks); see `docs/gaia-poseidon.md`.
- The VPS ownership registry (`docs/gaia-ownership.md`, VPS `gaia-registry`)
  has a different nine (no POSEIDON; compute adapters under HEPHAESTUS). The
  ws/gaia design has POSEIDON. Row for this crate in the ws/gaia scheme:
  `openmind-queue -> POSEIDON` (`gaia::REGISTRY_ROW`). Reconciling the two
  nine-function lists is an owner/GAIA decision.
