<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# gaia-core POSEIDON -> `openmind-queue` (the real adapter)

Written 2026-10-20T20:30:00+02:00 (Europe/Zagreb). Branch `swarm/gaia-poseidon`
= `swarm/gaia-live` (4fbbb7f: ws/gaia gaia-core + the nine function crates)
merged with `swarm/poseidon` (7d0c478: `openmind-queue`), plus this work.

## What changed

| Where | What |
| --- | --- |
| `crates/gaia-core/src/system.rs` | `FunctionId::COMPUTE`, **the single config point** for who owns the four compute systems. The ownership-table rows for `vram-pool`, `quotas`, `queues` and `budget-caps` read it. Also `System::COMPUTE` and `System::is_compute()`. Component strings now name `crates/openmind-queue`. |
| `crates/gaia-poseidon/src/queue.rs` | The adapter `QueueAdapter`: forwards GAIA directives to a running `openmind-queue` over loopback HTTP (the queue crate's own std-only client). |
| `crates/gaia-poseidon/src/lib.rs` | `Poseidon::new()` is still the in-memory stand-in (scenarios, unit tests stay deterministic). `Poseidon::with_queue(cfg)` uses the adapter for `handle`, `health`, `tick`, `state_json`. `systems()` returns `System::COMPUTE`. |
| `crates/gaia/src/poseidon.rs`, `main.rs`, `lib.rs` | `gaia poseidon ...` subcommands, `gaia dispatch ... --queue ADDR`, `gaia::build_with(compute)`. |

## Directive mapping

| System | `status` | `plan` | `run` / `execute` |
| --- | --- | --- | --- |
| `queues` | empty payload: `POST /gaia/dispatch` (depth, counters); `job:<id>` or `<id>`: `GET /jobs/{id}`; `dlq`: `GET /dlq` | queue's Gaia plan (dry-run) | **submit**: payload is a JSON object of job fields -> `POST /jobs` |
| `quotas` | Gaia status + per-agent `pending`, `inflight`, `max_pending`, `max_inflight` from `GET /stats` | dry-run | **refused** (read-only) |
| `budget-caps` | Gaia status (global spent/reserved, `cap:<agent>`, `spent:<agent>`) + `reserved:`, `remaining:`, `blocked:` per agent and `global_cap` from `GET /stats` | dry-run | **refused** (read-only) |
| `vram-pool` | Gaia status (approved/used/free/planned/goal/gap GB) + device list from `GET /vram` | `GB` payload: placement dry-run (which device, or "fits no approved device") | **refused** (read-only) |

Queue answers to a submission map to Gaia outcomes: `201` -> `done` (metric
`job_id`); `402 budget_hard_stop`, `429 quota_exceeded`,
`403 real_money_not_approved`, `503 backpressure` / `vram_unavailable` ->
`refused` with the queue's own detail in the summary and `http_status`
metric; `400` -> `rejected`. Money words in any payload are refused by the
adapter before any call (both the gaia-poseidon list and the queue's list).

## Read-only for budget, never spending

One allow-list (`gaia_poseidon::queue::allowed`) is checked before every
call: `GET /healthz|/stats|/vram|/dlq|/gaia/status|/jobs/{id}`,
`POST /gaia/dispatch` (only ever with verb `status` or `plan`; the queue's
Gaia dispatch is read-only by construction) and `POST /jobs`. Never sent:
`/caps`, `/quotas`, `/lease`, `/jobs/{id}/ack|nack|extend|redrive`,
`/vram/devices`, `/vram/sync-providers`, `/admin/compact`. Since spend is
only charged at ack and Gaia never leases or acks, Gaia cannot spend; caps,
quotas and devices stay operator actions on `openmind-queue` itself. Unit
tests drive every system x verb x a set of payloads through a recording fake
transport and assert every call is on the list; the real-life test compares
global/agent spend, reservations, acks and leases before and after a batch of
gaia commands (identical).

Submitting a job does carry a `cost` (a number the queue reserves at lease and
charges at ack, by the operator's worker). That is the queue's accounting of
agent work, not money: nothing anywhere is paid.

Other safety choices: the queue address must be loopback (the queue API has
no auth); if the queue is unreachable the directive is **refused** (exit 3
from the CLI), POSEIDON's integrity drops to 30 so GAIA marks it `degraded`,
and there is **no fallback to the stand-in** (a fake answer would hide an
outage).

## CLI

```
export GAIA_POSEIDON_QUEUE=127.0.0.1:19240        # or --queue ADDR
gaia poseidon status                              # health + four systems
gaia poseidon submit --agent a --kind echo --cost 40 [--queue-name Q] [--priority P]
                     [--max-attempts N] [--lease-ms N] [--vram-gb G] [--cpu-fallback] [--job-payload T]
gaia poseidon job ID | dlq | vram | plan-vram GB | quotas | caps
gaia dispatch queues run --payload '{"agent":"a","cost":5}' --queue 127.0.0.1:19240
```

Exit codes: 0 done / dry-run, 2 usage, 3 queue unreachable, 4 refused,
5 rejected. Without `--queue` / `GAIA_POSEIDON_QUEUE`, every other `gaia`
command (status, tick, simulate-faro, dispatch) keeps the stand-in.

## POSEIDON or HEPHAESTUS: not decided here

The two registries disagree:

* ws/gaia `gaia/docs/gaia-design.md` section 4.2 (and gaia-core as merged):
  compute resources and budgets belong to **POSEIDON** (`vram-pool`,
  `quotas`, `queues`, `budget-caps`).
* The VPS ownership registry `docs/gaia-ownership.md` has no POSEIDON; it
  lists compute adapters (`openmind-provider`, `openmind-route`,
  `openmind-fallback`, `openmind-exo`, `openmind-llama`, `openmind-sandbox`,
  `cgroup-detect`) under **HEPHAESTUS**.

This branch does not choose. The owner of the four compute systems is one
constant, `gaia_core::FunctionId::COMPUTE` (currently `FunctionId::Poseidon`
so the ws/gaia design keeps working). Everything else follows it: the
ownership table, `gaia systems`, `gaia poseidon` output (`owner` field),
GAIA routing and containment.

What a flip to HEPHAESTUS needs (an owner/GAIA decision; tried on this branch
and reverted):

1. Set `pub const COMPUTE: FunctionId = FunctionId::Hephaestus;`.
2. `gaia-hephaestus` must declare `System::COMPUTE` in addition to its own
   systems and delegate those directives to `gaia_poseidon::QueueAdapter`
   (or a renamed copy), and the POSEIDON slot must either declare nothing or
   not be registered.

Without step 2, GAIA refuses to boot and says why (observed 2026-10-20
20:23 +02:00 with the constant flipped):
`HEPHAESTUS declares ["terminal", ...] but owns ["terminal", ..., "vram-pool", "quotas", "queues", "budget-caps"]`.
That loud failure is deliberate: compute is never silently split or orphaned.

## Tests

* `gaia-core`: `compute_rows_follow_the_single_config_point`.
* `gaia-poseidon` (9 adapter tests): allow-list, every directive stays on it,
  budget/quota/VRAM writes refused without a call, money refused before any
  call, queue refusals mapped (402/429/503/400), unknown job fields and
  non-objects rejected, job inspect and status translation, unreachable ->
  refused + degraded, loopback-only config.
* `gaia`: `job_spec_from_flags`, `unreachable_queue_degrades_poseidon_and_refuses`.
* Real life: `scripts/real/gaia-poseidon.sh` (60 checks) and
  `GALION_REAL=1 cargo test -p gaia --test real_gaia_poseidon`; evidence in
  `docs/real-tests/gaia-poseidon.md`.

## Open

* Per-agent caps are listed only for agents that have a cap (that is how the
  queue's Gaia status reports them); `remaining:` comes from `/stats`.
* No authentication on the queue: Gaia talks to it on loopback only.
* The adapter runs a blocking HTTP call per directive and one `GET
  /gaia/status` per tick (2 s timeout). Fine for the CLI; a long-running GAIA
  loop with a down queue would wait up to 2 s per tick.
