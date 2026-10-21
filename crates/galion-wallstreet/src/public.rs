// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL-PUBLIC replay tapes (bench B2b) built from verbatim public data slices
//! committed under `fixtures/real/` (see `fixtures/real/NOTICE.md`):
//!
//! - ECB euro foreign exchange reference rates ("Source: ECB statistics."),
//!   reused **unmodified**: the CSV is a byte-for-byte slice of the ECB file and
//!   paper prices come from exact integer scaling (a lot of 100 or 1000 EUR),
//!   never rounding. [`verify_ecb_unmodified`] proves the round trip.
//! - U.S. Treasury daily par yield curve (US Government work, public domain),
//!   turned into a derived zero-coupon price proxy in integer fixed point.
//!
//! No network code here either: the slices were recorded once by hand (plain
//! HTTPS GET, no key) and the recording is described in `fixtures/real/source.json`.

use crate::broker::{Config, PaperBroker};
use crate::market::{ingest, Feed, MarketData};
use crate::universe::{self, Universe};
use serde::Serialize;

/// Verbatim ECB slice: header + 40 business days, newest first, all currencies.
pub const ECB_CSV: &str = include_str!("../fixtures/real/ecb-eurofxref-hist.csv");
/// Verbatim Treasury par yield slice: header + 40 business days, newest first.
pub const UST_CSV: &str = include_str!("../fixtures/real/ust-par-yield-curve.csv");
pub const NOTICE_MD: &str = include_str!("../fixtures/real/NOTICE.md");
pub const SOURCE_JSON: &str = include_str!("../fixtures/real/source.json");

pub const DATA_CLASS_REAL: &str = "REAL-PUBLIC";
pub const DATA_CLASS_SYNTHETIC: &str = "SYNTHETIC";
/// The attribution the ECB reuse policy requires, verbatim.
pub const ECB_ATTRIBUTION: &str = "Source: ECB statistics.";
pub const UST_ATTRIBUTION: &str = "Source: U.S. Department of the Treasury, Daily Treasury Par Yield Curve Rates. U.S. Government work, public domain. Not endorsed by the U.S. Treasury.";
pub const ECB_TERMS_URL: &str = "https://www.ecb.europa.eu/stats/ecb_statistics/governance_and_quality_framework/html/usage_policy.en.html";
pub const ECB_RATES_URL: &str = "https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html";
pub const UST_TERMS_URLS: [&str; 3] = [
    "https://home.treasury.gov/policy-issues/financing-the-government/interest-rate-statistics",
    "https://resources.data.gov/open-licenses/",
    "https://www.treasurydirect.gov/legal-information/terms/",
];

/// FX instruments on the ECB tape: (ticker, ECB currency column).
/// The paper account is USD, so only EURUSD is tradable in a mixed session;
/// the other pairs are replayed per pair in their own quote currency.
pub const FX_PAIRS: [(&str, &str); 4] = [
    ("EURUSD", "USD"),
    ("EURGBP", "GBP"),
    ("EURCHF", "CHF"),
    ("EURJPY", "JPY"),
];

/// Treasury instruments: (ticker, CSV tenor column, years to maturity).
pub const UST_TENORS: [(&str, &str, u32); 4] = [
    ("UST2Y", "2 Yr", 2),
    ("UST5Y", "5 Yr", 5),
    ("UST10Y", "10 Yr", 10),
    ("UST30Y", "30 Yr", 30),
];

// ---------------------------------------------------------------- decimals

/// An exact decimal as published: `mantissa / 10^scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dec {
    pub mantissa: i64,
    pub scale: u32,
}

