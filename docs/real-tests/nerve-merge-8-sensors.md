<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-merge-8-sensors real tests (slot 424), 2026-10-28 CET (+01:00)

The next nerve deploy candidate after nerve-merge-7 (slot 411). It is merge prep only and **not deployed**.
Branch `swarm/nerve-merge-8-sensors`, based on nerve-merge-7 `0d2d964f`. That branch was already pushed at 18:51, so it is the base, not eaa5d0ac.
Box ports 22940-22949. The VPS was only read.

## What was folded in

| step | commit | source |
|---|---|---|
| merge | 1acc55ed | `nerve-s2-sensor-gates` b5a340c8 (D-385-2 / D-404-1). This brings in 385 8f40f18e and the whole main-staging line it sits on. |
| cherry-pick | dc841156 | 6b264d54: the nerve half of `memory-freshness-sensor` (D-405-1 / D-388-3), with a new default-on feature `sensor-memory-freshness` |
| cherry-pick | 5f1ff51b | 1010eb7c: live-copy real test, staged drop-in, docs |
| fix | 2e1a5af1 | real_memory_freshness_sensor now needs `OPENMIND_MEMORY_BIN` |
| merge | de22cdca | `nerve-alerts-stage` b074ca20 (D-382-1 option A: loopback json/ntfy webhook, off by default, crit 6/h) |
| fix | 12bca089 | F-424-1, alert sink egress guard |
| fix | def08b46 | F-424-2, `compiled_out` cfg |
| fix | 71a21618 | F-424-3, repo-check after the line merge |

1d7164b4 (memory-db-stale-diag) was **not** brought in. It drags in non-nerve work: openmind-memory `freshness.rs`, zagreb-time and local-time changes. Only its nerve commits were cherry-picked, as row 424 allows.
As a result, this branch's `openmind-memory` has no `freshness` subcommand. The sensor needs a memory binary built at or after 1d7164b4 (D-405-3 / D-424-2).

Conflicts resolved in 1acc55ed (11):
- `Cargo.toml` of galion-deploy, galion-health (0.2.0), galion-one and openmind-nerve: unions. The S2 features stay next to `journal-native` / `sdjournal`.
- `openmind-nerve/src/config.rs`: Spec has both `deep` and `max_age_s`.
- `repo-check/src/lib.rs` and its README: both `Check::Nerve` and `Check::Deps`, `ALL: [Check; 10]`.
- openmind-queue `handle_conn`: auth / 401 / persist_refusals plus `nerve_client::observe_request`.
- galion-health `main.rs`: the worker pool, plus nerve started/heartbeat. `handle()` wraps `serve()` and reports the status code.
- `evt/journal.rs`: ours.
- `Cargo.lock`: theirs plus sdjournal.

Alert merge (de22cdca): alerts.rs keeps both `load_note` and `webhook`. The engine does both the sensors and `set_webhook`. Replay clears the webhook.

## Findings fixed on this branch

- **F-424-1:** the alert webhook (slot 382) did a raw TCP connect without the egress guard that the main-staging line enforces. `no_openrouter::every_egress_site_is_guarded` failed.
  - Fix: `parse_url` runs `egress::check_path` and `send` runs `egress::check_addr`.
  - The guard test knows `alert_sink.rs`, and a new test `alert_webhook_refuses_llm_paths` covers it.
  - The sink was already loopback only, so behaviour is unchanged for valid URLs.
- **F-424-2:** `compiled_out` was compiled only when one of the 385 features was off. The `s2-off` build (all 385 features plus memory freshness off) failed with E0425.
  - Fix: `sensor-memory-freshness` was added to its cfg.
  - The real test's `ALL_FEATURES` and size table now know about the feature.
  - Clippy is green for every feature combination tried: lean, 385-only, memfresh-only, and everything except memfresh.
- **F-424-3:** repo-check failed with 6 errors after the line merge. The nerve T2.4 check met main-staging bins, and the alert_sink tests contained public IPs.
  - Fix: the tests now use documentation IPs.
  - The probe-only list gains galion-ccwrap, tunnel-lint, and guardian-tick (which reaches the nerve through the event file).
  - It also gains an **INTERIM** line for console-static, a live console front, until nerve-client is wired into it (D-424-3).

## Gates (box, 21:14-21:35)

- `cargo fmt --check`: ok.
- clippy `-D warnings`: workspace (excluding openmind-sandbox, cgroup-detect, hermes-tools), nerve lean, nerve `journal-native`, and the feature matrix above. All rc 0.
- Workspace tests (no fail-fast, same excludes) at 12bca089: 399 suites, **2094 passed, 0 failed**, 18 ignored. The later commits are re-run below.
- Nerve default: 59 suites, 423 passed, 0 failed, 3 ignored.
- Nerve lean (`--no-default-features`): 38 suites, 270 passed, 0 failed.
- Nerve `journal-native`: 59 suites, 427 passed, 0 failed, 3 ignored.
- Other checks:
  - repo-check: PASS (2213 files, 0 errors, 88 warnings).
  - repo-check tests: 39/39.
  - `scripts/no-python.sh`: ok.
