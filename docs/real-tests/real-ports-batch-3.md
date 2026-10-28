<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# real-ports-batch-3 (wave-4 slot 383)

F-375 follow-up: the next 20 `tests/real_*.rs` files with literal ports
(18 in openmind-nerve covering nerve + cortex, 2 in hermes-gateway) now read a
port base from env through `galion_realtest::ports::Block`. Unset = the old
default block, so earlier runs and docs stay valid; a bad value panics naming
the var. `NERVE_LISTEN_SCOPE` follows `PORTS.range()`. Branch
`swarm/real-ports-batch-3` off `swarm/real-ports-batch-2` 7c3de266. Box only;
VPS read-only. Times Europe/Zagreb (CET +01:00), 2026-10-28.

## Env knobs (span 10 unless noted)

| test | env var | default | layout |
|---|---|---|---|
| nerve real_box | NERVE_BOX_REAL_PORT_BASE | 19105 (span 3) | nerve +0, live +1, dead +2 |
| nerve real_connrate_fix | NERVE_CONNRATE_REAL_PORT_BASE | 20480 | identity +0, svc +1, flow +2 |
| nerve real_consumer_compat | NERVE_CONSUMER_REAL_PORT_BASE | 20500 | nerve +0, dummy +1, nolog nerve +2 |
| nerve real_cortex_merge_all | CORTEX_MERGE_ALL_REAL_PORT_BASE | 20280 | nerve +0 |
| nerve real_cortex_s2_d | CORTEX_S2D_REAL_PORT_BASE | 19580 | nerve +0, svc +1 |
| nerve real_cortex_t32 | CORTEX_T32_REAL_PORT_BASE | 20490 | nerve +0, origin +1, surface +2 |
| nerve real_dst_clock | NERVE_DST_REAL_PORT_BASE | 20270 | nerve +0, svc +1 |
| nerve real_event_feed | CORTEX_EVENT_FEED_REAL_PORT_BASE | 19760 | nerve +0, svc +1; gaia `--allow-ports` = range |
| nerve real_cortex_t20 | CORTEX_T20_REAL_PORT_BASE | 19740 | nerve +0, svc +1, 503 +2 |
| nerve real_cortex_state_titles | CORTEX_TITLES_REAL_PORT_BASE | 19750 | nerve +0, svc +1, wild +3, stray +4 |
| nerve real_cortex_sensor | CORTEX_SENSOR_REAL_PORT_BASE | 19430 | nerve +0, live +1, dead +2, stray +3, 2nd nerve +4, 2nd stray +5 |
| nerve real_score_v2 | SCORE_V2_REAL_PORT_BASE | 19780 | nerve +0, live +1, dead +2, strays +3 +4 |
| nerve real_edge_rate | EDGE_RATE_REAL_PORT_BASE | 19770 | nerve +0, 2nd nerve +1, dead +2 |
| nerve real_cortex_s2a | CORTEX_S2A_REAL_PORT_BASE | 19550 | nerve +0, svc +1, lock nerve +2 |
| nerve real_cortex_s2b | CORTEX_S2B_REAL_PORT_BASE | 19560 | nerve +0, svc +1 |
| nerve real_cortex_s2_c | CORTEX_S2C_REAL_PORT_BASE | 19570 | nerve +0, stray +1, ghost +2, TLS +3, 503 +4, down +5 |
| nerve real_cortex_ingress_lint | INGRESS_LINT_REAL_PORT_BASE | 20290 | A +0, origins +1..+4, B +5 (`@N` placeholders) |
| nerve real_no_openrouter | NO_OPENROUTER_REAL_PORT_BASE | 21110 | delegate +0, refuse +1, hermes decoy +7, dead +8, OpenRouter decoy +9 |
| hermes-gateway real_enroll | HERMES_ENROLL_REAL_PORT_BASE | 19720 | P +0, PA +1, PB +2, PPROBE +3 |
| hermes-gateway real_omnode_enroll | OMNODE_ENROLL_REAL_PORT_BASE | 22000 | P +0, PT +1 |

Already movable before this slot (not touched): galion-desk real_wired
(GALION_REAL_DESK...), real_wiring (REAL_WIRING_PORT_BASE), real_mem_growth
(HMG_PORT_BASE). link-watch has no real port tests.