impl Dec {
    pub fn parse(s: &str) -> Result<Dec, String> {
        let (w, f) = s.split_once('.').unwrap_or((s, ""));
        let ok = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
        if w.is_empty() || !ok(w) || !ok(f) || f.len() > 9 || (s.contains('.') && f.is_empty()) {
            return Err(format!("bad decimal {s:?}"));
        }
        let digits = format!("{w}{f}");
        let mantissa: i64 = digits
            .parse()
            .map_err(|_| format!("decimal out of range {s:?}"))?;
        Ok(Dec {
            mantissa,
            scale: f.len() as u32,
        })
    }
    /// Same value with trailing fractional zeros removed (1.1200 == 1.12).
    pub fn normalized(self) -> Dec {
        let mut d = self;
        while d.scale > 0 && d.mantissa % 10 == 0 {
            d.mantissa /= 10;
            d.scale -= 1;
        }
        d
    }
}

// ---------------------------------------------------------------- dates

fn iso_ok(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
}

/// Treasury `MM/DD/YYYY` -> ISO `YYYY-MM-DD`.
pub fn us_date_to_iso(d: &str) -> Result<String, String> {
    let p: Vec<&str> = d.split('/').collect();
    if p.len() != 3 || p[0].len() != 2 || p[1].len() != 2 || p[2].len() != 4 {
        return Err(format!("bad US date {d:?}"));
    }
    let iso = format!("{}-{}-{}", p[2], p[0], p[1]);
    if !iso_ok(&iso) {
        return Err(format!("bad US date {d:?}"));
    }
    Ok(iso)
}

// ---------------------------------------------------------------- tables

/// A parsed rate table: columns, and rows sorted oldest first. Raw strings are
/// kept exactly as published (`None` for `N/A` or empty cells).
#[derive(Debug, Clone)]
pub struct RateTable {
    pub columns: Vec<String>,
    pub rows: Vec<(String, Vec<Option<String>>)>,
}

impl RateTable {
    pub fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
    pub fn days(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.0.clone()).collect()
    }
    pub fn raw(&self, date: &str, col: &str) -> Option<&str> {
        let c = self.col(col)?;
        let r = self.rows.iter().find(|r| r.0 == date)?;
        r.1.get(c)?.as_deref()
    }
}

fn parse_table(
    csv: &str,
    what: &str,
    date_conv: fn(&str) -> Result<String, String>,
) -> Result<RateTable, String> {
    let mut lines = csv.lines().enumerate();
    let (_, header) = lines.next().ok_or(format!("{what}: empty file"))?;
    let mut h: Vec<String> = header
        .split(',')
        .map(|c| c.trim_matches('"').to_string())
        .collect();
    if h.first().map(String::as_str) != Some("Date") {
        return Err(format!("{what}: line 1: header must start with Date"));
    }
    // ECB rows end with a trailing comma; keep the column count consistent.
    let trailing = h.last().is_some_and(String::is_empty);
    if trailing {
        h.pop();
    }
    let columns: Vec<String> = h[1..].to_vec();
    if columns.iter().any(String::is_empty) {
        return Err(format!("{what}: line 1: empty column name"));
    }
    let mut rows: Vec<(String, Vec<Option<String>>)> = Vec::new();
    for (i, line) in lines {
        let n = i + 1;
        if line.is_empty() {
            continue;
        }
        let mut f: Vec<&str> = line.split(',').collect();
        if trailing {
            if f.last() != Some(&"") {
                return Err(format!("{what}: line {n}: missing trailing comma"));
            }
            f.pop();
        }
        if f.len() != columns.len() + 1 {
            return Err(format!(
                "{what}: line {n}: expected {} fields, got {}",
                columns.len() + 1,
                f.len()
            ));
        }
        let date = date_conv(f[0]).map_err(|e| format!("{what}: line {n}: {e}"))?;
        let mut vals = Vec::with_capacity(columns.len());
        for v in &f[1..] {
            if v.is_empty() || *v == "N/A" {
                vals.push(None);
            } else {
                Dec::parse(v).map_err(|e| format!("{what}: line {n}: {e}"))?;
                vals.push(Some((*v).to_string()));
            }
        }
        if let Some(prev) = rows.last() {
            if prev.0.as_str() <= date.as_str() {
                return Err(format!(
                    "{what}: line {n}: dates must be strictly newest first"
                ));
            }
        }
        rows.push((date, vals));
    }
    if rows.is_empty() {
        return Err(format!("{what}: no rows"));
    }
    rows.reverse();
    Ok(RateTable { columns, rows })
}

