<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: nerve-alerts-stage (slot 382)

Branch `swarm/nerve-alerts-stage` off nerve-merge-6 9baa450c. Box ports 22520-22529.
All times Europe/Zagreb (CET +01:00). Nothing installed, deployed or changed on the VPS.

## Live VPS, read-only (ssh, GET / systemctl show / journalctl reads)

2026-10-28 10:37 +01:00, `GET 127.0.0.1:8670/state` (42,409 B):
`alerts.external = "none configured (no Telegram/Hermes alert channel found on the host)"`;
newest alert = incident inc-865945dd [error] galion-one-mouth HTTP 503, loading 16.2 h.
`systemctl show galion-nerve`: active, NRestarts 0, env sets NERVE_ADDR/COMPONENTS/DATA_DIR/
DRY_RUN/EVENT_LOG/EVT_ACTIONS/EVT_ESCALATE/FAIL_CYCLES/INTERVAL/MODE/REPORT_EVERY/ROLLBACK
(names only, no values printed), no NERVE_TELEGRAM_ENV, no NERVE_ALERT_WEBHOOK.

2026-10-28 10:43 +01:00, journal of galion-nerve, last 24 h: 214 `ALERT warn`, 35 `ALERT crit`
(console 26, ddos 7, hermes-rs 2). warn+crit per clock hour: 24 hours, 22 above 6, 109 over a
6/h cap; max 17/h (2026-10-27 11:00, 19:00 +01:00). crit max 6/h (2026-10-27 16:00 +01:00).
`/api/v1/alerts` now: 50 warn. `:2586` not listening; no ntfy binary. `/api/v1/state` 200.

## Box, real binary (`GALION_REAL=1`, `--release`)

`REAL_NERVE_PORT_BASE=22520 GALION_REAL=1 cargo test -p openmind-nerve --release --test real_alert_webhook -- --nocapture`
(nerve :22520, capturing receiver :22521, closed component ports :22522/:22523, down receiver :22524).
Run 2026-10-28 10:50 +01:00, 12.55 s, pass (also passed 10:41, 10:48, 10:49):

| Case | Result |
| --- | --- |
| json, warn, 2/h | 4 alerts in alerts.json; receiver got exactly 2 POSTs; sent 2, rate_limited 2, failed 0; body `{"component":"dead-a","key":"status:dead-a:failing","severity":"crit","source":"galion-nerve","ts":"2026-10-28T10:50:18+01:00",...}` |
| ntfy, crit | 4 crit sent = 4 crit in alerts.json, below_min = warn count; `Title: galion-nerve crit dead-a`, `Priority: urgent`, `Tags: galion-nerve,crit`, text/plain body |
| receiver down | failed 4, sent 0, last error `connect: connection refused`, nerve at cycle 2 (keeps cycling) |
| `http://ntfy.sh/galion` | nerve serves; `webhook {"configured":false,"rejected":"host \"ntfy.sh\" is not an IP literal or localhost"}`; self-eval alerts-channel = warn; nothing sent |
| unset | `external` = the exact live string above; alerts-channel info |

Regression, same port base, `--test-threads=1`, 10:50 +01:00: real_console_deep 1/1,
real_own_services 3/3, real_planned_stop 1/1, real_staged_console 2/2, staged_alert_webhook 1/1,
staged_components 2/2. Ports 22520-22529 free afterwards, no leftover processes.

Unit: alert_sink 5 tests (URL rules incl. userinfo/query/https/non-loopback/`::ffff:8.8.8.8`,
config defaults/clamp/errors, JSON + ntfy request shapes incl. header injection, floor +
sliding-hour cap + failures, send to a capturing listener).

## Gates (10:46-10:52 +01:00)

`cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox
--exclude cgroup-detect -- -D warnings` ok; `cargo test --workspace --exclude openmind-sandbox
--exclude cgroup-detect` 288 suites, 1364 passed, 0 failed, 6 ignored; openmind-nerve 25 suites
186/0/0; `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 562 scanned, 0 missing.
Sandbox tests not run (sandbox_vps_only; not touched by this slot).

## Observations

* F-382-1: status alert text doubles its prefix (`failing: failing: health probe down ...`):
  `engine.rs` prepends `status.as_str()` to reasons that already start with it. Cosmetic,
  pre-existing; left as is.
* F-382-2: one dead critical component yields 2 crit alerts in the first cycle (status + incident),
  so the per-hour cap counts roughly two per real outage.
