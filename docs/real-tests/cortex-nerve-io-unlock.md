<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# cortex-nerve-io-unlock (slot 247, D-236-2) – real-test record

Branch `swarm/cortex-nerve-io-unlock`, base `swarm/cortex-api-load` 5e0adba.
Box only, loopback only, slot ports 21170-21179. Not deployed. All times
Europe/Zagreb (CEST, +02:00).

## What changed

Before: the decider held the engine lock for a whole batch, including every
nerve HTTP call: `POST /actions` (3 s timeout), then the alert `POST /event`
(3 s) and the escalation `POST /event` (3 s). A hung nerve cost up to 9 s
per subject, the next batch waited behind it, and `POST /escalations/ack`
gave up with 503 after 2 s.

Now (`crates/gaia-cortex/src/engine.rs`):

1. **plan** (lock held, no nerve I/O): admit, correlate, decide, build the
   action requests. A restart is **reserved** against the unit's hourly cap
   here, so the cap can never be passed while a call is in flight.
2. **call** (no lock): `call_actions` sends the requests from the decider
   thread. **Per-batch breaker:** once one request's reply is lost (by
   `unknown`), the rest of the batch is not sent but answered locally:
   dry-run gives `simulated` by `local`, live gives `refused` by `local`,
   which is escalated. So a hung nerve costs a batch at most one 3 s
   timeout. The next batch tries the nerve again.
3. **commit** (lock held): count the outcomes, record the decisions, and
   free the cap reservation when the nerve said the restart did not run
   (refused, rate-limited, circuit-open, failed with a known reply). An
   `unknown` reply keeps the reservation, as before.
4. **posts** (no lock): alert and escalation events go to a **courier**
   thread with its own nerve link and a bounded queue (1024 posts;
   `try_send`, so a full queue drops and counts). Decisions never wait for
   posts. Alerts are still written to alerts.jsonl and the outbox first.
   `/stats` gains `nerve_posts {alerts_ok, escalations_ok, failed, dropped,
   queued}`. `alerts_to_nerve` is now `nerve_posts.alerts_ok`.

`busy` is now a flag the decider sets around each batch. Reads while the
decider waits on the nerve are **fresh** (they no longer come from the
snapshot), and carry `busy: true` and `x-cortex-busy: 1`. The snapshot path
from slot 236 stays as the fallback if the lock is ever held.
`Cortex::process` (the replay CLI and the unit tests) runs the same phases
on one thread, so its results are unchanged (pipeline 22/22 untouched).

## Unit and mock tests (box, 02:06 CEST)

`cargo test -p gaia-cortex`: **45 passed, 0 failed**, both with and without
`--features llm-advisor`. New `tests/io_unlock.rs` (6 tests):

- breaker, dry-run: 2 subjects and a hung mock gives 1 request sent, the
  second `local` "not sent", in < 600 ms (one 300 ms timeout).
- breaker, live: `failed`/unknown plus `refused`/local, both escalated.
  The unknown one keeps its cap slot and the unsent one frees it.
- the nerve's rate-limited, refused or circuit-open answers free the
  reserved slot, and simulated keeps it.
- engine lock free during a 1.5 s nerve call: 20 `try_lock`s, worst < 50 ms,
  `busy` true, then false.
- slow posts (1.5 s each) do not delay decisions: 3 batches decided
  < 1.2 s, then the courier delivers all alerts (dropped 0, queued 0).
- a link without `fork` posts inline, still outside the lock.

`cargo clippy -p gaia-cortex --all-targets -D warnings` gave 0 warnings
(default and `llm-advisor`). `cargo fmt --all --check` passed,
`scripts/no-python.sh` passed, `scripts/spdx-scan.sh` found 0 missing out
of 422.

## Real test: release binary + hung stub nerve (`tests/real_nerve_io_unlock.rs`)

Run with `GALION_REAL=1 cargo test --release -p gaia-cortex --test
real_nerve_io_unlock -- --nocapture`. The stub accepts every connection,
logs the request line and never answers. Before is 5e0adba (run with
`GALION_IO_BASELINE=1 GALION_CORTEX_BIN=<5e0adba>`) at 02:02:51-02:03:48;
after is this branch's binary at 02:05:07-02:05:22.

