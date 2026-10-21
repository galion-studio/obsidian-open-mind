// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Market data ingest from recorded fixtures, and a delayed feed over it.
//!
//! There is no network client in this crate. Bars come from a CSV fixture
//! (`fixtures/bars.csv`, synthetic, committed) and the feed only ever shows a
//! bar after it is complete: the "current" quote is the close of the last
//! visible bar, stamped `delayed: true` with the fixture's source line.

use crate::money::parse_cents;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// The committed synthetic bar fixture.
pub const BARS_FIXTURE: &str = include_str!("../fixtures/bars.csv");

const HEADER: &str = "date,symbol,open,high,low,close,volume";

/// One daily OHLCV bar, prices in cents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bar {
    pub date: String,
    pub symbol: String,
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub close: i64,
    pub volume: u64,
}

/// Validated, aligned bars for every symbol in the fixture.
#[derive(Debug, Clone)]
pub struct MarketData {
    pub source: String,
    pub delay_bars: usize,
    pub days: Vec<String>,
    bars: BTreeMap<String, Vec<Bar>>,
}

fn valid_date(d: &str) -> bool {
    let b = d.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9]
        .iter()
        .all(|&i| b[i].is_ascii_digit());
    if !digits {
        return false;
    }
    let m: u32 = d[5..7].parse().unwrap_or(0);
    let day: u32 = d[8..10].parse().unwrap_or(0);
    (1..=12).contains(&m) && (1..=31).contains(&day)
}

/// 1-6 chars, uppercase letter first, then uppercase letters or digits (UST10Y).
fn valid_symbol(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 6
        && b[0].is_ascii_uppercase()
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Parse and validate a bar CSV. Every problem names its line number.
pub fn ingest(csv: &str) -> Result<MarketData, String> {
    let mut source = String::from("unknown fixture");
    let mut delay_bars = 1usize;
    let mut header_seen = false;
    let mut bars: BTreeMap<String, Vec<Bar>> = BTreeMap::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for (i, raw) in csv.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(c) = line.strip_prefix('#') {
            let c = c.trim();
            if let Some(v) = c.strip_prefix("source:") {
                source = v.trim().to_string();
            } else if let Some(v) = c.strip_prefix("delay_bars:") {
                delay_bars = v
                    .trim()
                    .parse()
                    .map_err(|_| format!("line {n}: bad delay_bars"))?;
            }
            continue;
        }
        if !header_seen {
            if line != HEADER {
                return Err(format!("line {n}: expected header {HEADER:?}"));
            }
            header_seen = true;
            continue;
        }
        let f: Vec<&str> = line.split(',').collect();
        if f.len() != 7 {
            return Err(format!("line {n}: expected 7 fields, got {}", f.len()));
        }
        let (date, sym) = (f[0].trim(), f[1].trim());
        if !valid_date(date) {
            return Err(format!("line {n}: bad date {date:?}"));
        }
        if !valid_symbol(sym) {
            return Err(format!("line {n}: bad symbol {sym:?}"));
        }
        let p = |k: usize| parse_cents(f[k]).map_err(|e| format!("line {n}: {e}"));
        let (open, high, low, close) = (p(2)?, p(3)?, p(4)?, p(5)?);
        let volume: u64 = f[6]
            .trim()
            .parse()
            .map_err(|_| format!("line {n}: bad volume"))?;
        if low <= 0 {
            return Err(format!("line {n}: non-positive low"));
        }
        if high < open.max(close) || low > open.min(close) || high < low {
            return Err(format!("line {n}: OHLC out of order"));
        }
        if !seen.insert((sym.to_string(), date.to_string())) {
            return Err(format!("line {n}: duplicate bar {sym} {date}"));
        }
        let v = bars.entry(sym.to_string()).or_default();
        if let Some(last) = v.last() {
            if last.date.as_str() >= date {
                return Err(format!("line {n}: {sym} dates not increasing"));
            }
        }
        v.push(Bar {
            date: date.into(),
            symbol: sym.into(),
            open,
            high,
            low,
            close,
            volume,
        });
    }
    if !header_seen {
        return Err("no header".into());
    }
    let first = bars.values().next().ok_or("no bars")?;
    let days: Vec<String> = first.iter().map(|b| b.date.clone()).collect();
    for (sym, v) in &bars {
        let d: Vec<&str> = v.iter().map(|b| b.date.as_str()).collect();
        if d != days.iter().map(String::as_str).collect::<Vec<_>>() {
            return Err(format!("{sym}: calendar not aligned with other symbols"));
        }
    }
    Ok(MarketData {
        source,
        delay_bars,
        days,
        bars,
    })
}

