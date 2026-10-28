<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life probes: bench-data-licence (slot 134)

Doc-only research slot, so there is no binary or port (assigned 20040-20049, none used). Probes are plain HTTPS GETs from the box
(no key, no account, nothing kept in the repo) plus a read-only look at the live nerve on the VPS. Times are Europe/Zagreb (+02:00).

## Source reachability (box, 2026-10-21 05:10:11 +02:00)

| Probe | HTTP | Bytes | Shape check |
|---|---|---|---|
| `GET https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist.zip` | 200 | 641 518 | CSV of 7 119 lines; header `Date,USD,JPY,...`; newest row 2026-10-20 USD 1.1215 |
| `GET https://home.treasury.gov/resource-center/data-chart-center/interest-rates/daily-treasury-rates.csv/2026/all?type=daily_treasury_yield_curve...` | 200 | 16 391 | 201 lines; 14 tenors 1 Mo..30 Yr; 2026-10-20 10 Yr = 5.12 |
| `GET https://markets.newyorkfed.org/api/rates/secured/sofr/last/40.json` | 200 | 9 538 | 40 rows, 2026-08-21..2026-10-19; 2026-10-19 SOFR 3.87, volume 2888 bn |
| `GET https://www.federalreserve.gov/releases/h10/hist/dat00_eu.htm` | 200 | 1 131 354 | "Historical Rates for the EU Euro", 3-JAN-00 .. 16-OCT-26 (1.1231) |
| `GET https://iextrading.com/api/1.0/hist` | 200 | 1 393 160 | 2 492 dates; newest 20261019 TOPS 1.6, 11 197 958 400 bytes |
| Range `bytes=0-4095` on the 20261019 TOPS link | 206 | 4 096 | gzip `1f 8b 08`; inflates to pcapng `0a 0d 0d 0a` |
| `GET https://iextrading.com/api/1.0/hist?date=20261020` | 404 | 9 | (old per-date query form is gone; use the index) |

Slice sizes measured from those files (first 41 lines = header + 40 business days):
ECB 4 currencies (USD, JPY, GBP, CHF) 1 594 B; Treasury curve 3 351 B; SOFR 40 rows 9 538 B JSON.

Cross-source sanity: USD per EUR on 2026-10-16 was ECB 1.1253 vs Fed H.10 1.1231 (0.20% apart, different fixing times).
On 2026-10-14 it was ECB 1.1195 vs H.10 1.1123; on 2026-10-15, ECB 1.1208 vs H.10 1.1204.

## Terms pages read (2026-10-21 05:07-05:20 +02:00)

Fetched in full: FRED legal, ECB reuse policy + disclaimer, Fed Board disclaimer, Treasury interest-rate page, Alpha Vantage ToS,
Bank of Canada terms, Bank of England legal, IEX HIST terms, NY Fed terms of use (reference-rate section), Coinbase Market Data
Terms, Binance Vision TERMS_AND_CONDITIONS.md (v1.0, 26 Aug 2026), Yahoo SLN2310, ECB reference-rate page.
From search extracts only: Stooq terms §5.3, Kraken, Polygon/Massive, Nasdaq Data Link, Twelve Data, Finnhub, CoinGecko,
Fiscal Data licensing. Tiingo's terms URL returned 404.

## Live infrastructure (VPS, read-only, 2026-10-21 05:11:32 +02:00)

- `GET http://127.0.0.1:8670/state` (nerve): HTTP 200, 13 075 B, 15 components (memory-gateway, hermes-agents, link-watch,
  galion-desk, tuner, galion-one, speech-io, console, guardian-tick, tunnels x2, voice-relay, orchestration, quotas, nerve),
  6 with `http: 200` (the others are non-HTTP or pull-style). No wallstreet component and no cortex: **the cortex is not live**, so
  the nerve is the only real-time view. The B2 bench is not deployed on the VPS, so there is no live bench to compare against.
- `ss -ltn`: 18 listeners; 8670 and 8642 both listening.
- Nothing on the VPS was changed. No data was fetched from the VPS.
