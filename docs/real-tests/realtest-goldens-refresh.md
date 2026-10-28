<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# realtest-goldens-refresh (wave-4 slot 422)

Owner decision D-383-1 option (a): refresh the stale goldens/asserts found by
slot 383 (real-ports-batch-3) in one follow-up slot. Branch
`swarm/realtest-goldens-refresh` off `real-ports-batch-3` 26a526c1. Ports
22920-22929 (via each test's `*_REAL_PORT_BASE`). Test-only: no product code
changed, no new dependencies, nothing deployed. Times Europe/Zagreb (CET +01:00).

## What was stale and why (evidence)

| Finding | Test | Cause (evidence) | Fix |
|---|---|---|---|
| F-383-1 | real_cortex_sensor, real_score_v2 | Golden compared raw. The in-process `tests/cortex_replay.rs` (green) already normalises the same golden with `canon` + `strip_empty_last` + `strip_nerve_wiring` (nerve-wiring 8d7a9240 owner evidence `:8652 unknown owner ... -- process`, pre-trait `(last: )`, health-body key order). The real tests never got that. | Same normalisation in both real tests (JSON and markdown). Points, order and every other word still compared. |
| F-383-2 | real_cortex_merge_all | On this lineage one `POST /event` at `error` also becomes a hub push signal (`server.rs` `event_signal` -> `hub.incident_signals`, 5802d0e1/8d7a9240) with cause `t158`, next to the event-feed incident `gaia:t158`; slot 158 was written on a lineage without that path. The hub signal lives 120 s (`PUSH_WINDOW`), so the old 40 s score-back wait timed out. The lxc-host disk incident was the box disk at 95% (now 41%). | Exactly one `gaia:t158` incident plus at most the hub `t158: pushed 1 error(s) ...` incident; unrelated host deductions ignored for score-back (compared like with like); score-back wait 180 s; score must equal 100 - sum(deductions). Behaviour itself is owner decision D-422-1. |
| F-383-3 | real_cortex_ingress_lint | main-staging dad7984a (2026-10-21T23:36:05+02:00) registers edge-rate after ingress-lint and turned the unit tests' "last sensor" asserts into `contains`; the real test was missed. | Assert ingress-lint registered exactly once. |
| F-383-5 | real_enroll | omnode-enroll 17e15db6 (2026-10-21T17:10:15+02:00) deliberately tells a refused device 403 "the owner refused this device" (`enroll/mod.rs` `OUTCOME_TTL`: "so the waiting device gets a clear answer instead of 'not enrolled'"). The test (c28374e7, 01:01 the same day) predates it. Decision: the message is right, the test was stale. | Expect rc 2 with `(HTTP 403)` + "the owner refused this device" and not "not enrolled". No behaviour change. |

## Real runs (GALION_REAL=1, release, box, ports 22920-22929)

Before (26a526c1, 2026-10-28 20:40-20:49):

- real_cortex_sensor FAIL (golden, line 148); real_score_v2 FAIL ("v2 keeps v1 points");
  real_cortex_ingress_lint FAIL (`left: Some("edge-rate") right: Some("ingress-lint")`);
  real_cortex_merge_all FAIL (inc.len 2 vs 1); real_enroll 71/72 (`rejected B: claim refused: want 2 true, got 2 false`).

After (final run 2026-10-28 20:55:25-20:58:31):

| Test | Result | Key output |
|---|---|---|
| real_cortex_sensor | PASS 1/1 (11 s) | replay fixture 16/F = golden; replay1 live capture 56/D; serve :22920 cycle 1 sensors listeners/scanners/edge/components/ddos/edge-rate, cycle_ms 28, rss 3864 KiB; stray cleared cycle 2; :22924 SENSORS_OFF=listeners, stray :22925 not reported |
| real_score_v2 | PASS 1/1 (6 s) | replay 16/F, 7 deductions, covered 5 / scored 2; replay1 56/D; serve :22920 71 C deductions [25, 4], strays gone 75 C |
| real_cortex_ingress_lint | PASS 1/1 (17 s) | phase 1 ingress-closed-22922/22923 (5.5 s, cycle 2); phase 2 ingress-closed-22924 (5.0 s) |
| real_cortex_merge_all | PASS 1/1 (140 s) | start 100 A; warn ddos -4 -> 96; error: gaia:t158 + hub t158 incidents (15 + 15) -> 70; info clears; both incidents resolved after 120.3 s, back to 100 A |
| real_enroll | PASS 72/72 (12 s) | `PASS rejected B: claim refused (2 true)`; node B aarch64 under qemu-user |

## Negative controls (local, never committed, reverted after each)

| Control | Expected | Result |
|---|---|---|
| NC1a golden last deduction points +1 | sensor + score_v2 fail | both FAIL (real_cortex_sensor.rs:153, real_score_v2.rs:201) |
| NC1b golden deduction `why` "LISTEN 0.0.0.0:9999" -> ":9998" | both fail | both FAIL (same lines) |
| NC3 nerve started with `NERVE_SENSORS_OFF=ingress-lint` (phase-1 ids assert disabled) | count assert fails | FAIL line 220: `left: 0 right: 1` |
| NC2 two `error` pushes instead of one | hub-incident assert fails | FAIL line 279: `t158: pushed 2 error(s)` not the expected single push |
| NC5 hermes-rs outcome reply put back to 401 "not enrolled" | check fails | FAIL `rejected B: claim refused: want 2 true, got 2 false` |

Removing the fixes = the "before" column (all red).

## Gates (2026-10-28 20:52-20:55)

`cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets
--exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok;
`env -u GALION_REAL cargo test --workspace --exclude openmind-sandbox
--exclude cgroup-detect --exclude hermes-tools`: 378 suites 2011/0/18;
`scripts/no-python.sh` ok; SPDX headers present on every touched file. No
sandbox test run on the box (GALION_REAL unset for the workspace run). Test
files only, so no musl/android build change.

## VPS (read-only, 2026-10-28 20:51:52)

`GET 127.0.0.1:8670/state` 200, 32,844 B: mode live, cycle 14, ts 20:40:13,
score 68 D, degraded, sensors listeners/scanners/edge/components/ddos, open
incidents edge (error) + galion-one (error). Cortex :8674/health 200;
galion-nerve and hermes-rs active. The live nerve has no event-feed sensor,
so the D-422-1 double count does not hit live today. Nothing changed on the
VPS.

## Findings and owner decisions

- F-422-1 / D-422-1: one `POST /event` at error opens two 15-point incidents
  (`gaia:<src>` from event-feed and `<src>` from the hub), so one cause costs
  30 when event-feed is on. (a) fold: the hub uses the event-feed cause
  `gaia:<src>` for /event-derived signals (one nerve-merge slot, tests
  above then tighten to exactly one), (b) the hub skips /event signals when
  event-feed reads the same log, (c) keep as is. Recommend (a).
- F-422-2 / D-422-2: the hub incident text carries the raw event data
  (`last: {"title":"edge flood on 10.0.0.5:443","fix":"ban 10.0.0.5"}`) into
  self_eval deductions, while the event-feed path redacts it as `<addr>`.
  (a) redact addresses in hub `msg` the same way, (b) leave. Recommend (a),
  with D-422-1.
- Not in scope, unchanged: F-383-4 connrate, F-383-6 `file`, F-383-7 gaia
  TEST_PORTS, F-383-8 dst replay ids, F-383-9 t20 RSS.

Third-party: none added.