## Real runs (GALION_REAL=1, base 22530, release)

Round 1 11:19-11:25, round 2 11:28-11:33. Each run printed
`real ports: VAR -> 127.0.0.1:22530-22539` (real_box: 22530-22532).

| test | result | port evidence |
|---|---|---|
| real_box | PASS 1/1 12.2 s | nerve on 22530 |
| real_edge_rate | PASS 1/1 | serve :22530 cycle 2 edge-rate; :22531 NERVE_SENSORS_OFF=edge-rate |
| real_cortex_s2a | PASS 1/1 20.6 s | serve :22530 rustsec + auth-log bleed; :22532 lock nerve |
| real_cortex_s2b | PASS 1/1 35.1 s | :22530 drift; "port 22531: 95.6 new connections/s" crit |
| real_cortex_s2_c | PASS 1/1 10.7 s | serve :22530 proc-owner/surface, strays cleared |
| real_cortex_s2_d | PASS 1/1 36.2 s | hades findings cycle 1, cleared cycle 7 |
| real_cortex_t20 | PASS 1/1 5.5 s | serve :22530 all 13 sensors |
| real_cortex_t32 | PASS 1/1 15.6 s | origin up, edge resolved, score 84 B |
| real_cortex_state_titles | PASS 1/1 | public-listener-22533, serve :22530 cycle 1 |
| real_event_feed | PASS 1/1 | serve :22530 event-feed; GAIA_BIN unset (F-383-7) |
| real_consumer_compat | PASS 2/2 | nerve-healthz/dashboard 200, gaia /state contract |
| real_no_openrouter | PASS 2/2 | delegate llm_calls=0 decoy_connections=0; :22537 refused |
| real_score_v2 | port path PASS (golden fails, F-383-1) | serve :22530 score 71 C, strays gone cycle 2 75 |
| real_cortex_sensor | port path PASS (goldens fail, F-383-1) | serve :22530 cycle 1+2; :22534 SENSORS_OFF, stray :22535 not reported |
| real_cortex_ingress_lint | port path PASS (order assert, F-383-3) | ingress-closed-22532/22533, -22534; nerve B config refused |
| real_cortex_merge_all | FAIL (F-383-2) | phases 1-4 on 22530 ran; then score-back timeout |
| real_connrate_fix | FAIL 2/2 runs (F-383-4) | both nerves reported conn-rate on 22531 ~20/s |
| real_dst_clock | 1/2 (F-383-8) | serve test on 22530 PASS; VPS replay fold ids differ |
| real_enroll | 71/72 (F-383-5) | all port claims on 22530-22533 |
| real_omnode_enroll | 54/55 (F-383-6) | 22530/22531; only the `file` check fails |

"port path PASS" = run with that one golden/order assert temporarily
skipped (local, never committed) so every port-using phase was proven; the
committed asserts are unchanged.

VPS read-only probe 11:19:46: nerve /state cycle 79, degraded, 2 open
incidents (galion-one-mouth stuck loading 16.8 h / 503, cloudflared
failing-soon, tuner-quick failing, voice-worker absent); cortex :8674/health
200; galion-nerve active.

## Findings

- F-383-1 real_cortex_sensor + real_score_v2 cortex-replay goldens stale on
  this lineage (fail before any port is used).
- F-383-2 real_cortex_merge_all: extra incident "t158: pushed 1 error(s)"
  (event-feed + gaia correlation, like F-382-2) plus an lxc-host
  failing-soon disk incident; inc.len 2 vs 1, score stuck 70.
- F-383-3 real_cortex_ingress_lint: sensor-order assert stale (edge-rate now
  after ingress-lint).
- F-383-4 real_connrate_fix: flow/identity ratio (26.6 vs 20.2) and old-B41
  delta 4.8 under steady load; fails 2/2 at load ~1.
- F-383-5 real_enroll: hermes-rs says "rejected" where the test wants
  "not enrolled" (enroll/mod.rs:46).
- F-383-6 box lacks `file`; readelf confirms static AArch64 EXEC, no INTERP.
- F-383-7 gaia heal TEST_PORTS guard 19000-19999 refuses slot ranges above
  it, so GAIA_BIN-driven event_feed runs only work on default ports.
