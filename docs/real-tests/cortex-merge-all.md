<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# cortex-merge-all (slot 158, cortex plan T6.4 step), 2026-10-21 +02:00

Branch `swarm/cortex-merge-all` = `cortex-t20-merge` ee75f75
+ `cortex-state-titles` 967459d (merge 9b20ac7)
+ `cortex-event-ingest` 7a55e6c (merge 699439b)
+ `cortex-t212-edgerate` 9a48217 (merge 27227d4)
+ `cortex-score-v2` 7863e3d (merge 011222b)
+ one fix-up commit (this file, merge-only tests). No Cargo.toml / Cargo.lock change, no new dependency.

## Conflicts and how they were resolved

| merge | files | resolution |
|---|---|---|
| state-titles | `sensors/mod.rs`, `docs/cortex/sensors.md` | both sides kept (t20 S2 registry + title/fix removal in `cortex_events`) |
| event-ingest | `README.md`, `sensors/mod.rs`, `sensors.md` | `pub mod event_feed;` + `add_opt(EventFeed::from_env)` after `hades`; T2.10 row marked merged |
| t212-edgerate | `README.md`, `sensors/mod.rs`, `tests/cortex_replay.rs`, `sensors.md` | `edge-rate` registered last (after `event-feed`); replay sensor list = built-ins + `edge-rate` |
| score-v2 | `README.md`, `cortex.rs`, `sensors.md` | score v2 is the single owner of the score; `score.rs` and `engine.rs` merged without a textual conflict |

## Semantic (non-textual) conflicts found by the tests, fixed in the fix-up commit

`edge-rate` (T2.12) is **on by default** (`NERVE_EDGE_HOSTS=0` turns it off), so after the merge every nerve
lists it. Tests written on the older bases assumed otherwise:

| test | failure after merge | fix |
|---|---|---|
| `sensors::hades::tests::from_env_off_by_default_and_registered` | `names.last()` = `edge-rate`, not `hades` | assert `hades` registered before `edge-rate` |
| `tests/cortex_s2_c.rs` `replay_with_both_sensors_on_keeps_the_golden` | report had additive `edge-host:*` streams | `support::drop_edge_hosts` (same helper the T2.12 branch uses for the pre-trait golden) |
| `real_cortex_t20`, `real_cortex_s2a`, `real_cortex_s2b`, `real_cortex_s2_c`, `real_cortex_s2_d` | exact sensor-name lists missed `edge-rate` | `edge-rate` appended to each list |

New merge-only tests (paths that no single branch could test):

- `sensors::edgerate::tests::edge_host_flood_is_scored_once_by_score_v2`: edge-host flood -> correlator ->
  score v2: one critical-incident deduction (75), finding `ddos:edge-host-rate` covered by `ddos:edge-cloudflared`.
- `sensors::event_feed::tests::pushed_ddos_events_are_scored_once_by_score_v2`: pushed B33 `ddos` at warn
  costs its own 4 (v1: 0, invisible), pass kept; at error it opens `gaia:<src>`, scored once (85, fail), covered.
- `tests/real_cortex_merge_all.rs` (`GALION_REAL=1`, release, ports 20280-20289), see below.

## Goldens explained

| golden | merged result | why |
|---|---|---|
| `tests/fixtures/cortex-replay` (synthetic, all built-ins fire) | **21/F**, 7 deductions, findings covered 5 / scored 2 | unchanged. T2.12 adds only `edge-host:*` streams (all `normal` in this fixture), stream verdicts are never scored (score v2 rule 3), and the fixture has no pushed events. JSON golden equal after `drop_edge_hosts`; markdown golden equal after `drop_edge_hosts_md`. |
| `nerve-capture/replay1` (live VPS capture 2026-10-20 19:25 +02:00) | **61/D**, findings covered 4 / scored 1 | unchanged: 14 hostname streams, 419 failed requests, all `normal`, so no edge-rate finding. |
| `real_edge_rate` synthetic flood capture (fixture + 2400 lines at 40 req/s on one host) | **11/F** (t212 branch alone under score v1: 0/F) | score v2 scores the edge flood once, as incident `ddos:edge-cloudflared` (-25), and the dead-origin findings it absorbed are covered instead of charged again. Deductions: 25 edge flood, 25 tuner flood, 15 galion-one, 10 :9999 public, 5 scanners, 5 galion-desk, 4 strays = 89. `NERVE_EDGE_HOSTS=0` still gives 21/F. |

## Test numbers (box, 2026-10-21 08:14-08:22 +02:00)

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` 0 warnings
  (excl. openmind-sandbox, cgroup-detect, hermes-tools).
- `cargo test --workspace --no-fail-fast` (same excludes): 205 suites, **1007 passed, 0 failed, 3 ignored**;
  openmind-nerve 31 suites 194 passed (lib 137). Before the fix-up: 1 lib + 1 integration failure (table above).
- `scripts/no-python.sh` ok, `scripts/spdx-scan.sh` scanned 383, missing 0.
- x86_64 musl release `openmind-nerve` static-pie 1 487 648 B; aarch64 musl (rust-lld) static 1 250 144 B;
  `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf ok.