fn iso_date(d: &str) -> Result<String, String> {
    if iso_ok(d) {
        Ok(d.to_string())
    } else {
        Err(format!("bad date {d:?}"))
    }
}

pub fn parse_ecb(csv: &str) -> Result<RateTable, String> {
    parse_table(csv, "ecb", iso_date)
}

pub fn parse_ust(csv: &str) -> Result<RateTable, String> {
    parse_table(csv, "ust", us_date_to_iso)
}

pub fn ecb_table() -> RateTable {
    parse_ecb(ECB_CSV).expect("committed ECB slice parses")
}

pub fn ust_table() -> RateTable {
    parse_ust(UST_CSV).expect("committed Treasury slice parses")
}

// ---------------------------------------------------------------- ECB -> bars

/// Lot size in EUR so every published rate of `ccy` is an exact number of
/// quote-currency cents: 10^(max published decimals - 2), at least 1.
pub fn ecb_lot_eur(t: &RateTable, ccy: &str) -> Result<i64, String> {
    let c = t.col(ccy).ok_or(format!("ecb: no column {ccy}"))?;
    let mut max = 0u32;
    for (d, r) in &t.rows {
        let raw = r[c]
            .as_deref()
            .ok_or(format!("ecb: {ccy} missing on {d}"))?;
        max = max.max(Dec::parse(raw)?.scale);
    }
    Ok(10i64.pow(max.saturating_sub(2)))
}

/// Exact price in cents of `lot` EUR at the published rate. Errors instead of rounding.
pub fn ecb_price_cents(raw: &str, lot: i64) -> Result<i64, String> {
    let d = Dec::parse(raw)?;
    let num = d.mantissa as i128 * lot as i128 * 100;
    let den = 10i128.pow(d.scale);
    if num % den != 0 {
        return Err(format!("{raw} x {lot} EUR is not a whole number of cents"));
    }
    i64::try_from(num / den).map_err(|_| format!("{raw} out of range"))
}

fn fmt_price(cents: i64) -> String {
    crate::money::fmt_cents(cents)
}

/// Bars CSV (crate format) for the given ECB pairs. One fixing per day, so
/// open = high = low = close; volume is 0 (the ECB publishes no volume).
pub fn ecb_bars_csv(t: &RateTable, pairs: &[(&str, &str)]) -> Result<String, String> {
    let mut o = String::new();
    o.push_str(&format!(
        "# source: {DATA_CLASS_REAL} ECB euro foreign exchange reference rates, {ECB_ATTRIBUTION} Values unmodified (exact lot scaling). Daily fixing ~16:00 CET, {}..{}\n",
        t.rows[0].0,
        t.rows[t.rows.len() - 1].0
    ));
    o.push_str("# delay_bars: 1\n");
    o.push_str("date,symbol,open,high,low,close,volume\n");
    let mut lots = Vec::new();
    for (tk, ccy) in pairs {
        lots.push((*tk, *ccy, ecb_lot_eur(t, ccy)?));
    }
    for (tk, ccy, lot) in &lots {
        let c = t.col(ccy).ok_or(format!("ecb: no column {ccy}"))?;
        for (d, r) in &t.rows {
            let raw = r[c]
                .as_deref()
                .ok_or(format!("ecb: {ccy} missing on {d}"))?;
            let p = fmt_price(ecb_price_cents(raw, *lot)?);
            o.push_str(&format!("{d},{tk},{p},{p},{p},{p},0\n"));
        }
    }
    Ok(o)
}

