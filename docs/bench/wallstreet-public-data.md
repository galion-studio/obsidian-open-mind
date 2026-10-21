<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# B2b Wall Street: REAL-PUBLIC replay tapes (ECB reference rates, U.S. Treasury par yields)

Slot 174 (`swarm/bench-ws-ecb-data`, base `swarm/bench-hermes` 457ef74), 2026-10-21 (+02:00). Follows the licence research of
slot 134 (`swarm/bench-data-licence` 493f175, copied here as [`market-data-licence.md`](market-data-licence.md)).
**PAPER ONLY**: no broker, no orders, no money, no API key, no network code in the crate.

## What was committed

`crates/galion-wallstreet/fixtures/real/` (the data is **not MIT**, see `NOTICE.md` there):

| File | Content | Size | Terms |
|---|---|---:|---|
| `ecb-eurofxref-hist.csv` | first 41 lines of the ECB `eurofxref-hist.csv` (header + 40 business days 2026-08-26 .. 2026-10-20, all 41 columns, newest first), **byte-for-byte** | 10 875 B | **Source: ECB statistics.** Free reuse if the source is quoted and the statistics are not modified: https://www.ecb.europa.eu/stats/ecb_statistics/governance_and_quality_framework/html/usage_policy.en.html |
| `ust-par-yield-curve.csv` | first 41 lines of the Treasury 2026 Daily Par Yield Curve CSV (40 business days 2026-08-24 .. 2026-10-20), byte-for-byte | 3 351 B | U.S. Government work, public domain (17 U.S.C. § 105); credit Treasury, no endorsement: https://home.treasury.gov/policy-issues/financing-the-government/interest-rate-statistics · https://resources.data.gov/open-licenses/ · https://www.treasurydirect.gov/legal-information/terms/ |
| `NOTICE.md` | attribution lines, terms URLs, the "never edit" rule | | MIT (ours) |
| `source.json` | URLs, fetch time 2026-10-21 09:50:38 +02:00, sha256 of each download and slice, fnv1a64 of each slice (checked by a test) | | MIT (ours) |
| `*.license` | REUSE-style sidecars: `LicenseRef-ECB-statistics-reuse`, `LicenseRef-US-Government-Public-Domain`, MIT for source.json | | |

SOFR (NY Fed) and Fed H.10 were not committed: the row asked for ECB + Treasury, and both cover FX and rates. They can be added
the same way later (SOFR needs its fixed notice).

## How the data becomes paper-tradable bars (`src/public.rs`)

- **ECB (unmodified rule).** Each pair is one daily fixing, so open = high = low = close. The price is the rate times a lot
  of EUR chosen so the published value is an exact number of cents: lot = 10^(max decimals - 2). EURUSD, EURCHF: 100 EUR (4 dp);
  EURGBP: 1000 EUR (5 dp); EURJPY: 1 EUR (2 dp). `ecb_price_cents` **errors instead of rounding**, and
  `verify_ecb_unmodified` maps all 160 tape prices back to the exact published strings (a one-pip edit is caught by a test).
- **Treasury (derived, labelled).** `UST2Y`, `UST5Y`, `UST10Y`, `UST30Y` are a zero-coupon price proxy per 100 face,
  `100 / (1 + y/200)^(2n)`, in integer fixed point (1e18, round half up), so x86_64 and aarch64 give identical numbers
  (checked: same fingerprints under qemu-aarch64). This is a Galion computation on public-domain data, not a Treasury statistic,
  and every source line says so.
- Labels: every tape source line starts with `REAL-PUBLIC`, `/health` and `/data` return `data_class`, `attribution`, and the
  terms URLs; `GET /data/notice` and `galion-wallstreet data --notice` print NOTICE.md. The synthetic 100/100 scoreboard is
  untouched (same reference fingerprint `fnv1a64:19424da702570a34`, score 100/100).
- Calendars are real: the ECB fixes on US Labor Day (2026-09-07) and Columbus Day (2026-10-12), the Treasury market does not.
  The tapes are kept separate; a naive merge is refused by `ingest` ("calendar not aligned"), which is now tested on real data.
