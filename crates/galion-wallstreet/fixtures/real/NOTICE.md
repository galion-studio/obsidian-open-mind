<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# fixtures/real: REAL-PUBLIC market data (NOT MIT)

The two CSV files in this folder are **not** covered by the repository's MIT licence. They are verbatim slices of
public data and keep their publishers' terms. Only this NOTICE, `source.json` and the `.license` sidecars are ours.

| File | What | Publisher terms |
|---|---|---|
| `ecb-eurofxref-hist.csv` | First 41 lines (header + 40 business days, 2026-08-26 .. 2026-10-20) of `eurofxref-hist.csv` inside `https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist.zip`, byte-for-byte, newest first, all currencies | **Source: ECB statistics.** Reuse is free of charge provided the source is quoted and the statistics (including metadata) are not modified: https://www.ecb.europa.eu/stats/ecb_statistics/governance_and_quality_framework/html/usage_policy.en.html . The ECB publishes the reference rates for information purposes only: https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html |
| `ust-par-yield-curve.csv` | First 41 lines (header + 40 business days, 08/24/2026 .. 10/20/2026) of the Daily Treasury Par Yield Curve Rates CSV for 2026 (`https://home.treasury.gov/resource-center/data-chart-center/interest-rates/daily-treasury-rates.csv/2026/all?type=daily_treasury_yield_curve&field_tdr_date_value=2026&page&_format=csv`), byte-for-byte, newest first | Source: U.S. Department of the Treasury, Daily Treasury Par Yield Curve Rates. U.S. Government work, public domain (17 U.S.C. § 105). Not endorsed by the U.S. Treasury. https://home.treasury.gov/policy-issues/financing-the-government/interest-rate-statistics · https://resources.data.gov/open-licenses/ · https://www.treasurydirect.gov/legal-information/terms/ |

Rules the code follows:

- **The committed files are never edited.** Re-recording replaces a whole file and updates `source.json`.
- ECB values are converted to paper prices by **exact integer scaling only** (a lot of 100 or 1000 EUR, no rounding).
  `public::verify_ecb_unmodified` proves every price maps back to the exact published string.
- Treasury yields are turned into a *derived* zero-coupon price proxy (`100 / (1 + y/200)^(2n)`, integer fixed point).
  That is a Galion computation on public-domain data, labelled as such; it is not a Treasury statistic.
- Every quote, backtest and `/data` response carries `data_class: "REAL-PUBLIC"` and the attribution line, so a real-tape
  score is never mixed with the `SYNTHETIC` 100/100 scoreboard.
- Paper only: no broker, no orders, no money. The ECB says its rates are for information purposes only; they are used here
  as a historical replay tape, not as transaction rates.
