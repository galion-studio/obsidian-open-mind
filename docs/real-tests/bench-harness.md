<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: bench-harness (`galion-bench-mock`)

Run 2026-10-20 23:03 CEST (+02:00) on the swarm box, slot 50, ports 19500-19509 (all free at start; test used 19500).

```text
GALION_REAL=1 nice -n 10 cargo test -j 2 --release -p galion-bench --test real_mock -- --nocapture --test-threads=1
```

The Rust test spawns the **release** binary, waits for its banner, hits it over real TCP, kills it and checks the port is free again.

```text
test real_binary_check_mode ... check: ok fixtures=13 at=2026-10-20T23:03:19+02:00
bad check: galion-bench-mock: fixture line 1: /tmp/galion-bench-bad-2462643.jsonl: timestamp `2026-10-20T21:00:00Z` ends in Z; write Europe/Zagreb with +01:00/+02:00
ok
test real_binary_serves_fixtures ... listening http://127.0.0.1:19500 fixtures=13 at=2026-10-20T23:03:19+02:00
GET /api/1.0/accounts -> 200 [{"id":"acc-mock-eur","name":"Main","balance":1234.56,"currency":"EUR","state":"active","public":false,"created_at":"2026-10-01T09:00:00+02:00","updated_at":"2026-10-20T23:03:19+02:00"}]
GET /api/1.0/rate x2 -> 503, 200
POST /api/1.0/pay -> 403 {"code":403,"message":"mock: money moves are forbidden in benchmarks; use a payment draft"}
POST /supplier/orders x2 -> 201 {"order_id":"po-1","status":"accepted","simulated":true,"at":"2026-10-20T23:03:19+02:00"} / {"order_id":"po-2",...}
GET /quotes/ACME -> 200 {"symbol":"ACME","last":101.25,"delayed_minutes":15,"as_of":"2026-10-20T16:00:00+02:00"}
journal: 16 hits, journal at "2026-10-20T23:03:19+02:00"
torn down; port 19500 free
ok
test result: ok. 2 passed; 0 failed
```

After the run: `ss -ltn` showed no listener on 19500-19509.

Binaries: release x86_64-gnu 455,728 B; x86_64-unknown-linux-musl 578,336 B (static-pie); aarch64-unknown-linux-musl built (static) with `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` (the box's default `cc` rejects `--fix-cortex-a53-843419`, a box toolchain issue, not this crate). `cargo check` passes for aarch64-linux-android and armv7-unknown-linux-musleabihf.

VPS: no live service matches a fixture mock, so no VPS probe was made. Nothing touched Revolut or any bank/broker.
