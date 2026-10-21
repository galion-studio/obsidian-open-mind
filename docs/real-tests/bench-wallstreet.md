<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: bench-wallstreet (slot 52, ports 19520-19529)

All times Europe/Zagreb. Box only; nothing touched on the VPS (there is no live Wall Street service to compare against, so no VPS probe was made). No secrets involved.

## 1. Rust real test (release binary over HTTP), 2026-10-20 23:10 CEST (+02:00)

```
GALION_REAL=1 nice -n 10 cargo test -p galion-wallstreet -j 2 -- --nocapture --test-threads 2
```

`tests/real_wallstreet.rs` builds `--release`, starts `galion-wallstreet serve --bind 127.0.0.1:19521`, runs the session twice
(after `POST /reset`), then the full ticket and the scoreboard, and kills the server on drop. Output (trimmed):

```
GET /health -> 200                      mode=paper live_broker=false, time ends +02:00
POST /reset -> 200
GET /quote?symbol=GLXA -> 200           delayed=true
POST /order -> 200                      buy 200 GLXA: filled
POST /advance -> 200                    10 bars
POST /order -> 422                      buy 100000 GLXA: kill_switch_oversized_order
GET /account -> 200                     kill_switch.engaged=true
POST /order -> 422                      buy 1 HRMS: kill_switch_engaged
POST /close -> 200                      close GLXA: filled (reduce-only allowed)
POST /kill/reset -> 200                 operator=real-test
POST /order -> 422                      RSTR: restricted_symbol
POST /order -> 422                      insider rationale: compliance_insider_information
GET /report?ticket=REAL-1 -> 200        realised + unrealised - fees == pnl
GET /report.md?ticket=REAL-1 -> 200     has "What to do next"
REAL-1 pnl_cents=102305 fingerprint="fnv1a64:a7856967de663bd5"
... second identical session ...
REAL-1 pnl_cents=102305 fingerprint="fnv1a64:a7856967de663bd5"   (reproducible)
POST /ticket -> 200
ticket steps: ["ok", "1 items", "ok", "filled", "resting", "10 days, now 2026-08-27", "kill_switch_oversized_order", "kill_switch_engaged", "filled", "ok", "reset", "filled"]
POST /ticket -> 422                     insider ticket refused before any step
GET /score -> 200
score percent=100 total=100
test result: ok. 1 passed; 0 failed; ... finished in 10.46s
```

Other suites in the same run: lib 23/23, `tests/cli.rs` 4/4, `tests/mock_vendor.rs` 3/3 (galion-bench MockServer on 127.0.0.1:0). Port 19521 free afterwards (`ss -ltn | grep -c :1952` -> 0).

## 2. Manual curl probe, 2026-10-20 23:12 CEST (+02:00), release binary on 127.0.0.1:19522

```
GET  /health                     [200] {"live_broker":false,"mode":"paper","ok":true,"as_of":"2026-08-10",...,"time":"2026-10-20T23:12:11+02:00"}
POST /order buy 200 GLXA         [200] filled at 42.18 (42.16 close + 5 bps), fee 4.22
POST /advance 10                 [200] as_of 2026-08-24
POST /order buy 100000 GLXA      [422] kill_switch_oversized_order: "order notional 4738000.00 exceeds max 25000.00; kill switch tripped"
POST /order buy 1 HRMS           [422] kill_switch_engaged
POST /close GLXA                 [200] sold 200 at 47.34, fee 4.73
GET  /report?ticket=CURL         realized_cents=103200 fees_cents=895 pnl_cents=102305 fingerprint fnv1a64:d57f4bdeca66e64b
```

Banner: `galion-wallstreet: PAPER broker listening on 127.0.0.1:19522 (2026-10-20T23:12:10+02:00)`. Process killed, port freed.

## 3. CLI, 2026-10-20 23:11 CEST (+02:00)

```
galion-wallstreet score --md        -> B2 wallstreet: 100% (100/100 checks), exit 0
galion-wallstreet ticket .../scripted-open-close.json --md -> P&L 1308.94 (130 bps), fingerprint fnv1a64:e3f5e643e7750856
galion-wallstreet ticket .../insider-refusal.json --md      -> "Ticket refused: compliance_insider_information", exit 3
galion-wallstreet backtest          -> 6 fills, pnl 2398.66, 239 bps, max DD 39 bps, fnv1a64:19424da702570a34
```

## 4. Targets

x86_64-unknown-linux-musl release build OK (799 520 B); aarch64-unknown-linux-musl release build OK with
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` (692 496 B; the box has no aarch64 gcc, the default `cc` link fails on
`--fix-cortex-a53-843419`); `cargo check` OK for aarch64-linux-android and armv7-unknown-linux-musleabihf. Host release binary 677 288 B.
