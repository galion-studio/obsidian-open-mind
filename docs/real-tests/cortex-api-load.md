<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: cortex-api-load (slot 236, T5.1 follow-ups F4/F5)

Base `swarm/cortex-t51-chaos` e982210. Box only, loopback only, ports
21060-21069. No deploy, VPS read-only. All times Europe/Zagreb (+02:00).

## Problem (from slot 227's chaos run)

- **F5**: the decider holds the engine lock for a whole batch, including
  nerve HTTP calls (POST /actions to a hung nerve: 3 x 3 s). `/health` had
  already been made non-blocking; `/stats` (and `/incidents`, `/decisions`,
  `/alerts`, `/escalations`, and the invalid-item count of `POST /v1/events`)
  still waited: 8.8 s.
- **F4**: 32 idle connections (= `max_connections`) filled the API for the
  5 s head deadline: every request got 503 for ~5.1 s, and a flood that
  reconnects keeps it at 503 for as long as it lasts.

## Fix

1. `crates/gaia-cortex/src/snapshot.rs` (new): after every batch (and on
   start, nerve restart and escalation ack) the engine publishes its read
   views into a shared `Arc<Snapshot>` that lives outside the engine lock.
   Every GET route tries the lock; if the decider holds it, the route
   answers from the snapshot with `busy: true` + `snapshot_age_ms` (objects)
   and the headers `x-cortex-busy: 1` / `x-cortex-snapshot-age-ms` (all
   routes, also the `/decisions` and `/alerts` arrays). Fresh `/stats` now
   carries `busy: false`.
2. `POST /v1/events` never takes the engine lock: invalid items go to an
   atomic counter that the next batch folds into `events_invalid` (and
   `/stats` shows them right away). `POST /escalations/ack` (the one write
   that needs the engine) waits at most 2 s, then 503 + `retry-after: 1`.
3. `galion-common::http::Limits::evict_idle` (new, default off, so every
   other service is unchanged): at the connection cap the server closes the
   oldest connection that has not yet delivered a complete request instead
   of refusing the new one; connections already in the handler are never
   evicted. Live connections stay under 2 x `max_connections` while evicted
   threads wind down; with nothing to evict a new connection waits up to
   `EVICT_GRACE` (100 ms) for a slot before the 503. The cortex API turns it
   on (`server::api_limits()`); cap and deadlines unchanged (32, 5 s).
