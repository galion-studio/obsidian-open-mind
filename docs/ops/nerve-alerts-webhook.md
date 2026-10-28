<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve alert webhook (staged, not live)

Slot 382 `nerve-alerts-stage`, branch `swarm/nerve-alerts-stage` off nerve-merge-6 9baa450c.
Written 2026-10-28 10:45 +01:00 (Europe/Zagreb, CET).

## Why

Slot 362 found the live nerve has no alert channel: every alert goes only to
journald and `/var/lib/galion-nerve/alerts.json`, so an incident is only seen
by someone polling `/api/v1/state`. The live nerve says so itself
(read-only probe 2026-10-28 10:37 +01:00):
`"external":"none configured (no Telegram/Hermes alert channel found on the host)"`.
The unit sets no `NERVE_TELEGRAM_ENV`, so the built-in Telegram path is off too.

## What is staged

1. Code: `crates/openmind-nerve/src/alert_sink.rs` (std only, no new dependency).
   Off unless `NERVE_ALERT_WEBHOOK` is set. Wired into `Alerts::raise`, so it
   sees exactly the alerts that go to journald (after the existing 30-min
   per-key dedup).
2. Config: `deploy/staged/nerve/galion-nerve.service.d/50-alert-webhook.conf`
   (NOT installed). Option A values: ntfy format, `crit` only, 6/h, target
   `http://127.0.0.1:2586/galion-nerve`.

| Variable | Default | Meaning |
| --- | --- | --- |
| `NERVE_ALERT_WEBHOOK` | unset (off) | `http://<loopback>[:port]/path` only |
| `NERVE_ALERT_WEBHOOK_FORMAT` | `json` | `json` or `ntfy` |
| `NERVE_ALERT_WEBHOOK_MIN` | `warn` | `info`, `warn`, `crit` |
| `NERVE_ALERT_WEBHOOK_PER_HOUR` | 6 | sliding-hour cap, clamped 1..30 |

### No secrets, by construction

The URL must be plain `http://` to `127.0.0.0/8`, `::1` (also `::ffff:127.x`)
or `localhost`. Userinfo (`user:pw@`), query (`?token=`) and fragments are
refused, and so is any non-loopback host (`ntfy.sh`, `10.x`, `0.0.0.0`).
`send()` re-checks loopback before connecting. So no credential can be put in
the nerve's environment or in the repo; whatever relay holds an upstream token
(ntfy auth, Telegram bot, Hermes) listens on that loopback port and keeps it.

A rejected value does not stop the nerve: it keeps serving, `/state`
`alerts.webhook` shows `{"configured":false,"rejected":"..."}`, the self-eval
`alerts-channel` finding goes to `warn`, and nothing is sent.

### Bodies

* `json`: `POST <path>` `application/json`
  `{"source":"galion-nerve","ts":"2026-10-28T10:41:50+01:00","severity":"crit","component":"dead-a","key":"status:dead-a:failing","msg":"..."}`
  (msg capped at 1000 chars, control chars stripped).
* `ntfy`: `text/plain` body = msg, headers `Title: galion-nerve <sev> <component>`,
  `Priority: default|high|urgent` (info|warn|crit), `Tags: galion-nerve,<sev>`.
  Header values are ASCII-printable only (no header injection, unit-tested).

### Cost to the cycle

One blocking HTTP/1.1 request per sent alert, 1 s connect + 2 s read/write
timeouts. Loopback refuses closed ports instantly; a hung relay costs at most
~4 s per send and at most `per_hour` sends an hour. Rate-limited and
below-floor alerts cost nothing.

### Observability

`/api/v1/state` -> `alerts.webhook`: `url, format, min, per_hour, in_window,
sent, failed, rate_limited, below_min, last{ts,key,result,http|error}`;
`alerts.external` and the `alerts-channel` self-eval line carry a one-line
summary; `serve` logs `alerts external: ...` at start. `once` and `replay`
never send.

## Sizing from live data

Live journal of `galion-nerve`, 24 h to 2026-10-28 10:43 +01:00 (read-only):
249 warn+crit ALERT lines (214 warn, 35 crit: console 26, ddos 7, hermes-rs 2).
warn+crit exceeded 6 in 22 of 24 hours (109 would be dropped at 6/h); crit
peaked at 6 in one hour (2026-10-27 16:00-17:00 +01:00). So `min=crit` at
6/h delivers every crit alert seen in the last day, and `min=warn` would be
mostly noise from the open console / galion-one-mouth incidents.

## Owner decisions

* **D-382-1 target** (needed before install):
  * A. self-hosted ntfy on `127.0.0.1:2586` (staged values; ntfy is Apache-2.0
    and a single Go binary, not installed on the VPS; port free 10:43 +01:00).
    Phone gets push via the ntfy app through the console tunnel or ntfy's own
    upstream; auth stays in ntfy's config.
  * B. a small Rust relay (`json` format) that forwards to Telegram with the
    bot token in its own env file (the nerve's built-in Telegram path already
    exists: `NERVE_TELEGRAM_ENV`, but it reads the token itself).
  * C. hermes-rs (:8642) as the relay, if it grows a loopback
    `POST /notify` endpoint (it has none today; would be its own slot).
  * D. keep journald-only (status quo).
* **D-382-2 floor + cap**: recommend `crit`, 6/h (above). `warn` needs a
  higher cap or the console incident must be fixed first.
* **D-382-3 deploy**: needs a nerve build from this branch (live nerve
  9d1a4f3 does not know these variables), then the drop-in, then
  `daemon-reload` + restart by the owner. Not done here (no live change).
