// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Backtest replay: walk the fixture bar by bar, ask the strategy, route its
//! signals through the same paper broker and risk limits as a live ticket, and
//! close everything on the last bar. Same fixture + same strategy = same
//! fingerprint, on every machine.

use crate::broker::{Config, OrderRequest, Outcome, PaperBroker};
use crate::market::{Feed, MarketData};
use crate::money::ratio_bps;
use crate::report::fingerprint;
use crate::strategy::Strategy;
use crate::universe::Universe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    pub date: String,
    pub equity_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacktestResult {
    pub strategy: String,
    pub symbols: Vec<String>,
    pub first_day: String,
    pub last_day: String,
    pub trades: usize,
    pub refusals: usize,
    pub final_equity_cents: i64,
    pub pnl_cents: i64,
    pub return_bps: i64,
    pub max_drawdown_bps: i64,
    pub fees_cents: i64,
    pub equity_curve: Vec<EquityPoint>,
    pub fingerprint: String,
}

pub fn run(
    md: MarketData,
    universe: Universe,
    cfg: Config,
    strat: &mut dyn Strategy,
    symbols: &[&str],
) -> BacktestResult {
    let mut b = PaperBroker::new(Feed::new(md), universe, cfg);
    let ticket = format!("backtest:{}", strat.name());
    let mut curve = Vec::new();
    let first_day = b.today();
    loop {
        for s in symbols {
            let Some(h) = b.feed.history(s) else { continue };
            let h = h.to_vec();
            if let Some(sig) = strat.on_bar(&h, b.position(s)) {
                let _ = b.place(OrderRequest {
                    symbol: (*s).into(),
                    side: Some(sig.side),
                    qty: sig.qty,
                    limit: None,
                    rationale: format!("{} signal", strat.name()),
                    ticket: ticket.clone(),
                });
            }
        }
        curve.push(EquityPoint {
            date: b.today(),
            equity_cents: b.account().equity_cents,
        });
        if b.feed.at_end() {
            break;
        }
        b.advance();
    }
    for s in symbols {
        if b.position(s) > 0 {
            if let Outcome::Filled { .. } = b.close(s, &ticket) {}
        }
    }
    let a = b.account();
    if let Some(last) = curve.last_mut() {
        last.equity_cents = a.equity_cents;
    }
    let mut peak = i64::MIN;
    let mut mdd = 0;
    for p in &curve {
        peak = peak.max(p.equity_cents);
        mdd = mdd.max(ratio_bps(peak - p.equity_cents, peak));
    }
    let mut r = BacktestResult {
        strategy: strat.name(),
        symbols: symbols.iter().map(|s| s.to_string()).collect(),
        first_day,
        last_day: b.today(),
        trades: b.fills.len(),
        refusals: b.refusals.len(),
        final_equity_cents: a.equity_cents,
        pnl_cents: a.pnl_cents,
        return_bps: a.return_bps,
        max_drawdown_bps: mdd,
        fees_cents: a.fees_cents,
        equity_curve: curve,
        fingerprint: String::new(),
    };
    r.fingerprint = fingerprint(&serde_json::to_value(&r).unwrap_or_default());
    r
}

/// The reference backtest used by the CLI, HTTP and scoreboard.
pub fn reference() -> BacktestResult {
    let mut s = crate::strategy::SmaCross {
        fast: 3,
        slow: 8,
        qty: 100,
    };
    run(
        MarketData::fixture(),
        Universe::fixture(),
        Config::default(),
        &mut s,
        &["GLXA", "OMND", "POSD"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_is_deterministic_and_trades() {
        let a = reference();
        let b = reference();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert!(a.trades >= 2);
        assert_eq!(a.equity_curve.len(), 40);
        assert!(a.max_drawdown_bps >= 0);
    }
}
