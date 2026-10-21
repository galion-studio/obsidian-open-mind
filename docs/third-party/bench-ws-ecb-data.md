<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: bench-ws-ecb-data (slot 174)

## Code dependencies

None added. `galion-wallstreet` runtime deps stay exactly `serde`, `serde_json`, `zagreb-time` (dev: `galion-bench`).
No copied code. Cargo.lock unchanged.

## Data (committed, not MIT)

| Data | Publisher | Licence / terms (SPDX-style id used in the sidecar) | Source URL | Terms URL | Used for | Why not write it ourselves |
|---|---|---|---|---|---|---|
| Euro foreign exchange reference rates, 40 business days, all currencies, verbatim | European Central Bank | ECB statistics reuse policy: free reuse with "Source: ECB statistics.", no modification (`LicenseRef-ECB-statistics-reuse`) | https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist.zip | https://www.ecb.europa.eu/stats/ecb_statistics/governance_and_quality_framework/html/usage_policy.en.html | `ecb-fx` replay tape (EURUSD, EURGBP, EURCHF, EURJPY) | real prices cannot be invented; synthetic data already exists |
| Daily Treasury Par Yield Curve Rates, 40 business days, verbatim | U.S. Department of the Treasury | U.S. Government work, public domain, 17 U.S.C. § 105 (`LicenseRef-US-Government-Public-Domain`); credit Treasury, no endorsement | https://home.treasury.gov/resource-center/data-chart-center/interest-rates/daily-treasury-rates.csv/2026/all?type=daily_treasury_yield_curve&field_tdr_date_value=2026&page&_format=csv | https://resources.data.gov/open-licenses/ · https://www.treasurydirect.gov/legal-information/terms/ | `ust` replay tape (derived zero-coupon proxies UST2Y/5Y/10Y/30Y) | same |

Attribution lines live in `crates/galion-wallstreet/fixtures/real/NOTICE.md` and are served at `GET /data/notice`.
Licence research: `docs/bench/market-data-licence.md` (slot 134).

## Tools used once (not dependencies)

`curl` (HTTPS GET, no key) and `unzip` to record the slices, `sha256sum` to hash them. Not run by the build or tests.
