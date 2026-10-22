<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# hermes-gateway RSS growth (slot 244)

Follow-up to the slot 187 finding: hermes-gateway RSS grew about 9.5 MB/min
under load. Base: integration-5 0d68f46. Branch: `swarm/hermes-gateway-rss`.
All times Europe/Zagreb (CEST +02:00), 2026-10-22.

## Where the growth comes from

The growth is in our own Rust code: `MockProvider` in
`crates/hermes-rs/src/provider.rs`. It kept a full copy of every request
(about 10.2 kB: target, system prompt, messages, tool schemas) in a `Vec`
that never shrinks. Only `hermes-gateway --mock` and the unit tests use
`MockProvider`. The production path (`CurlProvider`, which the live
hermes-gateway uses) does not grow.

It is not a cache, a buffer or the allocator. A heap profile (a counting
`GlobalAlloc`) shows that clearing this one `Vec` frees all of the retained
growth.

Slot 198 found and fixed this on `swarm/hermes-mem-growth` c2457cd (based
on nerve-t33-backpressure, not on integration-5). integration-5 0d68f46
**did not have the fix.** The crates `hermes-rs` and `hermes-gateway` are
the same in 334beaa and 0d68f46, so this slot cherry-picked both commits
(b85c26e -> e09c1e7, c2457cd -> 2aeeeb8) without conflicts.

## Fix (already reviewed in slot 198, re-tested here)

- `MockProvider` request log is capped at `MOCK_CALL_LOG_CAP = 256`. When it
  is full, the oldest half is dropped. A `calls_total` counter keeps the
  full count.
- `hermes-gateway --mock` uses `MockProvider::echo().without_call_log()`,
  so it keeps no log at all.
- Production path: no change.

## Before and after, measured on the box (2026-10-22)

Real binaries, release builds, 30 chats/s for each gateway, loopback stub
upstream, no paid model.

| run | binary | soak | mock gateway | production-path (stub) gateway |
|---|---|---|---|---|
| before, 01:58-02:03 | 0d68f46 | 5 min | 4.0 -> 98.9 MB, **18,136 kB/min** | 4.2 -> 10.7 MB, 414 kB/min (still warming up) |
| after, 01:58-02:18 | 2aeeeb8 | 20 min | 4.0 -> 12.2 MB, **84 kB/min**, PASS | 4.2 -> 12.2 MB, 104 kB/min, PASS |

The heap profile (`HMG_HEAP=1`, 3,000 chats per case) shows:

| case | retained bytes per chat |
|---|---|
| old unbounded log | 10,210 B |
| cap 256 | 9.9 B |
| `--mock`, no log | 0.1 B |
| production-shaped provider | -0.1 B |

## Live VPS (read-only)

- `hermes-gateway-aa77864` (PID 718450) serves the live `:8642` endpoint
  without `--mock`. Its RSS was 1,824 kB at 01:58:51, 02:09:11 and 02:19:32,
  with a peak (VmHWM) of 1,904 kB, after 7 h 45 min to 8 h 06 min of
  uptime. `hermes-rs.service` NRestarts=0.
- The old `--mock` stray processes on 8652 and 8653 (slot 117) are gone.
  Nothing is listening on those ports.
- **Nothing to deploy.** The fix only affects `--mock` and tests.

## Side note (not hermes-gateway)

The nerve's `journalctl -f` child (PID 946277, parent `openmind-nerve-bb287cc`)
uses 43.3 to 43.7 MB RSS. It grew 336 kB in 10 min, which is typical of
journalctl mapping journal files. We did not investigate it further.

## Owner decisions

- D-244-1: merge `swarm/hermes-gateway-rss` (or slot 198's branch) into
  integration-5 / main. It changes only test and shadow code.
- The open items D-198-2 (the in-memory index of the JsonlStore, about
  485 B per session in no-sqlite builds, confirmed again here at 485 B) and
  D-198-3 (hermes-tools per-session counters, coding is off on live) are
  still open.
