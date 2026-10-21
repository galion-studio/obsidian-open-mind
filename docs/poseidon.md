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
| `http`, `server` | std-only HTTP/1.1 (Content-Length bodies, 1 MiB cap, connection cap 256), loopback bind by default (non-loopback needs `--allow-remote`), a ticker thread that sleeps on a condvar until the next deadline (max 1 s, no busy loop). |

## HTTP API

| Method and path | Body / result |
| --- | --- |
| `GET /healthz` | `{ok, function, service, version, time}` |
| `GET /stats` | depth, counters, budgets per agent, VRAM summary, store info, `refusals_sidecar` (seq, restored, bad_slots, writes, syncs), load |
| `POST /jobs` | `{queue, agent, kind, priority, max_attempts, cost, lease_ms, payload, vram_gb, cpu_fallback}` (all optional) -> 201 `{id}`; 400 bad (incl. `cost` over `MAX_COST` = 2^40), 402 budget, 403 money, 429 quota, 503 backpressure or vram_unavailable |
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
- Crash and soak behaviour (slot 206, `tests/real_crash_soak.rs`, real
  release binary, see `docs/real-tests/poseidon-crash-soak.md`):
  - `kill -9` at random moments under full load (producers, workers acking,
    deliberate stale acks, quota / budget / backpressure refusals) and with
    compaction running under that load: 0 lost jobs, 0 resurrected acks,
    exact ack and spend totals, caps and quotas intact, refusal counters
    never go backwards, lease tokens stay fenced, and the restarted server
    equals an independent replay of `queue.log` + `refusals.dat`.
  - Torn tail: any cut, garbage, half line or 4 KiB zero block at the end
    is cut back to the last complete line and the log keeps working. A
    complete last line with a bad checksum is treated the same way (cut,
    not refused); a `kill -9` cannot produce one, a disk bit flip can.
  - Two servers on one data dir: the second exits 2 with "in use by another
    process", binds nothing and does not touch the log; a SIGKILL frees the
    lock at once.
  - A failed compaction no longer turns a change that was already synced
    into a 500 (a retrying client would have enqueued or charged twice);
    it is logged and counted as `store.compaction_errors` in `GET /stats`.
    After the rename the snapshot's own handle becomes the log handle, so
    no later step can leave appends going to the old, unlinked file.
  - The HTTP client treats a body shorter than `Content-Length` as an
    error: a server killed between head and body used to leave "201" with
    no id, which a client took as an answer. The server now writes head
    and body in one write.
  - Memory: live heap is flat across 10k-job rounds (counting allocator,
    `tests/heap_rounds.rs`). The static musl build (the deploy target)
    holds 7.0 MB RSS with 10,000 live jobs and 5.4 MB drained, flat over
    10 rounds (100,000 jobs). A glibc build grows ~5 MB per round
    (thread-per-connection plus glibc per-thread arenas, memory the
    allocator keeps); `MALLOC_ARENA_MAX=2` makes it plateau at ~22 MB.
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

## Fuzzing (slot 221, gaia-queue-fuzz)

Std-only mutation/property targets in `crates/openmind-queue/tests/`
(`fuzz/mod.rs` harness, `fuzz_queue.rs` targets, `fuzz_finds.rs`
regressions, `real_queue_fuzz.rs` real binary). `cargo test` runs a short
fixed-seed pass; `GALION_FUZZ_SECS=N` runs each target N seconds with a
printed seed, `GALION_FUZZ_SEED` reproduces a find. Targets: `queue.log`
line decode, whole-file `Store::open` against an independent oracle,
hostile CRC-valid replay through `Engine::apply`, `POST /jobs` bodies
through the decide/apply loop (hard stop, depth, compaction fixed point),
`refusals.dat` open/save, and the HTTP request parser. The auth header
parser (slot 205) is not covered: `swarm/poseidon-auth` does not merge
cleanly onto this base (conflict in this file), so the row's "else skip"
applied.

Finds fixed (each with a regression test that fails on b71a909):

1. Duplicate job id in the log (two `enqueue` or `snapshot` records for
   one id) left a stale ready-index entry. After the ack, `POST /lease`
   panicked on `jobs[&id]`; the release build (`panic = "abort"`) died
   with SIGABRT, and `/stats` showed `dead` = 18446744073709551615.
   Replay now ignores a second record for a live id (first one wins), and
   `depth()` saturates.
2. `id + 1`, `token + 1` and every counter `+= 1` in `apply` overflowed
   on edge values (debug panic, release wrap). Now saturating; a
   saturated `next_id` that is still in use makes enqueue answer 503
   instead of reusing a live id.
3. `cost` near u64::MAX (from any `POST /jobs`) overflowed the reservation
   sums; saturating sums then released to a wrong `reserved` and
   compaction rebuilt a different one. `cost` is now limited to
   `MAX_COST` = 2^40 per attempt (400 above it); the sums stay exact for
   up to 2^24 leased jobs.
4. `Budget::fits` saturated: with `cap = u64::MAX`, `spent 1 + cost
   u64::MAX` compared equal to the cap and passed. Now a checked sum; an
   overflowing sum never fits. Same at the enqueue door.
5. `GET /stats` VRAM summary summed `cents_per_hour` (any u64 a client
   sends to `POST /vram/devices`) with `+=`: debug panic, release wrapped
   to a wrong price. Now saturating.
6. A CRC-valid `refusals.dat` slot at seq u64::MAX overflowed the next
   save (debug) or wrapped it to seq 0 (release), so the stale slot won
   every later open and refusal counts stopped persisting. A restored
   seq above 2^62 is rebased on open (same value written as seq 2, then
   seq 1, so a torn write still opens to it).
7. HTTP: differing duplicate `Content-Length` was "last one wins" and
   `+5` parsed as 5. Both are now 400 (RFC 9112 6.3); equal duplicates
   are still accepted.