- Account currency: the HTTP paper account is USD, so on the `ecb-fx` tape only `EURUSD` is tradable; `EURGBP`, `EURCHF`,
  `EURJPY` are listed as `restricted` with the reason. The FX backtest replays each pair in its **own** quote-currency account,
  sized to ~10 000.00 of that currency on day one.
- Symbol rule widened: tickers may now contain digits after an uppercase letter (`UST10Y`). Lowercase is still refused.

## Use

```
galion-wallstreet serve --bind 127.0.0.1:19520 --tape ecb-fx      # or ust, synthetic (default); env WALLSTREET_TAPE
galion-wallstreet backtest --data ecb-fx                           # 4 per-pair backtests (JSON)
galion-wallstreet backtest --data ust
galion-wallstreet data ust                                         # tape summary, attribution, terms URLs
galion-wallstreet data --notice
GET /data  GET /data?tape=ust  GET /data/notice  GET /backtest?data=ecb-fx  (POST /reset keeps the tape)
```

## Pinned replay results (integer cents, reproducible)

| Tape | Run | Fills | P&L | Fingerprint |
|---|---|---:|---:|---|
| ecb-fx | hand replay: buy 100 x EURUSD (100 EUR lots) on 2026-08-26 at 116.69 (+5 bps = 116.75), hold, close 2026-10-20 at 112.15 (-5 bps = 112.09), 5 bps fee each side | 2 | **-477.44 USD** | (report over HTTP: `fnv1a64:537fa48f279334b0`) |
| ecb-fx | `sma_cross(3/8)` EURUSD, USD account, 85 lots | 4 | -132.45 USD | `fnv1a64:0ea17fa4f45eaaed` |
| ecb-fx | `sma_cross(3/8)` EURGBP, GBP account, 11 lots of 1000 EUR | 4 | -53.76 GBP | `fnv1a64:050a56d418684519` |
| ecb-fx | `sma_cross(3/8)` EURCHF, CHF account | 4 | -249.93 CHF | `fnv1a64:11bd49f1b903e0d3` |
| ecb-fx | `sma_cross(3/8)` EURJPY, JPY account | 4 | -76.65 JPY | `fnv1a64:19a03e0446959321` |
| ust | `sma_cross(3/8, 100)` on UST2Y/5Y/10Y/30Y, USD account | 14 | -64.08 USD | `fnv1a64:c55c83952c5c599d` |

Honest read: on this 40-day window the euro fell against USD/CHF/JPY/GBP and long yields rose (10 Yr 4.70 -> 5.12), so a
long-only SMA crossover loses a little everywhere. That is the point of a real tape: the synthetic walk was kinder (+2398.66).
These numbers are a replay baseline for Hermes, not a score.

## Gaps

- Rates/FX only; equities stay synthetic (IEX HIST needs owner decision D-B2-1b).
- Daily fixings, no intraday range: limit orders on these tapes behave like close-only fills (high = low = close).
- The FX backtest per pair uses a "100 000.00" account in that pair's currency; P&L is not converted between currencies.
- Re-recording is manual (curl GET + `head -41`, documented in `docs/real-tests/bench-ws-ecb-data.md`); no `record`
  subcommand, to keep the crate free of network code.
- Hermes is not yet pointed at the real tapes (hermes-bench still drives the synthetic broker).

## Owner decisions (not decided here)

- **D-B2-1a** (slot 134): this branch implements (b1) for ECB + Treasury as the row asked. Please confirm committing this public
  data slice to the repo is OK. Rollback: revert the commit, nothing else depends on it.
- **D-B2-1c**: OK that `fixtures/real/*.csv` are *not MIT* inside the MIT repo, marked by NOTICE.md + `.license` sidecars
  (`LicenseRef-ECB-statistics-reuse`, `LicenseRef-US-Government-Public-Domain`)?
- **D-B2b-1 (new)**: should a real-tape axis enter the B2 scoreboard (for example "replay reproducible + attribution present +
  unmodified check"), or stay a separate baseline as now? Recommended: keep it separate until Hermes runs on it.
- **D-B2b-2 (new)**: add NY Fed SOFR (needs the fixed notice text) and Fed H.10 (second USD/EUR source, lets Hermes practise
  "two sources disagree": 2026-10-16 ECB 1.1253 vs H.10 1.1231)?
