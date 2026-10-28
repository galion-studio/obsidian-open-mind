<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# F-380-1 nerve edge failure overcount (~37x)

Slot 394. Europe/Zagreb times.

## Cause

`LogInsights::merge` correctly adds refusals / failed_requests from each
engine cycle. Live nerve runs `--mode event` with `heartbeat_s=900` and
`LOG_WINDOW_S=1800`: every cycle merges **new** journal lines since
`last_scan`, then calls `expire(now - 1800)`.

`expire` used to:

1. drop `edge_failures` stamps older than the cutoff,
2. drop dead-origin **ports** whose `last` was older than the cutoff,
3. bump `window_first` to the cutoff,

but it **left `refusals` / `failed_requests` at their lifetime totals** for
any port still receiving traffic. The edge sensor then computed

`per_min = failed_requests * 60 / (window_last - window_first)`

with a short denominator (~30 min) and a long numerator (hours of uptime)
→ roughly `uptime / window` inflation. Slot 380 measured ~37x vs a direct
cloudflared journal count.

Overlapping journal re-reads were a secondary hunch; the live event-mode
cursor already advances, so the dominant bug is expire-without-recount.

## Fix

`DeadOrigin` now keeps per-event `refusal_at` / `failed_at` stamps.
`merge` extends them and derives counts from their lengths. `expire`
retains stamps `>= cutoff`, recounts, and drops empty ports. JSON output
is unchanged (counts + last only).

## Proof

- Unit: `expire_drops_old_failed_request_counts` (18 h @ 30/min → still 30/min).
- `GALION_REAL=1` `real_edge_overcount_engine_replay`: 12 h engine-shaped
  merge+expire → 900 in-window failures, 30.03/min.
- `GALION_REAL=1 GALION_REAL_VPS=1` `real_edge_overcount_vps_journal_replay`:
  read-only `journalctl -u cloudflared` (hosts scrubbed to `ex.test`, never
  committed); engine replay failed count == single-pass.

## Deploy

NO deploy from this slot. Live nerve still runs the unfixed binary until an
owner-approved nerve roll includes this branch.
