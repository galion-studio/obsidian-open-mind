<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: `swarm/mouth-honest-stub-rs` (slot 291, ports 21600-21609)

Times Europe/Zagreb. Box only; no VPS write; no deploy.

## 1. What was run

```
export CARGO_TARGET_DIR=/workspace/swarm/target/mouth-honest-stub-rs
export PATH=/workspace/swarm/bin:$PATH
nice -n 10 cargo fmt -p galion-one -- --check
nice -n 10 cargo clippy -p galion-one --all-targets -j 4 -- -D warnings
nice -n 10 cargo test -p galion-one -j 4
GALION_REAL=1 nice -n 10 cargo test -p galion-one --release -j 4 --test real_mouth_honest_stub -- --nocapture
GALION_REAL=1 nice -n 10 cargo test -p galion-one --release -j 4 --test real_mouth_loading -- --nocapture
```

Recorded: **2026-10-27 18:13:34 CET (+0100)**

## 2. `real_mouth_honest_stub` (GALION_REAL=1, release) — PASS 4.11 s

| Case | Result |
|---|---|
| A | GET 127.0.0.1:21600/health → **503** `unavailable` + `voice not deployed`; constant over 4 s; no loading/chatterbox |
| B | POST /synthesize → **503** same reason, no RIFF, **0 ms** |
| C | GET /nope → **404** |
| D | `GALION_MOUTH_STUB_BIND=127.0.0.1:21601` → same honest 503 |

Body (verbatim):
```
{"ok":false,"status":"unavailable","provider":"stub","model":null,"loaded":false,"reason":"voice not deployed; set GALION_MOUTH to a galion-voice-serve address"}
```

## 3. Regression: `real_mouth_loading` (GALION_REAL=1) — PASS 7.98 s

A/B/C on galion-one still honest; reason now includes `voice not deployed` (still contains `set GALION_MOUTH`). Live-binary cases skipped (no GALION_ONE_LIVE_BIN).

## 4. Unit tests

`mouth_stub` 4/4 + galion-one unit 82/82 (debug). Clippy `-D warnings` clean. fmt clean.

## 5. Live VPS (read-only, optional)

Not required for this stub slot. Live `:8002` still expected 503 loading/chatterbox until option-1 galion-one deploy. This branch is **not** live. No tunnel / DNS / unit changes.
