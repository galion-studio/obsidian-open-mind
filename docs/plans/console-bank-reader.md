<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Console `/api/v1/bank` reader: plan + staged design (wave 4 slot 351, D-336-2)

Branch `swarm/console-bank-reader-plan` off `swarm/console-dist-arena-bank` 7888d8fb.
Plan and staged config only. No product code changed, nothing deployed, no VPS writes.
The owner decision (D-336-2, refined here as D-351-1..4) stays open.
Times: Europe/Zagreb CET +01:00.

## 1. In short

- Today nobody serves `/api/v1/bank`: live `:8080` and `:18080` both answer `404` (04:06 CET), so
  `/bank` shows its honest empty state. The Node `readBank()` that used to fill it is retired.
- **Recommendation:** a small bank reader inside **hermes-rs**, on its **own loopback listener
  `127.0.0.1:8649`** that answers only `GET/HEAD /api/v1/bank` from a **cached snapshot** refreshed
  in the background. The console points `--data-upstream 127.0.0.1:8649 --data-path /api/v1/bank` at it
  and stays key-free. No new dependencies (hermes-rs already has rusqlite and a curl transport that
  sends keys via stdin, never argv).
- Two things must be fixed first (findings below): the console forwards `Cookie` and
  `Authorization` to the data reader (F-351-1), and the old spend source is frozen since
  2026-10-18 (F-351-2). Live spend now sits in hermes-rs's own files.

## 2. Where the bank data lives today (VPS, read-only, 2026-10-28 04:05-04:09 CET)

Metadata and JSON shapes only. No secret file was opened; key files were only `stat`-ed.