| phase | before 5e0adba | after (slot 247) |
|---|---|---|
| H1: 1 failure, push to decision | **9540 ms** | **3319 ms** |
| H1: `/stats` during the hang | 200, 0 ms, snapshot (stale) | 200, 0 ms, **fresh**, busy |
| H1: `POST /escalations/ack` during the hang | **503 after 2007 ms** | **404 in 0 ms** (unknown id, answered) |
| H2: 2 failures in one batch, both decided | **18708 ms** | **3323 ms** |
| H2: `POST /actions` reaching the nerve | 2 | 1 (second "not sent", `local`) |
| H3: 3 batches 300 ms apart, last decision | **28147 ms** | **6672 ms** (batches 2+3 coalesced) |
| stub saw `POST /actions` / `POST /event` | 6 / 12 | 4 / 4 (courier still draining; dropped 0) |
| `actions_unknown` | 6 | 4 (2 "not sent" by the breaker) |

The after run asserts: H1 < 4.5 s, lost reply `unknown` plus escalated, `/stats`
< 100 ms fresh and busy, ack 404 < 100 ms. H2 has exactly one `POST /actions`,
by {local, unknown}, < 4.5 s. H3 is < 3 × 3.2 s + 1.5 s, with 2-3 `POST
/actions`. `nerve_posts.dropped` is 0.

## Regression runs (release, box)

- `real_api_load` (slot 236, ports 21060-21061, 02:04:51-02:05:07) passed:
  every phase had all `/health` and `/stats` replies at 200 with p99 < 7 ms
  (P2 hung: /health p99 6.03 ms, /stats 4.49 ms; P3 hung + flood: 5.07 /
  6.03 ms). 113 and 118 busy replies, invalid 0 to 2, actions_unknown 2.
- `real_t51_chaos` with `GALION_T51_PORT_BASE=21170`, the real event-mode
  nerve bb287cc (release, built on the box) and this branch's binary
  (02:05:28-02:06:36): **2 passed, 0 failed**, 67.7 s. S7 hung nerve: decision
  `unknown` after **3589 ms** (it was 9 s+ in the t51/236 runs), /health and
  /stats both 0 ms during the hang, and after SIGCONT the nerve logged 1 late
  dry-run action. S6: 1 s latency was answered by the nerve, 4 s latency
  gave `unknown`. S8 re-subscribed after 919 ms. S9: 0 LLM calls, 0
  non-loopback sockets. Teardown freed ports 21170-21173.

## VPS (read-only, 02:07:47-02:07:57)

`gaia-cortex` and `galion-nerve` are active. The live cortex is
`/opt/galion/rust/bin/gaia-cortex-bd6fbe6` (cortex-hermes-boss line: mode
live, llm_enabled, hermes_handoff on, 3 handoffs, 3 escalations open, 0
incidents, busy false, 0 actions requested). Cortex `/health` and `/stats`
answered 200 in about 0.6 ms. Nerve 8670 `/healthz` 200, `/health` is
system **degraded**, score 61, cycle 20, and `/state` is 200 (40.6 kB,
2.2 ms). Nothing was changed on the VPS. The live binary does not have
this change: it is on the hermes-boss line, so the change has to be merged
there before any deploy.

## Not done / owner decisions

- **D-247-1** deploy: not done (no deploy in this slot). The change sits on
  the cortex-api-load line. To go live it must be merged with the live line
  (bd6fbe6 / hermes-boss; not checked here whether its Hermes hand-off does
  I/O inside the decider, so check it in that merge) and go through the usual cutover.
- **D-247-2** breaker scope: per batch (chosen, so the next batch retries).
  A time-based "nerve suspect for N s" would cut the 3 s per batch further,
  but it would also skip requests to a nerve that has already recovered.
  Keep per batch?
- **D-247-3** an action the breaker did not send in **live** mode is
  `refused` by `local` and escalated (it will not run). The next failure of
  that unit retries it. Is that acceptable, or should it be queued and
  retried automatically after the nerve answers again?
- **D-247-4** the LLM advisor call (Hermes/River) still runs inside the plan
  phase, under the lock. It has its own timeout, and it is off unless the
  `llm-advisor` feature is built (it is on live). Move it out the same way
  (plan, advise, re-plan)?
- **D-247-5** courier queue cap 1024 with drop-newest. During a long hang
  the queue fills with 3 s timeouts one by one (7 were queued after the
  real run). Is drop-newest right, or should it be drop-oldest, or should
  posts be skipped while an action reply is lost?
