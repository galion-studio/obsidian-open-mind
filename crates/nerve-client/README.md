<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# nerve-client

Optional, fire-and-forget push of signals to the nerve (`openmind-nerve`,
live unit `galion-nerve`, `POST /signal`). std only, no dependencies,
loopback `http://` only.

- `NERVE_URL` unset (default): every call is a no-op (one atomic load).
- `NERVE_URL=http://127.0.0.1:8670`: signals go into a **bounded queue**
  (`NERVE_QUEUE`, default 256, clamped 8..4096) drained by ONE worker
  thread (300 ms connect/read/write timeouts). `send` never blocks: a full
  queue drops the signal and counts it. After a failed POST the worker
  skips connecting for 2 s (a dead nerve costs one refused connect per 2 s,
  and the queue drains instead of filling up).
- Counters (`nerve_client::stats()`): `queued`, `sent`, `dropped` (queue
  full), `failed` (nerve down/hung). The heartbeat carries them to the nerve
  (`msg: "queued=.. sent=.. dropped=.. failed=.."` + metric
  `nerve_client_dropped`).
- `NERVE_SLOW_MS` (default 1000): `observe_request` reports slower requests.
- `NERVE_HEARTBEAT_S` overrides the heartbeat period (min 10 s).

```rust
nerve_client::event("hermes-rs", "started", "listening on 127.0.0.1:8642");
nerve_client::observe_request("hermes-rs", "POST /v1/chat/completions", 502, elapsed); // 5xx + slow
nerve_client::cap_hit("hermes-rs", "spend_cap", "daily spend cap reached"); // max 1 push / 5 s
nerve_client::metric("hermes-rs", "queue_depth", 3.0);
nerve_client::start_heartbeat("hermes-rs", 60); // VmRSS in KiB + queue counters
```

Wired into `hermes-rs` (`api::spawn`: started, heartbeat, `http_5xx`,
`slow_request`; `http::serve`: `conn_cap`; `spend::SpendTracker::check`:
`spend_cap`) and `openmind-memory` (`main`: started, heartbeat, `http_5xx`,
`slow_request`; `http::serve`: `conn_cap`). Ported from ws/nerve a63b730
(which used one thread per signal, max 4 in flight) on swarm/nerve-wiring.