| Data | Path | Owner/mode | Last write (CET) | Notes |
|---|---|---|---|---|
| OpenRouter key (Node list #1/#2) | `/root/galion-secrets/hermes.env`, `/opt/galion/galion-light/secrets/persist/hermes.env` | root 0600 | 2026-10-13 14:12 | same size/mtime (copies) |
| OpenRouter key (#3) | `/opt/galion/galion-light/hermes-home/.env` | root 0600 | 2026-10-13 14:12 | |
| OpenRouter + River keys (#4, River list) | `/opt/galion/hermes/.env` = `/workspace/hermes/.env` | root 0600 | 2026-10-18 20:56 | **already the `EnvironmentFile` of `hermes-rs.service`** |
| Legacy spend (Node `weekSpend`) | `/opt/galion/hermes/state.db` (40.8 MB) | root 0600 | 2026-10-18 21:12 | **frozen**: Python Hermes `hermes.service` inactive; hermes-rs opens it read-only as `HERMES_RS_LEGACY_STATE_DB` |
| Live session spend | `/var/lib/hermes-rs/rs-state.db` (1.3 MB, same `sessions` schema) | root 0600 (dir 0700) | 2026-10-27 22:55 | `HERMES_RS_STATE_DB` |
| Live spend ledger | `/var/lib/hermes-rs/rs-spend.json` | root 0600 | 2026-10-27 22:55 | `day 2026-10-27, day_usd 0.4755, day_calls 20, 58 sessions, sum $2.9606`; caps session $5 / day $7 / 2000 calls |
| Survival probe | `/workspace/galion-light/state/survival.json` | root 0644 | 2026-10-27 18:19 | keys `cpu disk gpu lane openrouter revolut rules runpod when`; `openrouter{measured:true, cash_usd, key_left, key_limit, key_usage, total_credits, total_usage, note}`; `runpod{measured:false, reason:"no API key on this host"}`; `revolut{measured:false, reason}`. **One-shot**: no timer/unit writes it (10 h old at probe) |

Processes (04:05 CET): `hermes-rs.service` active, `hermes-gateway-b7fbbc78` on `127.0.0.1:8642`
(user root, `HERMES_RS_HOST=127.0.0.1`), `console.service` + `console-rust-primary.service`
active (`galion-console-serve-2d846fb8 --dist ... --ui rust --local-api`, no `--data-upstream`;
that build predates it). Port `127.0.0.1:8649` is free (loopback 86xx in use: 8642, 8646, 8650,
8660, 8670, 8671, 8674). No `sqlite3` CLI on the VPS (so week totals were not computed from the DBs).

Live answers:
```
:8080  GET /api/v1/bank  -> 404 "Not Found"
:18080 GET /api/v1/bank  -> 404 "Not Found"
:8642  GET /health       -> 200 {"implementation":"hermes-rs","implementation_version":"0.1.0",...}
:8642  GET /api/spend    -> 401 gateway_auth_failed (API_SERVER_KEY)
:8642  GET /api/v1/bank  -> 401 gateway_auth_failed
```
So pointing the key-free console straight at `:8642` cannot work: every non-health route needs
the gateway key, and giving the console that key would undo "console stays key-free".

Nerve (`127.0.0.1:8670/state`, 04:00:01 CET, cycle 48, mode live): `hermes-rs` healthy
(`hermes-rs.service`), `console` healthy (`console.service`). The cortex is live but not needed here.

## 3. Options for D-336-2 (who serves `/api/v1/bank`)

| | Option | Keys live in | Effort | Risk |
|---|---|---|---|---|
| **A (rec.)** | hermes-rs, **second loopback listener** `127.0.0.1:8649`, GET `/api/v1/bank` only, no gateway key, cached snapshot | hermes-rs (already loads `/opt/galion/hermes/.env`) | ~300 lines Rust + tests, no deps | small: read-only, loopback, one route |
| B | hermes-rs, route on `:8642` exempt from the key check (like `/health`) | hermes-rs | smaller | widens the auth-exempt surface of the agent API; any local process reads balances without the key |
| C | hermes-rs route on `:8642` with key; console gets the key | console + hermes-rs | small | console no longer key-free (rejected by D-336-2's own premise) |
| D | standalone `galion-bank` binary + unit | new unit reads key file | new crate + unit + rollout | another root service; duplicates hermes-rs's curl/sqlite code |
| E | do nothing: `/bank` keeps the empty state | - | 0 | no runway figure on the console |

Why A: hermes-rs already has the keys, the spend DBs (own + legacy) and the HTTPS transport; a
separate listener keeps the agent API's key check untouched and gives the reader its own
connection cap. The console needs no code change for the happy path (verified below).

## 4. Staged design (option A)

Files: `deploy/staged/console-bank-reader/hermes-bank.conf` (hermes-rs drop-in, design env vars),
`console-bank.conf` (console drop-in), `bank-snap.example.json` (the wire contract).

**Listener.** `HERMES_RS_BANK_BIND` (must be loopback, refuse otherwise; unset = off). Reuse
`hermes_rs::http::serve(listener, handler, max_conns=4)`. Routes: `GET|HEAD /api/v1/bank` -> 200
snapshot; `GET /health` -> 200; any other method -> 405 `Allow: GET, HEAD`; other paths -> 404.
Ignores and never logs `Cookie` / `Authorization` (see F-351-1).

**Refresh, not per request.** The console-ui call budget is 4 s (`console_ui::client::CALL_BUDGET`)
and the Node reader did up to three sequential 15 s provider calls. So a background thread
refreshes every `HERMES_RS_BANK_REFRESH_S` (default 300, min 60) and the handler only clones the
last snapshot (`Arc<Mutex<Option<Snap>>>`). Before the first refresh: 503 `{"ok":false,"warming":true}`
(the page then shows "HTTP 503 ..."). One refresh at a time; a failed provider call keeps the purse
as `measured:false, note:"request failed"` and never blocks the snapshot.

**Inputs (same rules as `bank.ts`, with three fixes):**
1. OpenRouter: `GET https://openrouter.ai/api/v1/key` with the `provider::CurlProvider` pattern (a small GET variant: system curl, `-K -` config on stdin)
   (key through stdin config). `usd = data.limit_remaining`; **new:** if `data.effective_budget`
   is present, `usd = min(limit_remaining, effective_budget.remaining_usd)` and the note says which
   budget binds (OpenRouter docs, Limits page, fetched 2026-10-28 04:06 CET, now return `budgets`
   / `effective_budget`; the Node code ignored them). `limit_remaining == null` -> `/api/v1/credits`
   fallback (`total_credits - total_usage`) as before. Weekly/monthly from `usage_weekly` /
   `usage_monthly`.
2. River: `GET https://api.river.ai/v1/models` -> "key live, balance not published by the API"
   (unchanged; River has no balance endpoint).
3. Revolut: fixed `measured:false, "not connected"` (Revolut ban lifted for design only; no live call).
4. Runpod: from `survival.json` `.runpod` (same `moneyUnder` walk). **New:** if OpenRouter could
   not be read live but `survival.json.openrouter.measured` is true, use `key_left` with note
   "from survival probe <when>", so a provider outage still shows the last known figure.
5. Week spend (**fix F-351-2**): sum `input_tokens`, `output_tokens`, `estimated_cost_usd` for
   sessions with `last_activity_at` in the last 7 days over `rs-state.db` first, then the legacy DB,
   de-duplicated by session `id` (hermes-rs's `Layered` store order). Read-only
   `SQLITE_OPEN_READ_ONLY`, busy timeout 2 s.
6. `dailyUsd`, `daysLeft`, `riverIfMovedDailyUsd` (0.12 / 0.48 per M tokens) and `note`: same
   formulas as `bank.ts` so the Rust page renders identically.

**Wire contract.** Exactly the `BankSnap` keys the console-ui page reads (`purses[] {name, usd,
measured, note}`, `totalUsd, weekTokensIn, weekTokensOut, weekCostUsd, dailyUsd, daysLeft,
riverIfMovedDailyUsd, note`) plus optional `asOf` (Zagreb ISO with offset), `ageS`, `stale`,
`source`. Verified: the current page ignores the extra keys, so freshness must also be put into
`note` ("... as of 04:00 CET") until the page shows `asOf` (small console-ui follow-up). Notes go
through the page's `clean()` scrubber anyway (drops `sk-/or-...` tokens, `Bearer ...`, `api_key=`).

**Never:** return or log a key, a key file path, OS error text, or the raw provider body; write
to any file; call a paid endpoint (both OpenRouter endpoints are free account reads).

**Size.** hermes-gateway must stay under the 2 MiB budget (slot 329). The reader reuses curl,
serde_json and rusqlite already linked; expected growth is a few KiB. Measure in the build slot.

**Console side.** `--data-upstream 127.0.0.1:8649 --data-path /api/v1/bank` (staged drop-in).
The live console binary (2d846fb8) does not have these flags yet; they ship with the next console
deploy (7888d8fb line). Fold F-351-1 first.

**Tests for the build slot (not written here):** unit tests for the `bank.ts` parity cases
(missing key, HTTP 401, `limit_remaining` null -> credits, effective_budget lower than limit, week
cutoff, legacy+own de-dup); a `GALION_REAL=1` test that starts hermes-gateway with
`HERMES_RS_BANK_PROVIDERS=0` and fixture DBs, then the console with `--data-upstream`, and checks
`/bank` renders the numbers; a VPS read-only check after deploy (`curl 127.0.0.1:8649/api/v1/bank`
-> 200, `:8080/bank` shows the purses, nerve still healthy).

## 5. Box real test of the staged wiring (2026-10-28 04:07-04:09 CET)

Release `galion-console-static` from 7888d8fb (this branch, unchanged), a scratch std-only Rust
fake reader (not committed) that logs header names and masks Cookie/Authorization values to a
length, ports 22210-22215.

```
console --dist /tmp/s351dist --ui rust --bind 127.0.0.1:22212 --data-upstream 127.0.0.1:22213 --data-path /api/v1/bank
fixture GET /api/v1/bank 200 754B          body byte-identical to bank-snap.example.json
fixture GET /bank        200 15434B 1 ms   $12.50, Revolut, Runpod, "no API key on this host", 62.5 days,
                                           19391995 / 1830315 tokens, $0.46 River estimate, note rendered;
                                           asOf/source not rendered (ignored), 0 <script>
200 x GET /api/v1/bank via proxy: 1259 ms total (~6.3 ms each incl. curl start)
reader down (nothing on :22215): GET /api/v1/bank 502 "Bad Gateway"; /bank 200 14934B
  "HTTP 502. The console reads no key files and calls no provider, so balances show once a Rust
   bank reader serves /api/v1/bank (owner decision)."
gate (bind 22210): POST /api/v1/bank 405, Host: evil.example 401, /api/v1/bankx 404,
  /api/arena (not in --data-path) 404 - none reached the reader
```

Header capture at the reader (F-351-1):
```
plain:            Host, User-Agent, Accept, X-Forwarded-For=127.0.0.1, X-Forwarded-Proto, X-Forwarded-Host, Connection
with Cookie:      ... Cookie=<34 bytes> ...                      <- console cookie forwarded
with Authorization + X-Forwarded-For:6.6.6.6:
                  ... Authorization=<11 bytes> ... X-Forwarded-For=6.6.6.6, 127.0.0.1
in-process /bank: User-Agent=galion-console-ui, no Cookie, X-Forwarded-For=127.0.0.1, 127.0.0.1
```

## 6. Findings

- **F-351-1 (fix before wiring any reader):** `--data-upstream` forwards the client's `Cookie` and
  `Authorization` headers unchanged (`proxy::upstream_head`), so a paired phone's admin cookie
  would reach hermes-rs on every direct `GET /api/v1/bank`. The reader needs neither. Fix in
  console-static: strip `Cookie`, `Authorization`, `Proxy-Authorization` for data-upstream
  forwards (the generic `--upstream` proxy may still need them). Small, own slot, with a real test
  that asserts the reader sees none.
- **F-351-2:** the Node reader's spend source `/opt/galion/hermes/state.db` has not been written
  since 2026-10-18 21:12 CET; a straight port would report `$0` week spend forever. Live spend is in
  `/var/lib/hermes-rs/rs-state.db` + `rs-spend.json` (design step 5).
- **F-351-3:** `survival.json` is a one-shot file (written 2026-10-27 18:19 CET, no timer). Fine as
  a fallback, wrong as the main source.
- **F-351-4:** OpenRouter `/api/v1/key` now has `budgets` / `effective_budget`; `limit_remaining`
  alone can overstate what is spendable when a guardrail budget binds first (design step 1).
- **F-351-5:** the page ignores `asOf`/`stale`; put freshness in `note` until console-ui renders it.
- Keys exist in up to 5 env files (two pairs are copies). The reader should read only what
  hermes-rs's unit already loads (`/opt/galion/hermes/.env` via `EnvironmentFile`), not walk the
  Node list. Key rotation/consolidation stays with the off-host key work (slot 340).

## 7. Owner decisions (open)

- **D-351-1 (= D-336-2):** who serves `/api/v1/bank`: A hermes-rs second loopback listener (rec.),
  B auth-exempt route on :8642, D separate service, E none.
- **D-351-2:** may the reader call OpenRouter `/api/v1/key` (+ `/credits`) and River `/v1/models`
  every 5 min? Free, read-only account calls, but they are outbound calls with the live keys.
  Alternative: `HERMES_RS_BANK_PROVIDERS=0` (local files only: survival.json + spend DBs).
- **D-351-3:** refresh interval (300 s proposed) and whether a stale snapshot (> 3 intervals) is
  shown with a warning or hidden.
- **D-351-4:** order: F-351-1 console fix -> hermes-rs reader build slot -> deploy both together
  with the next console deploy (rec.), or reader first behind the flag (port closed until set).

## 8. Next slots (proposed)

1. `console-data-upstream-strip` (console-static): F-351-1 + real test, base 7888d8fb.
2. `hermes-bank-reader` (hermes-rs): section 4, `HERMES_RS_BANK_*`, unit + GALION_REAL tests,
   size check, `PROVIDERS=0` in tests (no live provider calls from the box).
3. After D-351-1/2: install the two drop-ins with the next console deploy; VPS read-only verify.
