<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: bench-ws-ecb-data (slot 174, B2b)

Box, branch `swarm/bench-ws-ecb-data`, ports 20440-20449 (used 20440, 20441, 20442). Times Europe/Zagreb (+02:00).

## 1. Recording (box, 2026-10-21 09:50:38 +02:00)

```
curl -sS -o eurofxref-hist.zip https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist.zip         -> 200 641518 B
curl -sS -o treasury-2026.csv  "https://home.treasury.gov/resource-center/data-chart-center/interest-rates/daily-treasury-rates.csv/2026/all?type=daily_treasury_yield_curve&field_tdr_date_value=2026&page&_format=csv"  -> 200 16391 B
unzip eurofxref-hist.zip ; head -41 eurofxref-hist.csv > ecb-eurofxref-hist.csv ; head -41 treasury-2026.csv > ust-par-yield-curve.csv
cmp against head -41 of the originals: identical
sha256 eurofxref-hist.zip 4599576d…e96c  eurofxref-hist.csv 3265d760…1338  treasury-2026.csv 5a14c359…af12
sha256 slices: ecb bc65a73f…716e (10875 B)  ust 0166c66b…a0f1 (3351 B)   (full values in fixtures/real/source.json)
```

## 2. Independent cross-check of the newest row (box, 09:57:40 +02:00)

Different endpoints from the ones recorded:

| Check | Endpoint | Result | Fixture row 2026-10-20 |
|---|---|---|---|
| ECB daily XML | `https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml` (200, 1547 B) | time 2026-10-20: USD 1.1215, JPY 177.34, GBP 0.84028, CHF 0.9237 | identical |
| Treasury XML feed | `.../interest-rates/pages/xml?data=daily_treasury_yield_curve&field_tdr_date_value_month=202610` (200, 20758 B) | last entry: 2Y 4.79, 5Y 4.94, 10Y 5.12, 30Y 5.42 | identical |
| Slot 134 probe (05:10 +02:00, docs/real-tests/bench-data-licence.md) | ECB USD 2026-10-14/15/16 = 1.1195 / 1.1208 / 1.1253; Treasury 10 Yr 2026-10-20 = 5.12 | | identical (asserted in `tests/public_tape.rs::known_published_values`) |

## 3. GALION_REAL release test, real HTTP (box, 09:56:56 +02:00)

`GALION_REAL=1 cargo-slot test -j 2 -p galion-wallstreet --test real_public_tape -- --nocapture` -> **1 passed** (13.3 s incl. release build).

```
galion-wallstreet: PAPER broker listening on 127.0.0.1:20440 (tape ecb-fx, REAL-PUBLIC) (2026-10-21T09:56:56+02:00)
galion-wallstreet: PAPER broker listening on 127.0.0.1:20441 (tape ust, REAL-PUBLIC) (2026-10-21T09:56:56+02:00)
fx first /health after 1 ms   -> tape ecb-fx, data_class REAL-PUBLIC, attribution "Source: ECB statistics.", time +02:00
GET /data (40 days 2026-08-26..2026-10-20, terms URLs)  GET /data/notice (NOT MIT, Source: ECB statistics.)  GET /universe
session x2: /reset -> 2026-08-26; /quote EURUSD 116.69 delayed, source carries attribution;
  buy 100 -> filled 116.75; buy 300 (35 007.00) -> 422 kill_switch_oversized_order; EURGBP -> 422; /kill/reset operator;
  EURGBP -> 422 restricted_symbol; /advance 100 -> as_of 2026-10-20; /close -> filled 112.09; /report + /report.md (PAPER)
B2B-REAL pnl_cents=-47744 fingerprint=fnv1a64:537fa48f279334b0   (both sessions identical; equals the hand formula)
GET /backtest?data=ecb-fx -> 4 runs, EURUSD fnv1a64:0ea17fa4f45eaaed ; ?data=yahoo -> 400 ; /backtest (synthetic) fnv1a64:19424da702570a34
:20441 /health tape ust ; /quote UST10Y source "…Not endorsed by the U.S. Treasury." ; buy 100 filled ; /advance 100 ;
  /quote UST10Y 60.32 as_of 2026-10-20 (= 100/1.0256^20 at 5.12 %) ; /backtest?data=ust fnv1a64:c55c83952c5c599d ; /reset -> 2026-08-24
RSS fx=3088 KiB ust=2824 KiB
CLI: data ust (REAL-PUBLIC), data --notice, backtest --data ecb-fx ok; serve --tape stooq -> exit 2 before binding 20442
both servers killed; 20440 refuses connections afterwards
```

Regression of the existing synthetic real test on this branch, moved into the slot range with the new `WALLSTREET_REAL_PORT`:
`GALION_REAL=1 WALLSTREET_REAL_PORT=20442 … --test real_wallstreet` -> 1 passed, `REAL-1 pnl_cents=102305
fingerprint=fnv1a64:a7856967de663bd5` twice, `/score` 100 (total 100): **identical** to the base record in
`docs/real-tests/bench-wallstreet.md`.

## 4. Cross-target determinism (box, ~09:59 +02:00)

Static musl release binaries, `backtest --data ust` and `--data ecb-fx`: x86_64 (844 576 B, static-pie) and aarch64
(732 200 B, static, run under qemu-aarch64) print the same 5 fingerprints (c55c8395…, 0ea17fa4…, 050a56d4…, 11bd49f1…, 19a03e04…).

## 5. Live server (VPS, read-only, 2026-10-21 09:57:27 +02:00)

`ssh slavik-vps` GETs only:
- nerve `GET 127.0.0.1:8670/state` -> 200, 35 802 B, 0.7 ms; `cycle` 28, `mode` live (openmind-nerve deployed 09:43:54 by slot 180);
  components: 9 healthy, 3 degrading, 1 failing-soon, 2 absent; system degraded.
- `GET 127.0.0.1:8670/cortex` -> 200 (the nerve's cortex view is now answering; first item: warn `unmanaged-listeners` :8672 :19120).
- `ss -ltn`: 19 listeners, none in 19520-19529 / 20440-20449; no `galion-wallstreet` in /opt/galion/rust/bin.
  **The B2 bench is not deployed on the VPS**, so there is no live bench to compare against; the "real data" check is
  the independent publisher cross-check in section 2. Nothing on the VPS was changed.

## 6. Checks

- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: **588 passed, 0 failed, 0 ignored, 147 suites**
  (+15 new: 3 unit in `public.rs`, 11 in `tests/public_tape.rs`, 1 `real_public_tape`; GALION_REAL tests skip without the env).
- `galion-wallstreet` alone: 26 unit + 4 cli + 3 mock_vendor + 11 public_tape + 1 + 1 real (skipped w/o env) = all pass.
- x86_64 + aarch64 musl release build ok (static); `cargo check` aarch64-linux-android + armv7-unknown-linux-musleabihf ok.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` missing 0; `repo-check` 858 files, 0 errors, 45 warnings (none in galion-wallstreet) -> PASS.