## GALION_REAL=1, release, `--test-threads=1`: 12/12 ok (08:15:59-08:19:03, s2b re-run 08:19:40 +02:00)

The 7 tests of cortex-t20-merge plus the 4 branch tests plus the new merge test:

| test | result |
|---|---|
| real_box | ok (12.3 s) |
| real_cortex_sensor | replay fixture 21/F (= pre-trait golden), replay1 61/D; serve :19430 cycle_ms 23, rss 3512 KiB |
| real_cortex_s2a | sshd flood replay 67/D; rustsec 2 ids; auth-log disk bleed then cleared; cycle_ms 26, rss 4512 KiB |
| real_cortex_s2b | drift + conn-rate detect/clear on :19560 (30.1 s) |
| real_cortex_s2_c | stray-owner, deleted-binary, 3 surface findings; cycle_ms 27, rss 3820 KiB |
| real_cortex_s2_d | hades-gate ledger -> hades sensor -> events, cleared after 30.1 s; rss 3960 kB |
| real_cortex_t20 | all 13 sensors in one nerve (12 + edge-rate), cycle_ms 28, rss 4224 KiB |
| real_cortex_state_titles | `cortex_events` carry no title/fix (state 7356 B) |
| real_event_feed | heal refused/capped/applied, survival probe; cycle_ms 23, event-feed 53 us, rss 3632 KiB |
| real_edge_rate | flood host `flood`, 1 finding, 1 incident, 11/F; off -> 21/F; replay1 61/D |
| real_score_v2 | fixture 21/F covered 5/scored 2; replay1 61/D covered 4/scored 1; serve strays 71 -> 75 |
| **real_cortex_merge_all** (new, :20280) | see below |

`real_cortex_merge_all` output (one real `openmind-nerve serve`, event feed = its own `POST /event` log):

```
1. serve :20280 cycle 1: sensors ["listeners", "scanners", "edge", "components", "ddos", "event-feed", "edge-rate"]; start score 100 "A"
2. cycle 2: deduction [(4, "ddos (warn): t158: edge flood on <addr>")]; score 100 -> 96; cortex_events top keys Some(["cause", "component", "evidence_n", "id"])
3. cycle 3: covered "ddos:ev-t158-edge-flood" by gaia:t158; deductions [(15, "error incident: gaia:t158: tunnel t158: edge flood on <addr> (2 event(s) in the window)")]; score 85 pass false
4. cycle 4: finding cleared; score 85 deductions [(15, "error incident: gaia:t158: ...")]
   cycle 5: incident resolved, score back to 100 "A"; cycle_ms 23
```

The pushed title carried `10.0.0.5:443` and a `fix`; the cortex shows `<addr>`, and `cortex_events` has no
title/fix (state-titles + event-feed cleaning work together).

## Real server (VPS, read-only, 2026-10-21 08:23-08:25 +02:00)

Read-only only: 2x `GET 127.0.0.1:8670/state` (cycles 6937, 6938), `GET :8660/status.json`, `ss -ltn`,
`journalctl -o json` of the running stack units for 30 min (2434 lines), `systemctl is-active/show`.
Nothing written on the VPS. Capture kept on the box at `/workspace/swarm/nerve-capture/replay158` (not committed).

- Live nerve (galion-nerve from 2f5a730, MainPID 1466797, NRestarts 0; shadow active): status `degrading`,
  `/self-eval` 200 with no score field; `/api/v1/state` and `/api/v1/cortex/events` 404 -> **cortex not deployed**,
  observed through the nerve per the standing rule.
- Merged release binary `openmind-nerve replay replay158`: **66/D**, pass false, deductions 15 (edge: 15
  hostnames -> 13 closed origin ports, 31.6 failed req/min), 15 (galion-one-mouth stuck loading 2.4 d),
  4 (stray loopback listener :8672). 15 `edge-host:*` streams, all `normal` -> no edge-rate finding.
  `edge-dead-origins`, `error-rate-cloudflared`, `stuck-loading-galion-one-mouth` covered by their
  incidents (same cause), `memory-heavy-console` info -> 0. No ddos findings, so v1 and v2 agree on live data.

## Open / owner

- Score v2 ddos weights (15/10/4) and "uncovered error-level ddos blocks pass": owner decision carried from
  slot 108 (docs/cortex/score-v2.md), unchanged here.
- `edge-rate` on by default is inherited from T2.12; every sensor-list consumer must expect it.
- Deploying the cortex (nerve -> cortex) is not part of this slot; nothing installed.