- F-383-8 real_dst_clock VPS replay: fold vs control incident ids differ
  while the live nerve has open incidents (ids not stable across replays).

## Owner decisions

- D-383-1 stale goldens/asserts (F-383-1/2/3/5): (a) refresh in one
  follow-up slot, (b) per-test slots, (c) leave and mark known-red.
- D-383-2 gaia guard (F-383-7): (a) widen TEST_PORTS to 19000-22999,
  (b) env override for tests only, (c) keep and run gaia tests on defaults.
- D-383-3 connrate (F-383-4): (a) widen tolerance, (b) investigate b41
  flow counter, (c) mark ignored until fixed.
- D-383-4 `file` on the box (F-383-6): (a) apt install after update,
  (b) switch the test to readelf, (c) skip when missing.

## Restart r3 (2026-10-28 14:05-14:13 +01:00)

- Committed the leftover clippy fix (`needless_borrow` on
  `.env("NERVE_LISTEN_SCOPE", PORTS.range())`, 7 nerve tests) as 25afe662;
  reverted the test-generated `docs/bench/vending.score.json` timestamp churn.
- Gates: `cargo fmt --all --check` OK; `cargo clippy --workspace
  --all-targets -- -D warnings` OK; `env -u GALION_REAL cargo test
  --workspace --exclude openmind-sandbox --exclude cgroup-detect` rc=0,
  **2033 passed / 0 failed / 18 ignored** (14:06-14:07).
- GALION_REAL=1 re-runs on slot base 22530 (14:07-14:11):

| test | result | evidence |
|---|---|---|
| real_cortex_s2_d | PASS 1/1 30.2 s | `real ports: CORTEX_S2D_REAL_PORT_BASE -> 127.0.0.1:22530-22539`; hades-gate forged/replayed/kill refused, valid token WouldApply |
| real_cortex_t32 | PASS 1/1 15.6 s | fixture replay 16/"F" in 35 ms |
| real_event_feed | PASS 1/1 25.1 s | event-feed on 22530 |
| real_dst_clock serve | PASS | offset-named self-evals on 22530 |
| real_dst_clock VPS replay (NERVE_DST_SSH, GET /state only) | FAIL (F-383-8, unchanged) | live snapshot 14:00:01 +01:00, 14 components, degraded; fold ids {inc-55399539, inc-9a32fa93} differ from control |
| real_cortex_t20 | FAIL 3/3 (F-383-9) | port path OK (serve :22530 cycle 2, all 13 sensors, cycle_ms 33-35) but rss 11728-11828 KiB > 8 MiB budget |
| real_cortex_t20 on default 19740 | FAIL, same | rss 11680 KiB -> not port-related |

- VPS read-only probe 14:06:18: nerve `/state` 200, 38,385 B, cycle 91,
  galion-one-mouth still stuck loading (19.8 h, 503); cortex :8674/health
  200; galion-nerve active. Nothing written on the VPS.

### More findings

- F-383-9 real_cortex_t20 RSS budget (8 MiB) now exceeded at ~11.7 MiB on
  both slot and default ports; it passed in round 1 (11:2x). Likely the
  release nerve was rebuilt with more features (workspace-wide build /
  feature unification, cf. 385 nerve-dep-growth default-on sensors) — needs
  a follow-up to pin the build features or re-baseline the budget (D-383-5).
- F-383-10 (process) the box shell env has `GALION_REAL` set globally, so
  the r2 "workspace test" (11:40-13:08) actually ran every GALION_REAL test,
  on DEFAULT ports, including galion-realtest `real_sh_sandbox` on the box
  (breaks sandbox_vps_only; it passed, contained, temp dirs gone, no cgroups
  left). r3 runs the workspace gate with `env -u GALION_REAL`. Owner: unset
  GALION_REAL in the shared env (D-383-6).

- D-383-5 t20 RSS: (a) re-baseline to 12 MiB, (b) build the test's nerve
  with pinned default features, (c) leave red.
- D-383-6 GALION_REAL in shared env: (a) remove it from the box profile,
  (b) make cargo-slot strip it unless asked, (c) leave as is.
