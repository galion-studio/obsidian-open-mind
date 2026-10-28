<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 258 cortex-live-merge-3: real-test record and deploy checklist

Branch `swarm/cortex-live-merge-3` @ e94498c85a9e78753713e777adbd17e350603dc7, all times Europe/Zagreb (CET +01:00), 2026-10-27.

## What is merged

| Step | Commit | Content |
|---|---|---|
| base | `1305dec` | cortex-live-merge-2 = live bd6fbe6 + api-load + flap-dedupe + incident-backfill |
| 1 | `cd956db` | cortex-nerve-io-unlock (slot 247, D-236-2): plan/call/commit — nerve I/O out of the engine lock; courier posts; busy = snap flag |
| 2 | `c68d8cb` | cortex-sandbox-oom-info (R4b-sandbox-oom): sbxtest* oom.kill -> info only, no Hermes escalate (Mate approved hoff-1792623978-2). Clean merge. |

Conflict resolution (io-unlock onto merge-2; only `engine.rs` conflicted, six hunks):

- Struct: kept `repeats` (flap-dedupe) **and** `posts`/`outgoing` (io-unlock).
- Kept `track_nerve_incident` + `backfill` + `note_resolved` from merge-2; took `plan`/`plan_one`/`call_actions`/`commit`/`run_decider`/`Courier` from io-unlock.
- `escalate`: kept hermes handoff (`hand_to_hermes`) and switched the nerve post to the courier queue (not under the engine lock).
- `stats_json`: caps + `repeat_watch` + `nerve_posts` / `alerts_to_nerve` = `posts.alerts_ok()`.
- `health_json`: `hermes_handoff` + `handoffs` + `busy: snap.busy()`.

## Gates (box, 17:38-17:44 CET)

- `cargo fmt -p gaia-cortex -- --check` ok.
- `cargo clippy -p gaia-cortex --all-targets -D warnings` ok (default).
- `cargo clippy -p gaia-cortex --all-targets --features llm-advisor -D warnings` ok.
- `cargo test -p gaia-cortex --all-targets` (default): lib 29 + binary_e2e 1 + flap_dedupe 7 + incident_lifecycle 11 + io_unlock 6 + pipeline 24 = 78/0; real suites skip without GALION_REAL.
- Same with `--features llm-advisor`: 78/0.
- `scripts/no-python.sh` ok.

## Real tests (release, box loopback, ports 21270-21279)

Nerve binary: `/workspace/swarm/artifacts/openmind-nerve-bb287cc`. Cortex: this branch release.

| Test | Time (CET) | Result |
|---|---|---|
| `real_nerve_io_unlock` | 17:39:46-17:40:01 | 1/1 ok. H1 decision 3457 ms (busy /stats 0 ms, ack 404@0 ms); H2 3317 ms (1 POST /actions); H3 3 batches last 6664 ms; nerve_posts queued 7 / failed 3 (stub closed) |
| `real_api_load` | 17:40:15-17:40:32 | 1/1 ok. p99 health/stats: quiet 0.50/0.45, flood 0.86/0.78, hung 0.85/0.82 (busy 117), hung+flood 1.00/0.79 ms; 0 failed |
| `real_t51_chaos` (`GALION_T51_PORT_BASE=21270`) | 17:40:46-17:41:55 | 2/2 ok. S7 hung decision 3643 ms, /health+/stats 0 ms during; S8 resubscribe 906 ms; S9 llm 0; nerve_posts alerts_ok 23 escalations_ok 10 |
| `real_t237_backfill` | 17:42:10-17:42:36 | 2/2 ok (live shadow skipped, no GALION_T237_LIVE_PORT). P1 backfill 3/3 in 515 ms; P2 cleared 513 ms; P3 resolve +1033 ms; P4 remapped; nerve_posts alerts_ok 11 |
| `real_flap_dedupe` | 17:43:08-17:43:46 | 1/1 ok. AFTER R2 after 4 folded writes / 2540 ms; caps dry-run 2/unit then R3-cap; total 6/h then R3-cap-total; live same |

Logs: `/workspace/swarm/logs/cortex-live-merge-3-*.log`.

## Live VPS, read-only (17:44:18 CET)

- Live binary still `/opt/galion/rust/bin/gaia-cortex-bd6fbe6` (active, NRestarts 0, started 2026-10-22 23:46:47 +02:00). **Not this branch.**
- `/health`: live, llm_enabled true, hermes_handoff true, busy false, incidents_open 2, escalations_open 18, handoffs 17, events_in 947.
- Nerve still holds long-lived open incidents (edge/cloudflared among them). Backfill gap of merge-2 / this branch still not deployed.
- This slot does **not** deploy.

## Deploy checklist (owner/parent only; slot 258 does NOT deploy)

1. Build artifact: `cargo build --release -p gaia-cortex --features llm-advisor --target x86_64-unknown-linux-musl` (or zigbuild). Install as `/opt/galion/rust/bin/gaia-cortex-<sha>`, keep `gaia-cortex-bd6fbe6` for rollback.
2. Drop-in: new `ExecStart=` for the new binary; keep `CORTEX_ACTIONS=live`, `CORTEX_LLM_ENABLED=1`, `CORTEX_HERMES_HANDOFF=1`, `CORTEX_HERMES_PER_HOUR=6`.
3. **D-245-1 caps**: live config pins `restarts_per_unit_hour: 3`; merged default is 2. Decide before cutover.
4. **D-245-2 backfill burst**: first connect backfills every open nerve incident (~R11 alerts; critical may hand off to Hermes, capped 6/h).
5. **D-247 / io-unlock**: nerve I/O no longer holds the engine lock. Expect `/stats.nerve_posts` and `busy: true` only during a batch. Courier posts alerts/escalations off the decider thread.
6. **R4b-sandbox-oom** (c68d8cb): sbxtest* oom.kill -> info, no Hermes. Safe for live (Mate approved hoff-1792623978-2).
7. After restart check: `/health` hermes_handoff true + busy false when idle; `/stats` `nerve_incidents_backfilled` ~= nerve open count, `nerve_posts.failed` near 0, `caps` as decided, `actions_executed` sane; nerve `/state` cortex stream connected; journal shows plan/call/commit path (no lock across POST /actions).
8. Rollback: point `ExecStart=` back at `gaia-cortex-bd6fbe6`, `daemon-reload`, restart.

## Owner decisions still open

- D-245-1 caps 3 vs 2
- D-245-2 first-connect backfill burst
- D-247-1..5 (from io-unlock: courier cap, breaker policy, deploy timing)
- Whether to deploy this full merge-3 (ships api-load + flap-dedupe + backfill + io-unlock + R4b) vs a thinner cherry-pick onto live bd6fbe6 (slot 253 owns the thin R4b port)