/// Check every bar of an ECB-derived tape maps back to the exact published
/// value. Returns the number of values checked.
pub fn verify_ecb_unmodified(
    t: &RateTable,
    md: &MarketData,
    pairs: &[(&str, &str)],
) -> Result<usize, String> {
    let mut n = 0;
    for (tk, ccy) in pairs {
        let lot = ecb_lot_eur(t, ccy)?;
        let series = md.series(tk).ok_or(format!("tape has no {tk}"))?;
        if series.len() != t.rows.len() {
            return Err(format!(
                "{tk}: {} bars vs {} ECB rows",
                series.len(),
                t.rows.len()
            ));
        }
        for b in series {
            let raw = t
                .raw(&b.date, ccy)
                .ok_or(format!("{tk}: no ECB value on {}", b.date))?;
            let published = Dec::parse(raw)?.normalized();
            // close cents / lot EUR = rate * 100  ->  rate = close / (lot * 100)
            let back = Dec {
                mantissa: b.close,
                scale: 2 + lot.ilog10(),
            }
            .normalized();
            if back != published || b.open != b.close || b.high != b.close || b.low != b.close {
                return Err(format!("{tk} {}: tape {back:?} != published {raw}", b.date));
            }
            n += 1;
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------- UST -> bars

const FP: i128 = 1_000_000_000_000_000_000; // 1e18

/// Zero-coupon price proxy in cents for face 100.00 at par yield `y`
/// (hundredths of a percent, e.g. 512 = 5.12 %), semi-annual compounding:
/// `100 / (1 + y/200)^(2n)`. Integer fixed point, round half up: identical
/// on every target. A Galion-derived number, not a Treasury statistic.
pub fn zc_price_cents(y_hundredths: i64, years: u32) -> i64 {
    // one half-year discount factor = 20000 / (20000 + y)
    let r = 20_000 * FP / (20_000 + y_hundredths as i128);
    let mut acc = FP;
    for _ in 0..(2 * years) {
        acc = (acc * r + FP / 2) / FP;
    }
    // cents = 10000 * acc / 1e18, rounded half up
    ((acc * 10_000 + FP / 2) / FP) as i64
}

pub fn ust_bars_csv(t: &RateTable, tenors: &[(&str, &str, u32)]) -> Result<String, String> {
    let mut o = String::new();
    o.push_str(&format!(
        "# source: {DATA_CLASS_REAL} U.S. Treasury daily par yield curve (public domain), derived zero-coupon price proxy 100/(1+y/200)^(2n) per 100 face, {}..{}. Not endorsed by the U.S. Treasury.\n",
        t.rows[0].0,
        t.rows[t.rows.len() - 1].0
    ));
    o.push_str("# delay_bars: 1\n");
    o.push_str("date,symbol,open,high,low,close,volume\n");
    for (tk, col, years) in tenors {
        let c = t.col(col).ok_or(format!("ust: no column {col}"))?;
        for (d, r) in &t.rows {
            let raw = r[c]
                .as_deref()
                .ok_or(format!("ust: {col} missing on {d}"))?;
            let y = Dec::parse(raw)?;
            if y.scale > 2 {
                return Err(format!("ust: {col} {raw} has more than 2 decimals"));
            }
            let yh = y.mantissa * 10i64.pow(2 - y.scale);
            let p = fmt_price(zc_price_cents(yh, *years));
            o.push_str(&format!("{d},{tk},{p},{p},{p},{p},0\n"));
        }
    }
    Ok(o)
}

// ---------------------------------------------------------------- tapes

/// Which data a broker / backtest runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tape {
    #[default]
    Synthetic,
    EcbFx,
    Ust,
}

impl Tape {
    pub const ALL: [Tape; 3] = [Tape::Synthetic, Tape::EcbFx, Tape::Ust];

    pub fn parse(s: &str) -> Result<Tape, String> {
        match s {
            "synthetic" | "" => Ok(Tape::Synthetic),
            "ecb-fx" | "ecb" => Ok(Tape::EcbFx),
            "ust" | "treasury" => Ok(Tape::Ust),
            o => Err(format!("unknown tape {o:?} (synthetic | ecb-fx | ust)")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Tape::Synthetic => "synthetic",
            Tape::EcbFx => "ecb-fx",
            Tape::Ust => "ust",
        }
    }
    pub fn data_class(self) -> &'static str {
        match self {
            Tape::Synthetic => DATA_CLASS_SYNTHETIC,
            _ => DATA_CLASS_REAL,
        }
    }
    pub fn attribution(self) -> &'static str {
        match self {
            Tape::Synthetic => "SYNTHETIC recorded fixture, not real market data",
            Tape::EcbFx => ECB_ATTRIBUTION,
            Tape::Ust => UST_ATTRIBUTION,
        }
    }
    pub fn market(self) -> MarketData {
        match self {
            Tape::Synthetic => MarketData::fixture(),
            Tape::EcbFx => ingest(&ecb_bars_csv(&ecb_table(), &FX_PAIRS).expect("ECB tape"))
                .expect("ECB tape ingests"),
            Tape::Ust => ingest(&ust_bars_csv(&ust_table(), &UST_TENORS).expect("UST tape"))
                .expect("UST tape ingests"),
        }
    }
    pub fn universe_csv(self) -> String {
        let mut o = String::from("ticker,name,sector,status,note\n");
        match self {
            Tape::Synthetic => return universe::UNIVERSE_FIXTURE.to_string(),
            Tape::EcbFx => {
                let t = ecb_table();
                for (tk, ccy) in FX_PAIRS {
                    let lot = ecb_lot_eur(&t, ccy).unwrap_or(1);
                    if ccy == "USD" {
                        o.push_str(&format!(
                            "{tk},{lot} EUR in USD (ECB reference rate),FX,tradable,{ECB_ATTRIBUTION}\n"
                        ));
                    } else {
                        o.push_str(&format!(
                            "{tk},{lot} EUR in {ccy} (ECB reference rate),FX,restricted,quote currency {ccy} is not the USD paper account; replay per pair only. {ECB_ATTRIBUTION}\n"
                        ));
                    }
                }
            }
            Tape::Ust => {
                for (tk, col, years) in UST_TENORS {
                    o.push_str(&format!(
                        "{tk},{years}y zero-coupon proxy from the {col} par yield (100 face),Rates,tradable,derived from U.S. Treasury par yields; not a Treasury statistic\n"
                    ));
                }
            }
        }
        o
    }
    pub fn universe(self) -> Universe {
        universe::parse(&self.universe_csv()).expect("tape universe parses")
    }
    pub fn broker(self) -> PaperBroker {
        let mut b = PaperBroker::new(Feed::new(self.market()), self.universe(), Config::default());
        b.tape = self;
        b
    }
}

/// Summary served at `GET /data` and printed by `galion-wallstreet data`.
#[derive(Debug, Clone, Serialize)]
pub struct TapeInfo {
    pub tape: Tape,
    pub data_class: &'static str,
    pub attribution: &'static str,
    pub source: String,
    pub first_day: String,
    pub last_day: String,
    pub days: usize,
    pub symbols: Vec<String>,
    pub terms_urls: Vec<&'static str>,
    pub delay_bars: usize,
}

pub fn info(t: Tape) -> TapeInfo {
    let md = t.market();
    TapeInfo {
        tape: t,
        data_class: t.data_class(),
        attribution: t.attribution(),
        source: md.source.clone(),
        first_day: md.days.first().cloned().unwrap_or_default(),
        last_day: md.days.last().cloned().unwrap_or_default(),
        days: md.days.len(),
        symbols: md.symbols().cloned().collect(),
        terms_urls: match t {
            Tape::Synthetic => vec![],
            Tape::EcbFx => vec![ECB_TERMS_URL, ECB_RATES_URL],
            Tape::Ust => UST_TERMS_URLS.to_vec(),
        },
        delay_bars: md.delay_bars,
    }
}

/// FNV-1a 64 over raw bytes (tamper check against `source.json`).
pub fn fnv1a64(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("fnv1a64:{h:016x}")
}

// ---------------------------------------------------------------- backtests

/// Reference SMA backtest on a REAL-PUBLIC tape. ECB pairs run one per paper
/// account in their own quote currency (the cents are that currency's cents);
/// Treasury proxies share one USD account.
#[derive(Debug, Clone, Serialize)]
pub struct RealBacktest {
    pub tape: Tape,
    pub data_class: &'static str,
    pub attribution: &'static str,
    pub account_currency: String,
    pub result: crate::backtest::BacktestResult,
}

pub fn backtests(t: Tape) -> Vec<RealBacktest> {
    use crate::strategy::SmaCross;
    let mut out = Vec::new();
    match t {
        Tape::Synthetic => out.push(RealBacktest {
            tape: t,
            data_class: t.data_class(),
            attribution: t.attribution(),
            account_currency: "USD".into(),
            result: crate::backtest::reference(),
        }),
        Tape::EcbFx => {
            let md = t.market();
            for (tk, ccy) in FX_PAIRS {
                let u = universe::parse(&format!(
                    "ticker,name,sector,status,note\n{tk},EUR in {ccy},FX,tradable,{ECB_ATTRIBUTION}\n"
                ))
                .expect("pair universe");
                // Size each pair to ~10 000.00 of its quote currency on day one.
                let first = md.series(tk).map(|s| s[0].close).unwrap_or(1).max(1);
                let mut s = SmaCross {
                    fast: 3,
                    slow: 8,
                    qty: (1_000_000 / first).max(1),
                };
                out.push(RealBacktest {
                    tape: t,
                    data_class: t.data_class(),
                    attribution: t.attribution(),
                    account_currency: ccy.into(),
                    result: crate::backtest::run(md.clone(), u, Config::default(), &mut s, &[tk]),
                });
            }
        }
        Tape::Ust => {
            let mut s = SmaCross {
                fast: 3,
                slow: 8,
                qty: 100,
            };
            let syms: Vec<&str> = UST_TENORS.iter().map(|x| x.0).collect();
            out.push(RealBacktest {
                tape: t,
                data_class: t.data_class(),
                attribution: t.attribution(),
                account_currency: "USD".into(),
                result: crate::backtest::run(
                    t.market(),
                    t.universe(),
                    Config::default(),
                    &mut s,
                    &syms,
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_exact() {
        assert_eq!(
            Dec::parse("1.1215"),
            Ok(Dec {
                mantissa: 11215,
                scale: 4
            })
        );
        assert_eq!(
            Dec::parse("363.4").unwrap().normalized(),
            Dec::parse("363.40").unwrap().normalized()
        );
        for bad in ["", ".5", "1.", "-1", "1e3", "N/A", "1,2"] {
            assert!(Dec::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(ecb_price_cents("1.1215", 100), Ok(11215));
        assert_eq!(ecb_price_cents("0.84028", 1000), Ok(84028));
        assert!(
            ecb_price_cents("0.84028", 100).is_err(),
            "must refuse to round"
        );
        assert_eq!(us_date_to_iso("10/20/2026").unwrap(), "2026-10-20");
        assert!(us_date_to_iso("2026-10-20").is_err());
    }

    #[test]
    fn zc_proxy_sane() {
        assert_eq!(zc_price_cents(0, 10), 10_000);
        // 5.12 % 10y semi-annual: 100 / 1.0256^20 = 60.317 -> 60.32
        assert_eq!(zc_price_cents(512, 10), 6032);
        assert!(zc_price_cents(513, 10) < zc_price_cents(512, 10));
        assert!(zc_price_cents(479, 2) > zc_price_cents(479, 30));
    }

    #[test]
    fn tape_parse_names() {
        for t in Tape::ALL {
            assert_eq!(Tape::parse(t.name()), Ok(t));
        }
        assert!(Tape::parse("yahoo").is_err());
    }
}
