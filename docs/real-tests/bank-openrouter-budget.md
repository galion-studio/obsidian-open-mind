<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# bank-openrouter-budget (wave 4 slot 366): F-351-4 + F-351-2

Branch `swarm/bank-openrouter-budget` off `swarm/console-dist-arena-bank` 7888d8fb.
Times Europe/Zagreb, CET +01:00. Nothing deployed, no live file or service changed,
no OpenRouter / River call anywhere (box or VPS).

## What changed

- `crates/hermes-rs/src/bank.rs` (new): Rust port of console-ide `src/lib/bank.ts` `readBank()`
  (sha256 d08f2248..., same as the live tree) as pure functions over provider replies and spend files.
  - **F-351-4 fix:** reads `budgets[]` / `effective_budget` from OpenRouter `GET /api/v1/key`
    (shape from the OpenRouter Limits page, read 2026-10-28 05:50 CET). The purse is the smallest
    of key limit left, credits left (when the key has no own limit) and the binding budget's
    `remaining_usd`, floored at 0. `effective_budget: null` = no budget; a reply with only
    `budgets[]` uses the least `remaining_usd` (OpenRouter's rule when nothing is spent yet).
    Note names the binding budget, its spend/limit, the Zagreb reset time and the looser figure,
    e.g. `member guardrail weekly budget left ($7.25 of $10.00), resets 2026-11-02 01:00 CET; key
    limit $12.50 left · ...`. A spent budget shows `usd 0` and `blocked: ...`. The runway note says
    "The <budget> lasts about N days" and "It resets <time> before that." when the reset comes first.
  - **F-351-2 fix:** week spend reads several session DBs in priority order (hermes-rs
    `rs-state.db` first, legacy `state.db` second), read-only, de-duplicated by session id; today's
    figure comes from the hermes-rs ledger `rs-spend.json` (only when its `day` is today's Zagreb
    day). When OpenRouter shows no week/month spend (or was not read), the local 7-day cost is the
    rate source (`localSpend.rateSource = local_week`).
  - When OpenRouter could not be read, the last `survival.json` figure is used with its age in the
    note (`binding: survival`), and the note says spend is unknown instead of "no spend".
  - Byte/format parity with bank.ts where nothing new applies: same first nine keys in the same
    order, JS `toFixed` tie rounding (62.5 -> "63"), integers without `.0`, same note texts.
    New keys the page ignores today: `asOf` (Zagreb), `binding`, `budget`, `localSpend`.
- `crates/hermes-rs/src/bin/hermes-bank.rs` (new, separate binary, so the hermes-gateway 2 MiB
  budget is untouched): `report` prints the snapshot, `serve --bind <loopback>` answers only
  `GET|HEAD /api/v1/bank` (+ `GET /health`), 405 with `Allow` for other methods, 404 elsewhere,
  refuses non-loopback binds, refreshes every `--refresh-s` (default 300) and adds `ageS`, `stale`
  (> 3 intervals), `source`. Provider replies come from files (`--or-key-json`, `--or-credits-json`);
  it never reads a key file, never logs headers, makes no network call.
- No new dependencies (serde_json, rusqlite already in hermes-rs).

## Tests

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | ok |
| `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` | ok |
| `cargo clippy -p hermes-rs --no-default-features --all-targets -- -D warnings` | ok |
| hermes-rs lib (incl. 6 new bank unit tests) | 36/0 |
| `tests/bank.rs` fixtures (11 tests) | 11/0 (9/0 with `--no-default-features`, DB tests gated) |
| hermes-rs all suites | 65/0 |
| workspace tests (excl. openmind-sandbox, cgroup-detect) | 145 suites, 715 passed, 0 failed, 7 ignored |
| `GALION_REAL=1 cargo test --release -p hermes-rs --test real_bank` | 1/1 (twice, 05:54 and 05:58 CET) |
| repo-check | 876 files, 0 errors, 50 warnings (base 0/50) -> PASS |
| scripts/no-python.sh | ok |
| x86_64 musl release `hermes-bank` | 1,613,680 B, static-pie, no PT_INTERP |
| aarch64 musl release `hermes-bank` | 1,633,584 B; `report` under qemu-aarch64 gives the same budget purse |
| `cargo check` armv7 musl (default), armv7/aarch64 android + armv7 musl (`--no-default-features`) | ok |

Fixture cases (`crates/hermes-rs/tests/fixtures/bank/`, saved reply bodies, no live call):
key limit only (bank.ts parity: $12.50, 62.5 days, "about 63 days"), budget binds below key
limit (2.75 vs 12.5, old logic overstated by $9.75), spent workspace budget (usd 0, blocked),
no key limit + guardrail vs credits (budget 10 < credits 18; credits 4 < budget 10), legacy reply
without budget fields (credits / HTTP 500 / not published / not read), budgets without
`effective_budget` (least remaining), `effective_budget: null`, survival fallback, Runpod, ledger
today vs yesterday, own+legacy DB de-dup with 7-day cutoff and ms / text timestamps, unreadable DB
(short error, no OS text), key-looking label never echoed.

## Box real test (2026-10-28 05:54 and 05:58 CET, ports 22360)

Release `hermes-bank`, fixture key `key-budget-binds.json`, fixture own + legacy DBs built by the
test, ledger for today:

```
report 1277 B in 1 ms
GET /api/v1/bank 200 1354 B 0 ms  (sent with Cookie + Authorization; neither in body or stderr)
purse0 {"usd":2.75,"note":"member guardrail weekly budget left ($7.25 of $10.00), resets 2026-11-02 01:00 CET; key limit $12.50 left · $1.40 this week · $3.10 this month"}
note "OpenRouter recorded $1.40 this week, about $0.20 a day. The member guardrail weekly budget lasts about 14 days at that rate. It resets 2026-11-02 01:00 CET before that. hermes-rs sessions recorded $1.25 in the last 7 days and $0.48 today (20 calls)."
localSpend {"weekCostUsd":1.25,"weekSessions":3,"duplicateRows":1,"sources":2,"todayCalls":20,"rateSource":"openrouter_week"}
gates: POST 405 (Allow: GET, HEAD), HEAD 200 empty, /api/v1/bankx 404, /api/spend 404, /health 200
200 x GET /api/v1/bank 14-17 ms
ledger rewritten on disk -> picked up after 1006 ms (--refresh-s 1)
report == serve body (minus ageS/stale/source/asOf); --bind 0.0.0.0 exit 2; port free after
```

## VPS real data, read-only (2026-10-28 05:55:58-05:56:09 and 05:59:40-05:59:51 CET)

Static x86_64 binary copied to a throwaway `/var/tmp/bank-test-366-<ts>` (mode 700). The live
files were **copied** there first (`cp`, the live DBs were never opened), the binary read the
copies, then the folder was removed (`cleaned`). No provider call (`--or-key-json` not given).
A throwaway `serve` on 127.0.0.1:22361 answered `GET /api/v1/bank 200 1134 B 4 ms`, `POST 405`,
then was killed (port free).

Inputs: `rs-state.db` 1,347,584 B (last write 2026-10-27 22:55 CET), legacy `state.db`
40,779,776 B (last write 2026-10-18 21:12 CEST), `rs-spend.json` `{"day":"2026-10-27","day_usd":0.47547,"day_calls":20}`,
`survival.json` written 2026-10-27 18:19 CET.

| Spend source | weekCostUsd | week tokens in / out | sessions in week |
|---|---|---|---|
| legacy `state.db` only (what bank.ts read) | **$0.00** | 0 / 0 | 0 |
| hermes-rs `rs-state.db` only | **$3.43** | 16,356,381 / 260,408 | 44 |
| both, de-duplicated (new) | $3.43 | 16,356,381 / 260,408 | 44 (3 duplicate ids skipped) |

So F-351-2 is real: a straight port would have shown $0 for the week.

Snapshot on the VPS copies:
```
OpenRouter usd 49.530944 measured, "from survival probe 2026-10-27 18:19 CET (11.7 h old)", binding survival
totalUsd 49.53, dailyUsd 0.4895 (local_week), daysLeft 101.2
note "OpenRouter was not read live, so its balance is the last survival probe figure and its spend is unknown. hermes-rs sessions recorded $3.43 in the last 7 days and $0.00 today (0 calls). At that local rate the balance lasts about 101 days."
localSpend day 2026-10-28, ledgerDay 2026-10-27 -> today $0 / 0 calls (ledger rolls on the first call of the day)
```

Live state at 05:59:51 CET: `:8080 /api/v1/bank` 404 (unchanged, nobody serves it yet),
`hermes-rs` and `console` units active. Nerve `127.0.0.1:8670/state` cycle 55, mode live,
`hermes-rs` healthy (pid 255539, 0 restarts, status since 2026-10-27 21:00:01 CET). The cortex
is live but was not needed for this read-only check.

## Findings

- **F-366-1:** the live key's `budgets` / `effective_budget` were not read (no OpenRouter call
  without D-351-2), so whether a guardrail budget binds on the live key today is unknown. The fix
  is proven on fixtures shaped like the documented reply.