- Cross-target:
  - aarch64-musl release, rust-lld: default 1 657 816 B, lean 1 334 376 B.
  - `cargo check` ok for aarch64-linux-android, armv7-linux-androideabi and armv7-unknown-linux-musleabihf.

## Real tests (GALION_REAL=1)

- **real_s2_sensor_gates:** ok, 222 s, `NERVE_S2G_PORT_BASE=22940`, `NERVE_S2G_TABLE=1`.
  - default :22940 runs 15 sensors (the 5 built-ins plus the 7 S2 plus 385's 3) with no warnings.
  - s2-off :22941 runs 8 sensors and gives 7 warnings.
  - lean :22942 runs the 5 built-ins and gives 10 warnings.
  - lean-bare: 1 warning (edge-rate default). lean-edge-off: 0 warnings.
  - default-bare: built-ins plus edge-rate.
- **real_build_stamp:** ok, 129 s, ports 22946-22948.
  - A, B and C are byte-identical, sha256 6f0f92a0.
  - `/health` build `{"commit":"12bca089…","date":"2026-10-28T21:14:53+01:00"}`.
  - The unstamped D reports `unknown`.
  - Stamp delta against live 9baa450c (1 479 456 B): **+536 608 B**.
  - A second run against `NERVE_STAMP_BASE=0d2d964f` measured 1 483 552 B, a **+532 512 B** delta. The run then failed only on the expected `--version` assert, because the test wants a pre-stamp base and 0d2d964f is already stamped. So this is a size reading, not a defect.
- **real_memory_freshness_sensor:** 2/2 ok, 64 s, ports 22943-22947.
  - `OPENMIND_MEMORY_BIN` was a release build of `wt/memory-freshness-sensor` 1010eb7c.
  - A stale store (223.5 h) raises `memory-stale` (warn) and an alert.
  - A real `/capture` makes it fresh, and the incident resolves in 15 s.
  - A fresh store gives no finding. An old binary gives `memory-freshness-unknown` (info).
  - The sensor costs 30 µs per cycle.
- **real_alert_webhook:** ok, `REAL_NERVE_PORT_BASE=22940`.
  - The json sink on :22941: sent 2, failed 0, rate-limited 2. So the egress guard lets loopback through.
- **real_no_openrouter:** 2/2 ok.

## Size table, x86_64-musl release (budget 2 MiB = 2 097 152 B, F-376-1)

| build | bytes | headroom | vs default |
|---|---:|---:|---:|
| default (S2 + 385 + memfresh) | 2 016 064 | 81 088 | - |
| minus memory-freshness | 2 003 776 | 93 376 | -12 288 |
| minus sshd | 2 007 872 | | -8 192 |
| minus conn-rate | 1 995 584 | | -20 480 |
| minus proc-owner | 1 975 104 | | -40 960 |
| minus surface | 1 971 008 | | -45 056 |
| minus hades | 1 995 584 | | -20 480 |
| minus event-feed | 1 987 392 | | -28 672 |
| minus edge-rate | 1 999 680 | | -16 384 |
| s2-off (385's 3 only) | 1 827 648 | 269 504 | -188 416 |
| lean (`--no-default-features`) | 1 631 040 | 466 112 | -385 024 |
| live 9baa450c (stamp test Z) | 1 479 456 | 617 696 | -536 608 |
| nerve-merge-7 0d2d964f | 1 483 552 | 613 600 | -532 512 |

Default is under budget but has only **3.9 % headroom** (81 KB). Under D-404-2 the deploy recipe is an owner choice (D-424-1):
- default, which matches live behaviour plus off-by-default sensors;
- lean plus edge-rate;
- slot 404's recommended (d).

## Live nerve, VPS read-only (21:15)

- `galion-nerve` is active: MainPID 1251569, NRestarts 0, up since 17:40. It runs 9baa450c with no `commit` in `/health`.
- `/state`: cycle 16, score 68, grade D, degraded.
  - There are 2 open error incidents: edge closed origins with 77.7 failed req/min, and galion-one-mouth stuck loading.
  - Cortex sensors: listeners, scanners, edge, components, ddos.
  - External alerts: none configured.
- No S2, memfresh or webhook env is set on the unit, so after this candidate is deployed the new sensors stay off until they are switched on. :8674 cortex returned 200.
- Nothing was changed on the VPS. A temp file was written to `/tmp` there and removed.
