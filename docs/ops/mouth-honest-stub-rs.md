<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Mouth honest stub (`galion-mouth-stub`) — slot 291

Prepared **2026-10-27 18:10 CET (+01:00)** on `swarm/mouth-honest-stub-rs`, base
`swarm/main-staging` `98466ab9`. Follow-up to slot 259
`docs/ops/mouth-honest-health.md` (publish blocker B7 / mouth 503).

**No live unit was changed. No VPS write. No deploy.**

## 1. Why a stub binary

Slot 259 documented the honest `mouth_health()` path inside `galion-one`
(503 `"unavailable"` + reason; never `"loading"` / `"chatterbox"`). Live VPS
`:8002` still answers the hard-coded loading lie because that binary is not
deployed.

This slot adds a **tiny** second binary in the same crate that already serves
`:8002`:

| Binary | Role |
|---|---|
| `galion-one` | Full One (8091 + mouth 8002 + optional keep-warm). Mouth `/health` now uses the shared stub body when no backend is configured. |
| `galion-mouth-stub` | Mouth **only**. GET `/health` + POST `/synthesize` → 503 `"voice not deployed"`. **No** keep-warm loop, **no** model load, **no** Hermes/mind ping. |

Use the stub when you want a truthful `:8002` without running the whole One
process (box tests, staging rehearsal, or a future split where the mouth
listener is not inside galion-one). Owner still has to OK any live swap.

## 2. Wire contract

```
GET  /health      -> 503 application/json
POST /synthesize  -> 503 application/json
other             -> 404 text/plain
```

Body gist (stable):

```json
{"ok":false,"status":"unavailable","provider":"stub","model":null,"loaded":false,"reason":"voice not deployed; set GALION_MOUTH to a galion-voice-serve address"}
```

Shared module: `crates/galion-one/src/mouth_stub.rs` (also used by
`mouth_health()` in `main.rs` when local speech is not ready).

Bind: `--bind ADDR`, else `GALION_MOUTH_STUB_BIND`, else `ONE_MOUTH_BIND`, else
`127.0.0.1:8002`.

## 3. Relation to option 1 / 1b / 2 (mouth-loading.md)

| Option | This branch |
|---|---|
| 1 (honest galion-one `/health`) | Strengthened: reason now says **"voice not deployed"** via `mouth_stub`. |
| 1b (nerve treats unavailable as absent) | Still owner-gated; not done here. |
| 2 (real `galion-voice-serve` on :8003) | Still owner-gated + SIMD ruling; stub does **not** load models. |

## 4. Deploy notes (owner OK required — not done)

1. Build: `cargo build --locked --release -p galion-one --bin galion-mouth-stub --target x86_64-unknown-linux-musl`
2. Box real test: `GALION_REAL=1 cargo test -p galion-one --release --test real_mouth_honest_stub`
3. Live swap of `:8002` alone is **not** recommended while `galion-one` still
   binds both 8091 and 8002 — would conflict. Prefer deploying honest
   `galion-one` (option 1) or pointing a future split unit at this stub.
4. Do **not** add `:8002` to the Cloudflare tunnel.

## 5. Ports

Slot 291 box tests: **21600–21609**.
