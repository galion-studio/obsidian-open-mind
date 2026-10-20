// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The symbol universe: which tickers exist, and which may be traded.

use crate::market::MarketData;
use serde::Serialize;
use std::collections::BTreeMap;

pub const UNIVERSE_FIXTURE: &str = include_str!("../fixtures/universe.csv");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Tradable,
    /// On the house restricted list: no research output, no orders.
    Restricted,
    /// Exchange halt in the fixture window.
    Halted,
}

#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub ticker: String,
    pub name: String,
    pub sector: String,
    pub status: Status,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Universe {
    pub symbols: BTreeMap<String, Symbol>,
}

/// Why a symbol cannot be traded right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolRefusal {
    Unknown,
    Restricted(String),
    Halted(String),
    NoData,
}

impl SymbolRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            SymbolRefusal::Unknown => "unknown_symbol",
            SymbolRefusal::Restricted(_) => "restricted_symbol",
            SymbolRefusal::Halted(_) => "halted_symbol",
            SymbolRefusal::NoData => "no_market_data",
        }
    }
    pub fn message(&self, t: &str) -> String {
        match self {
            SymbolRefusal::Unknown => format!("{t} is not in the paper universe"),
            SymbolRefusal::Restricted(n) => format!("{t} is restricted: {n}"),
            SymbolRefusal::Halted(n) => format!("{t} is halted: {n}"),
            SymbolRefusal::NoData => format!("{t} has no fixture market data"),
        }
    }
}

pub fn parse(csv: &str) -> Result<Universe, String> {
    let mut symbols = BTreeMap::new();
    let mut header = false;
    for (i, raw) in csv.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            if line != "ticker,name,sector,status,note" {
                return Err(format!("line {}: bad universe header", i + 1));
            }
            header = true;
            continue;
        }
        let f: Vec<&str> = line.splitn(5, ',').collect();
        if f.len() != 5 {
            return Err(format!("line {}: expected 5 fields", i + 1));
        }
        let status = match f[3] {
            "tradable" => Status::Tradable,
            "restricted" => Status::Restricted,
            "halted" => Status::Halted,
            s => return Err(format!("line {}: bad status {s:?}", i + 1)),
        };
        let s = Symbol {
            ticker: f[0].into(),
            name: f[1].into(),
            sector: f[2].into(),
            status,
            note: f[4].into(),
        };
        if symbols.insert(s.ticker.clone(), s).is_some() {
            return Err(format!("line {}: duplicate ticker", i + 1));
        }
    }
    if symbols.is_empty() {
        return Err("empty universe".into());
    }
    Ok(Universe { symbols })
}

impl Universe {
    pub fn fixture() -> Universe {
        parse(UNIVERSE_FIXTURE).expect("committed universe fixture is valid")
    }

    /// Every universe ticker must have bars; returns the tickers that do not.
    pub fn missing_data(&self, md: &MarketData) -> Vec<String> {
        self.symbols
            .keys()
            .filter(|t| md.series(t).is_none())
            .cloned()
            .collect()
    }

    pub fn check_tradable(&self, t: &str, md: &MarketData) -> Result<&Symbol, SymbolRefusal> {
        let s = self.symbols.get(t).ok_or(SymbolRefusal::Unknown)?;
        match s.status {
            Status::Restricted => return Err(SymbolRefusal::Restricted(s.note.clone())),
            Status::Halted => return Err(SymbolRefusal::Halted(s.note.clone())),
            Status::Tradable => {}
        }
        if md.series(t).is_none() {
            return Err(SymbolRefusal::NoData);
        }
        Ok(s)
    }

    pub fn is_restricted(&self, t: &str) -> bool {
        self.symbols
            .get(t)
            .is_some_and(|s| s.status == Status::Restricted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_universe() {
        let u = Universe::fixture();
        let md = MarketData::fixture();
        assert_eq!(u.symbols.len(), 7);
        assert!(u.missing_data(&md).is_empty());
        assert!(u.check_tradable("GLXA", &md).is_ok());
        assert_eq!(
            u.check_tradable("RSTR", &md).unwrap_err().code(),
            "restricted_symbol"
        );
        assert_eq!(
            u.check_tradable("HLTD", &md).unwrap_err().code(),
            "halted_symbol"
        );
        assert_eq!(
            u.check_tradable("AAPL", &md).unwrap_err().code(),
            "unknown_symbol"
        );
    }
}