impl MarketData {
    /// The committed fixture; panics only if the committed file is broken,
    /// which the unit tests catch.
    pub fn fixture() -> MarketData {
        ingest(BARS_FIXTURE).expect("committed bars fixture is valid")
    }
    pub fn symbols(&self) -> impl Iterator<Item = &String> {
        self.bars.keys()
    }
    pub fn series(&self, sym: &str) -> Option<&[Bar]> {
        self.bars.get(sym).map(Vec::as_slice)
    }
}

/// A delayed quote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quote {
    pub symbol: String,
    pub price_cents: i64,
    pub price: String,
    pub as_of: String,
    pub delayed: bool,
    pub delay_bars: usize,
    pub source: String,
}

/// Cursor over the fixture calendar. `cursor` is the last *visible* bar.
#[derive(Debug, Clone)]
pub struct Feed {
    pub data: MarketData,
    pub cursor: usize,
}

impl Feed {
    pub fn new(data: MarketData) -> Feed {
        Feed { data, cursor: 0 }
    }
    pub fn today(&self) -> &str {
        &self.data.days[self.cursor]
    }
    pub fn at_end(&self) -> bool {
        self.cursor + 1 >= self.data.days.len()
    }
    /// Move to a given date (must exist in the calendar).
    pub fn seek(&mut self, date: &str) -> Result<(), String> {
        let i = self
            .data
            .days
            .iter()
            .position(|d| d == date)
            .ok_or_else(|| format!("date {date} not in fixture calendar"))?;
        self.cursor = i;
        Ok(())
    }
    /// Reveal the next bar. Returns false at the end of the fixture.
    pub fn advance(&mut self) -> bool {
        if self.at_end() {
            false
        } else {
            self.cursor += 1;
            true
        }
    }
    pub fn bar(&self, sym: &str) -> Option<&Bar> {
        self.data.series(sym).map(|s| &s[self.cursor])
    }
    pub fn history(&self, sym: &str) -> Option<&[Bar]> {
        self.data.series(sym).map(|s| &s[..=self.cursor])
    }
    pub fn quote(&self, sym: &str) -> Option<Quote> {
        let b = self.bar(sym)?;
        Some(Quote {
            symbol: sym.into(),
            price_cents: b.close,
            price: crate::money::fmt_cents(b.close),
            as_of: b.date.clone(),
            delayed: true,
            delay_bars: self.data.delay_bars,
            source: self.data.source.clone(),
        })
    }
}

/// Malformed fixtures that ingest must refuse (used by tests and the scoreboard).
pub fn malformed_cases() -> Vec<(&'static str, String)> {
    let h = format!("{HEADER}\n");
    vec![
        ("missing header", "2026-08-10,GLXA,1,1,1,1,1\n".to_string()),
        ("wrong field count", format!("{h}2026-08-10,GLXA,1,1,1,1\n")),
        ("bad date", format!("{h}2026-13-10,GLXA,1,1,1,1,1\n")),
        (
            "high below close",
            format!("{h}2026-08-10,GLXA,1.00,1.00,0.90,1.10,5\n"),
        ),
        ("negative price", format!("{h}2026-08-10,GLXA,-1,1,1,1,1\n")),
        (
            "duplicate bar",
            format!("{h}2026-08-10,GLXA,1,1,1,1,1\n2026-08-10,GLXA,1,1,1,1,1\n"),
        ),
        (
            "dates going backwards",
            format!("{h}2026-08-11,GLXA,1,1,1,1,1\n2026-08-10,GLXA,1,1,1,1,1\n"),
        ),
        (
            "misaligned calendars",
            format!("{h}2026-08-10,GLXA,1,1,1,1,1\n2026-08-11,OMND,1,1,1,1,1\n"),
        ),
        (
            "lowercase symbol",
            format!("{h}2026-08-10,glxa,1,1,1,1,1\n"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_ingests() {
        let md = MarketData::fixture();
        assert_eq!(md.days.len(), 40);
        assert_eq!(md.symbols().count(), 7);
        assert_eq!(md.delay_bars, 1);
        assert!(md.source.contains("SYNTHETIC"));
    }

    #[test]
    fn malformed_refused() {
        for (name, csv) in malformed_cases() {
            assert!(ingest(&csv).is_err(), "{name} should be refused");
        }
    }

    #[test]
    fn feed_is_delayed_and_advances() {
        let mut f = Feed::new(MarketData::fixture());
        let q = f.quote("GLXA").unwrap();
        assert!(q.delayed);
        assert_eq!(q.as_of, "2026-08-10");
        assert!(f.advance());
        assert_eq!(f.quote("GLXA").unwrap().as_of, "2026-08-11");
        assert_eq!(f.history("GLXA").unwrap().len(), 2);
        assert!(f.quote("NOPE").is_none());
    }
}