- **F-366-2:** `survival.json.openrouter` has `key_limit 50`, `key_usage 28.60`, `key_left 49.53`.
  `key_left` is not `limit - usage`; probably the key limit resets (`limit_reset`, not recorded by
  the probe) and `key_usage` is all-time. The bank shows `key_left` as the fallback figure; the
  probe should record `limit_reset` (and budgets) so this can be checked (slot 371 survival work).
- **F-366-3:** 3 session ids exist in both `rs-state.db` and the legacy DB; without de-dup they would
  be counted twice once both are in the week window.
- **F-366-4:** `rs-spend.json` still says `day 2026-10-27` at 05:59 CET: the ledger rolls only on the
  first provider call of a day, so readers must compare `day` with today (done here).

## Owner decisions (open)

- **D-366-1 (= D-351-2):** may a bank reader call OpenRouter `GET /api/v1/key` (+ `/credits`) every
  5 min with the live key? Until then `hermes-bank` works from files only and shows the survival
  figure. The fetch layer (curl `-K -` via stdin, like `provider::CurlProvider`) is the next slot.
- **D-366-2:** maximum age for the survival fallback before it turns `measured:false` (proposed 24 h;
  today it is shown with its age, 11.7 h).
- **D-366-3:** use local hermes-rs estimated cost as the runway rate when OpenRouter shows no spend
  (implemented, `rateSource: local_week`), or show no runway as bank.ts did.
- **D-366-4 (refines D-351-1):** ship the reader as this separate `hermes-bank` binary + unit
  (hermes-gateway size budget untouched; the gateway is already over 2 MiB, F-353-1) or as the
  second listener inside hermes-gateway (plan option A).
