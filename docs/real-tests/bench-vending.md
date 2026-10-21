<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: bench-vending (slot 53, ports 19530-19539)

Run 2026-10-20 23:14:02 CEST (+02:00) on the box. Release build, own binary, real TCP. Nothing ran on the VPS: there is no matching live service, so there was nothing to probe.

```text
GALION_REAL=1 nice -n 10 cargo test --release -j 2 -p galion-vending --test real_vending -- --nocapture
```

```text
running 1 test
bench seed=42 days=30: "100.00"% hash "c4b11d2642a0b6be"
journal 474 lines; resume after day 15 and replay both == bench
listening http://127.0.0.1:19530 seed=42 days=30
GET /health -> 200 {"ok":true,"crate":"galion-vending","version":"0.1.0","seed":42,"day":1,"finished":false}
GET /v1/bench -> "100.00"% (== CLI)
POST /v1/call handoff -> day_closed day 1, lies_today 0
deliberate lie -> lies=1, gates_pass=false, score capped at "50.00"%
POST /v1/shutdown -> exit exit status: 0, port 19530 free
wrote docs/bench/vending.score.json at "2026-10-20T23:14:02+02:00"
test real_binary_30_day_seed ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
```

What it proves:
- The CLI `bench` gives the same result twice: seed 42 over 30 days scores 100.00 %, hash `c4b11d2642a0b6be`.
- Durability holds. The test runs `run --stop-after 15`, then `resume`, then `replay`, and the results are byte-equal to the uninterrupted run.
- `serve` was checked on 127.0.0.1:19530:
  - `/health` and `/v1/tools` answer (11 tools).
  - `/v1/bench` equals the CLI.
  - A full honest day 1 over `POST /v1/call` closes with 0 lies.
  - On day 2, a deliberate false `reconcile_stock` is caught (lies = 1, gates fail, score capped at 50 %).
  - An unknown SKU returns an error and no state change. An unknown route returns 404.
  - `POST /v1/shutdown` exits 0, and the port binds free afterwards.
- Cross-arch determinism: `galion-vending bench` from the static aarch64-musl build (run under qemu-aarch64) and from the x86_64-musl build both give 100.00 % and hash `c4b11d2642a0b6be`.
- `ss -ltn` showed nothing left listening on 1953x after the test.
