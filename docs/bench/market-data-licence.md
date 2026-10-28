<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# B2 Wall Street: free delayed market data we may commit to a public repo

Slot 134 (bench-data-licence), wave 4. Research only: **no data is committed by this branch**, no account was created,
no API key was used, no paid call was made. Terms read and probes run 2026-10-21 05:07-05:20 CEST (+02:00).
This answers decision **D-B2-1** in `docs/bench/wallstreet.md` (branch `swarm/bench-wallstreet` 4b1e127):
"(b) record a small slice from a free public source whose terms allow redistribution in a public repo".

Not legal advice. Each verdict quotes the exact clause and links to the terms URL so the owner can check it.

## Bottom line

1. **No free equity source gives a clean open licence.** The one free US-equity source whose terms *expressly allow
   redistribution* is **IEX HIST** (IEX Exchange's own tick data, T+1). It comes with a required attribution line, and IEX keeps
   ownership. It is usable, but the data is IEX-venue-only and each day is a gzip file of about 11-15 GB.
2. **Rates and FX from central banks and governments are the clean option.** US Treasury par yield curve (US public domain),
   Federal Reserve Board H.10 FX (public domain), ECB euro reference rates (free reuse with "Source: ECB statistics" and no
   modification), NY Fed SOFR (free licence plus a fixed notice). All four answered over plain HTTPS with no key from the box today.
3. **Every popular "free" price API forbids it:** Yahoo, Stooq, Alpha Vantage, Coinbase, Kraken, Binance Vision (non-commercial
   share-alike only), Polygon/Massive, Nasdaq Data Link, Twelve Data, Finnhub, Tiingo and CoinGecko. FRED, the obvious
   aggregator, also forbids it. Its terms ban "incorporating any FRED Content in any database, compilation, archive". Take the
   same series from the **original publisher** (Treasury, the Fed Board, the ECB) instead.

**Recommendation for D-B2-1 (the owner decides):** option **(b1)**, a ~40-business-day slice of **Treasury par yields + H.10 or ECB FX
+ SOFR**, committed as fixtures with a `fixtures/real/NOTICE.md` carrying the required attribution lines. That gives the
Wall Street bench a real, reproducible rates/FX "tape" with zero licence ambiguity. Add **(b2) IEX HIST equities** only if the
owner accepts IEX's attribution clause and its proprietary-rights clause (03) for a public repo. The fixtures should keep
the `SYNTHETIC`/`REAL` label so scores stay comparable.

## Verdict table

Legend: **YES** = terms allow committing a small slice to a public repo. **COND** = allowed only with conditions that need an owner
OK. **NO** = forbidden without written permission.

| # | Source (what) | Delay / cadence | Key? | Verdict | Licence basis (quote) | Terms URL |
|---|---|---|---|---|---|---|
| 1 | US Treasury Daily Par Yield Curve Rates (1 Mo-30 Yr) | EOD, ~3:30 PM ET quotes, daily | no | **YES** | US Government work, not subject to copyright (17 U.S.C. § 105): "works created by U.S. Government employees within the scope of their employment default to U.S. Public Domain". Treasury asks for credit and forbids implying endorsement (31 U.S.C. § 333). | https://home.treasury.gov/policy-issues/financing-the-government/interest-rate-statistics · https://resources.data.gov/open-licenses/ · https://www.treasurydirect.gov/legal-information/terms/ |
| 2 | Federal Reserve Board H.10 FX (noon buying rates, e.g. USD per EUR) | EOD, daily (weekly release) | no | **YES** | "Unless otherwise indicated, information on Board's website is in the public domain and may be copied and distributed without permission. Please cite to the Board as the source." | https://www.federalreserve.gov/disclaimer.htm · data: https://www.federalreserve.gov/releases/h10/hist/ |
| 3 | ECB euro foreign exchange reference rates (~30 currencies) | EOD, ~16:00 CET, daily | no | **YES** (no modification) | "All publicly available ESCB statistics may be reused free of charge on the condition that the source is quoted (e.g. 'Source: ECB statistics.') and that the statistics (including metadata) are not modified." Third-party data is excluded. The ECB says the rates are "published for information purposes only". | https://www.ecb.europa.eu/stats/ecb_statistics/governance_and_quality_framework/html/usage_policy.en.html · https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html |
| 4 | NY Fed SOFR (and other reference rates) | EOD, published next business day | no | **YES** (fixed notice + same terms onward) | "The New York Fed grants you a non-exclusive license ... to use, copy, and distribute Content for your personal or business purposes". Redistribution must carry the same terms and the notice "The SOFR is subject to the Terms of Use posted at newyorkfed.org. The New York Fed is not responsible for publication of the SOFR by [publisher], does not sanction or endorse any particular republication, and has no liability for your use." | https://www.newyorkfed.org/privacy/termsofuse · https://www.newyorkfed.org/markets/reference-rates-terms-of-use |
| 5 | **IEX HIST** (IEX Exchange TOPS/DEEP pcap, US equities, IEX venue only) | T+1, trailing 12 months + archive since 2016-12-12 | no | **COND** | "If you distribute, sell, lease, furnish, or otherwise permit or provide access to ... IEX historical data, you must cite IEX as the source with the following text and link: 'Data provided for free by IEX. By accessing or using IEX Historical Data, you agree to the IEX Historical Data Terms of Use.'" Clause 03: IEX keeps "exclusive proprietary rights". Clause 08: the data "does not reflect trading activity on markets other than IEX". | https://www.iex.io/legal/hist-data-terms |
| 6 | US Treasury Fiscal Data (avg. interest rates, FX reporting rates) | monthly/quarterly | no | YES but not market-like | "available to copy, adapt, redistribute, or otherwise use for non-commercial and commercial purposes" | https://fiscaldata.treasury.gov/about-us/ |
| 7 | Bank of Canada FX | EOD | no | **NO for FX** | Site content is freely reusable with attribution, *but* FX: "The underlying data is sourced from LSEG". Section 2.3 gives no permission for third-party content. | https://www.bankofcanada.ca/terms/ |
| 8 | Bank of England Database | EOD | no | COND (rates YES, **FX NO**) | Database is under the UK Open Government Licence, "Please note that selected exchange rate data and series are excluded from this licence as they are reproduced by the Bank under licence from third parties." | https://www.bankofengland.co.uk/legal |
| 9 | FRED (St. Louis Fed) | varies | key for API | **NO** (use the original publisher) | Prohibited: "Store, cache, or archive any portion of ... FRED Content; ... or incorporate any FRED Content in any database, compilation, archive". Also bans using FRED content for AI/ML development. | https://fred.stlouisfed.org/legal/ |
| 10 | Stooq | EOD + intraday | captcha key | **NO** | "5.3. Redistribution of data found on the website is not allowed without the consent of Stooq." S&P and LME data are personal-use only. | https://stooq.com/terms.html |
| 11 | Yahoo Finance | 15-20 min / EOD | no | **NO** | "You must not redistribute information displayed on or provided by Yahoo Finance." | https://help.yahoo.com/kb/SLN2310.html |
| 12 | Alpha Vantage | EOD / delayed | free key | **NO** | Licence "for personal, non-commercial use"; use on behalf of a company, or giving others access to the data, counts as "commercial use". | https://www.alphavantage.co/terms_of_service/ |
| 13 | Coinbase Exchange market data | real-time / candles | no | **NO** | May not "Redistribute, display, or disseminate the Market Data—or any ... Derived Works — to any third party outside of your organization". Also bans AI/agent training and benchmarking use. | https://www.coinbase.com/legal/market_data |
| 14 | Binance Vision archives (data.binance.vision) | daily dumps | no | **NO for us** | Dataset Terms v1.0 (26 Aug 2026) §3.1 CC BY-NC-SA 4.0; §4.5 derivatives must stay CC BY-NC-SA. Non-commercial + share-alike are outside our allowed-licence list (RULES.md). The repo README "MIT" covers the scripts only. | https://github.com/binance/binance-public-data/blob/master/TERMS_AND_CONDITIONS.md |
| 15 | Kraken downloadable OHLCVT | quarterly dumps | no | **NO** | Global Terms forbid copying/distributing content. Market Rulebook 13.1: "may not collect, redistribute, or manipulate ... market data ... without the explicit written authorisation". | https://www.kraken.com/legal/global-terms · https://support.kraken.com/articles/360047124832 |
| 16 | Polygon.io / Massive (free tier) | EOD / delayed | key | **NO** | Redistribution needs a commercial licence and prior written consent. | https://massive.com/terms/market_data_terms.pdf · https://massive.com/knowledge-base/article/how-can-i-redistribute-massives-market-data |
| 17 | Nasdaq Data Link | EOD | key | **NO** | Internal use only; no redistribution unless a service order allows it. | https://data.nasdaq.com/terms |
| 18 | Twelve Data / Finnhub / Tiingo | delayed / EOD | key | **NO** | Redistribution only with a paid add-on, an enterprise plan or a written agreement. | https://twelvedata.com/terms · https://finnhub.io/pricing-startups-and-enterprise · (Tiingo terms page returned 404 today, not verified) |
| 19 | CoinGecko (Demo plan) | delayed | key | **NO** | No redistributing or syndicating raw data on standard plans. Redistribution needs an Enterprise licence. | https://www.coingecko.com/en/api_terms |

Note: rows 16-19 were checked against the terms pages via web search extracts, not
read in full; they are all "NO" either way, so they do not change the recommendation.

## What a committed slice would look like (proposal, not done here)

- Location: `crates/galion-wallstreet/fixtures/real/` beside the existing synthetic `bars.csv`, with
  `fixtures/real/NOTICE.md` (attribution lines below) and `REUSE.toml`/`.license` sidecars, so REUSE lint does not tag the
  data as MIT. **The data is not MIT. It keeps its own terms.**
- Size: 40 business days is about 3.4 KB for the Treasury curve, ~1.6 KB for 4 ECB currencies, ~10 KB of SOFR JSON (measured below).
- Recording: a Rust `galion-wallstreet record` subcommand (GET only, no key) run by hand. The output must be **byte-for-byte
  unmodified** values (ECB rule), plus a `source.json` with URL, fetch time (+02:00) and sha256. The bench keeps `delay_bars: 1`.
- Labels: `data_class = "REAL-PUBLIC"` vs `"SYNTHETIC"`, so the 100/100 synthetic score and any real-tape score are never mixed.
- Attribution lines for NOTICE.md:
  - "Source: U.S. Department of the Treasury, Daily Treasury Par Yield Curve Rates. U.S. Government work, public domain. Not endorsed by the U.S. Treasury."
  - "Source: Board of Governors of the Federal Reserve System, H.10 Foreign Exchange Rates."
  - "Source: ECB statistics." (values unmodified)
  - "The SOFR is subject to the Terms of Use posted at newyorkfed.org. The New York Fed is not responsible for publication of the SOFR by Galion / galion-studio, does not sanction or endorse any particular republication, and has no liability for your use."
  - (only if IEX is approved) "Data provided for free by IEX. By accessing or using IEX Historical Data, you agree to the IEX Historical Data Terms of Use." with a link to https://www.iex.io/legal/hist-data-terms

## Why rates/FX still fit the B2 bench

The bench's broker trades "symbols with bars". A rates/FX tape gives real, dated moves: on 2026-10-16, ECB USD/EUR was 1.1253 and
H.10 was 1.1231, a 0.20% gap between the 14:10 CET concertation and the New York noon fix. That is a free, honest test of
"two sources disagree, say so" for Hermes's report-quality axis. The equity universe (7 symbols) can stay synthetic until the
IEX decision.

## IEX HIST: practical cost if approved

- Index `https://iextrading.com/api/1.0/hist` (JSON, 1.39 MB, 2492 dates, 2016-12-12 .. 2026-10-19 today).
- A TOPS day is a gzip **pcapng** file of **11.2 GB (2026-10-19) to 14.6 GB (2026-10-16)**. A range GET of the first 4 KB
  returned HTTP 206 with gzip magic `1f 8b` and pcapng magic `0a 0d 0d 0a` after inflating, so it can be streamed.
- Making 7-symbol daily bars means streaming the whole day through a Rust IEX-TP/TOPS decoder (std-only, no new deps needed:
  gzip would need `flate2`/`miniz_oxide`, MIT/Apache-2.0) and keeping only trade reports. That is ~11-15 GB of download per day,
  so 40 days is about 500 GB. Too heavy for the box during a swarm run; 5-10 days is realistic.
- Prices are **IEX-only last sales** (IEX is a few % of US volume), not consolidated closes. Label them that way.

## Owner decisions

- **D-B2-1a:** approve (b1) public-domain/open rates+FX slice (Treasury + Fed H.10 + ECB + SOFR) committed with NOTICE.md? (recommended)
- **D-B2-1b:** approve (b2) IEX HIST equity slice under IEX's attribution + proprietary-rights terms in a public repo, or keep equities synthetic?
- **D-B2-1c:** OK to treat fixture data as *not MIT* (REUSE sidecars with its own terms) in an otherwise MIT repo?
- Anything crypto stays synthetic: no free crypto source found that allows public redistribution for a commercial-capable project.