4. `tests/real_t51_chaos.rs`: ports movable with `GALION_T51_PORT_BASE`
   (default 20970, slot 227's), S7 now also asserts `/stats` < 1 s.

## Real test: `crates/gaia-cortex/tests/real_api_load.rs`

`GALION_REAL=1`; `GALION_CORTEX_BIN` picks the binary;
`GALION_LOAD_BASELINE=1` records without asserting (for the old binary).
The real release `gaia-cortex` runs against a stub nerve on 21060 that
accepts and never answers (the decider hangs in POST /actions, 3 x 3 s);
cortex on 21061, dry-run, LLM off. Every sample is one new connection.

| phase | what |
|---|---|
| P0 quiet | 100 x (/health, /stats) |
| P1 flood | 32 idle connections that reconnect as soon as they are closed; 100 x (/health, /stats) |
| P2 hung decider | push a `unit.failed` for galion-desk.service, wait 0.9 s, read for 6 s; meanwhile push 2 invalid lines |
| P3 hung + flood | push galion-health.service, start the flood, read for 6 s |

Asserted (new binary): every read 200, p99 < 100 ms in every phase, P2 hits
the snapshot path, invalid push 400 in < 100 ms, both invalid lines counted
(`events_invalid` 0 -> 2), both decisions land with `by = unknown`.

### Before: e982210 release (00:35:06, `before-e982210.json`)

| phase | /health ok | /health p50 / p99 | /stats ok | /stats p50 / p99 |
|---|---|---|---|---|
| P0 quiet | 100/100 | 0.05 / 0.13 ms | 100/100 | 0.06 / 0.34 ms |
| P1 flood | **0/100 (503)** | 0.10 / 2.39 ms | **0/100 (503)** | 0.04 / 2.52 ms |
| P2 hung decider | 1/1 (busy) | 0.28 ms | 1/1 | **8459.7 ms** (one sample fills the 6 s window) |
| P3 hung + flood | **0/288** | 0.10 / 2.97 ms | **0/288 (503)** | 0.04 / 2.52 ms |

Invalid push during the hang: 400 after **8359 ms**. An earlier baseline run
(00:22:48) gave the same picture: /stats 8511 ms, P1 0/100, P3 1/201.

### After: 873e929 release

| run | P1 flood /health p99 | P1 /stats p99 | P2 /health p99 | P2 /stats p99 | P3 /health p99 | P3 /stats p99 | all 200 |
|---|---|---|---|---|---|---|---|
| 00:33:59 glibc release | 7.90 ms | 13.87 ms | 17.09 ms | 11.35 ms | 25.67 ms | 8.44 ms | 1326/1326 |
| 00:34:22 glibc release (`after-873e929.json`) | 7.66 ms | 7.95 ms | 4.58 ms | 4.07 ms | 5.16 ms | 4.18 ms | 1528/1528 |
| 00:34:45 static x86_64 musl | 4.86 ms | 6.39 ms | 8.31 ms | 6.69 ms | 4.86 ms | 2.92 ms | 1500/1500 |
| 00:37:22 merged onto live line (`after-live-merge-50f5d43.json`, llm-advisor compiled) | 2.04 ms | 0.89 ms | 7.90 ms | 4.06 ms | 3.05 ms | 4.51 ms | 1534/1534 |

p50 stays 0.1-3 ms in every phase; worst single read over all after-runs
36.7 ms (run 1, box busy with other slots' builds). Every P2/P3 read was
served from the snapshot (`busy` on all of them); the flood was evicted and
reconnected 116-411 times per phase instead of holding the API. Invalid push
during the hang: 400 in 0-4 ms, counted (`events_invalid` 0 -> 2).

### T5.1 chaos re-run (real event-mode nerve bb287cc, SIGSTOP)

`GALION_REAL=1 GALION_T51_PORT_BASE=21060 GALION_NERVE_BIN=<nerve bb287cc release>`:
2/2 green twice (00:25-00:26 on 873e929's tree, 00:38 on the live merge with
llm-advisor). S7 hung nerve: `/stats` **200 in 3 ms / 0 ms** (was 8817-8821
ms), `/health` 3 / 0 ms, decision by `unknown` after 9.7-9.9 s as before. S5
40-connection flood: `/health` **200 during**, recovered after 0 ms (was 503,
5132-5147 ms).

## Gates (873e929)

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -D warnings` 0;
  gaia-cortex with `--features llm-advisor` 0.
- Workspace tests (excl. openmind-sandbox, cgroup-detect, hermes-tools,
  `--no-fail-fast`): 230 suites, 1135 passed / 1 failed / 4 ignored. The one
  failure is `openmind-nerve replay_corpus every_saved_capture_is_in_the_corpus`
  (box state: other slots' `live:replay200`/`replay207` captures), the same as on
  the base (slot 227: 1127/1/4).
- New tests: snapshot 3 unit, `galion-common` `http_evict` 4 (30/30 repeat runs
  green after the grace fix), `http_fuzz` 6/6 still green; real_api_load 1 (5 runs).
- x86_64 + aarch64 musl release static ok; armv7-musleabihf + aarch64-android
  `cargo check` ok; `scripts/no-python.sh` ok. No new dependencies.
- Live-line merge 50f5d43 (`swarm/cortex-api-load-live`, local): one trivial
  conflict (server.rs `/health` fields hermes_handoff + handoffs, now in
  `Cortex::health_json`); gaia-cortex + galion-common 77/0, clippy -D both feature sets 0.

## VPS (read-only, 00:35:41-00:35:50)

`gaia-cortex` + `galion-nerve` active. Live cortex is
`/opt/galion/rust/bin/gaia-cortex-bd6fbe6` (= cortex-live-fixes: e982210 merged
into cortex-hermes-boss), active since 23:36:53, mode live, llm_enabled true,
hermes_handoff on, 0 incidents, 0 busy; `/health` 0.55 ms, `/stats` 0.4-0.7 ms
(5 GETs) while idle. The F4/F5 behaviour above applies to that binary
(it has e982210's server). Nerve 8670 `/healthz` 200, `/health` system
**failing**, score 45, cycle 12 (was 66/D at 23:56). Nothing was changed on the VPS.

## Not done / owner decisions

- **D-236-1** deploy: roll 50f5d43 (or a later live-line merge) to the VPS? Needs
  the usual live-cutover path; not done here (box only).
- **D-236-2** root cause: the decider still holds the engine lock across nerve
  I/O (actions, alerts, escalations). Reads no longer care, but a batch still
  blocks the next batch for up to 9 s per subject. Moving nerve I/O out of the
  lock (plan, call, commit) is the real fix (slot 227's D4); not done.
- **D-236-3** `evict_idle` for other `galion-common::http` services
  (galion-origin, console, ...)? Off by default; turning it on is one line each.
- **D-236-4** snapshot freshness: during a hang reads show the state before the
  batch (age shown). Acceptable, or publish also mid-batch (after each subject)?
- **D-236-5** an evicted request is one that was still sending its head/body at
  the cap (e.g. a slow POST over 32 parallel uploads); on loopback that is
  only floods. Keep, or exempt bodies already in progress?
