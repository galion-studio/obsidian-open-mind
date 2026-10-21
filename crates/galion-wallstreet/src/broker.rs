// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The paper broker: a cash account, a position book, resting limit orders,
//! fills against delayed fixture bars, and a ledger. Nothing here can reach a
//! real venue: there is no network code, no credential, no account id.
//!
//! Fill model (documented so a P&L can be reproduced by hand):
//! - Market order: fills now at the last visible close, plus `slippage_bps`
//!   against you (buy higher, sell lower).
//! - Limit order: if marketable at the last close it fills like a market order
//!   but never through the limit. Otherwise it rests and is checked against the
//!   *next* revealed bar: a buy fills when that bar's low touches the limit, at
//!   min(open, limit); a sell when the high touches it, at max(open, limit).
//! - Fee: `fee_bps` of notional, rounded half away from zero, paid from cash.
//! - Long only, no margin: a sell may not exceed the free position, a buy may
//!   not exceed free cash.

use crate::compliance::{self, Verdict};
use crate::market::Feed;
use crate::money::{apply_bps, fmt_cents};
use crate::risk::{self, Intent, KillSwitch, RiskLimits};
use crate::universe::Universe;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    fn sign(self) -> i64 {
        match self {
            Side::Buy => 1,
            Side::Sell => -1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub starting_cash: i64,
    pub fee_bps: i64,
    pub slippage_bps: i64,
    pub limits: RiskLimits,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            starting_cash: 10_000_000, // 100 000.00 paper
            fee_bps: 5,
            slippage_bps: 5,
            limits: RiskLimits::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: Option<Side>,
    pub qty: i64,
    pub limit: Option<i64>,
    pub rationale: String,
    pub ticket: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fill {
    pub order_id: u64,
    pub date: String,
    pub symbol: String,
    pub side: Side,
    pub qty: i64,
    pub price_cents: i64,
    pub price: String,
    pub notional_cents: i64,
    pub fee_cents: i64,
    pub ticket: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestingOrder {
    pub order_id: u64,
    pub placed: String,
    pub symbol: String,
    pub side: Side,
    pub qty: i64,
    pub limit_cents: i64,
    pub limit: String,
    pub ticket: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: String,
    pub message: String,
    pub date: String,
    pub symbol: String,
    pub ticket: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Filled { fill: Fill },
    Resting { order: RestingOrder },
    Refused { refusal: Refusal },
}

impl Outcome {
    pub fn code(&self) -> &str {
        match self {
            Outcome::Filled { .. } => "filled",
            Outcome::Resting { .. } => "resting",
            Outcome::Refused { refusal } => &refusal.code,
        }
    }
    pub fn is_refused(&self) -> bool {
        matches!(self, Outcome::Refused { .. })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Position {
    pub qty: i64,
    pub cost_cents: i64,
    pub realized_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub date: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PositionView {
    pub symbol: String,
    pub qty: i64,
    pub avg_cost: String,
    pub last: String,
    pub market_value_cents: i64,
    pub unrealized_cents: i64,
    pub realized_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    pub mode: &'static str,
    pub as_of: String,
    pub starting_cash_cents: i64,
    pub cash_cents: i64,
    pub equity_cents: i64,
    pub gross_exposure_cents: i64,
    pub realized_cents: i64,
    pub unrealized_cents: i64,
    pub fees_cents: i64,
    pub pnl_cents: i64,
    pub return_bps: i64,
    pub positions: Vec<PositionView>,
    pub open_orders: Vec<RestingOrder>,
    pub kill_switch: KillSwitch,
}

#[derive(Debug, Clone)]
pub struct PaperBroker {
    pub feed: Feed,
    pub universe: Universe,
    pub cfg: Config,
    pub kill: KillSwitch,
    cash: i64,
    fees: i64,
    positions: BTreeMap<String, Position>,
    open: Vec<RestingOrder>,
    pub fills: Vec<Fill>,
    pub refusals: Vec<Refusal>,
    pub events: Vec<Event>,
    next_id: u64,
    /// Which data tape this broker replays (`/reset` keeps it).
    pub tape: crate::public::Tape,
}

impl PaperBroker {
    pub fn new(feed: Feed, universe: Universe, cfg: Config) -> PaperBroker {
        PaperBroker {
            feed,
            universe,
            cfg,
            kill: KillSwitch::default(),
            cash: cfg.starting_cash,
            fees: 0,
            positions: BTreeMap::new(),
            open: Vec::new(),
            fills: Vec::new(),
            refusals: Vec::new(),
            events: Vec::new(),
            next_id: 1,
            tape: crate::public::Tape::Synthetic,
        }
    }

    /// Fresh broker on the committed fixtures and default limits.
    pub fn fixture() -> PaperBroker {
        PaperBroker::new(
            Feed::new(crate::market::MarketData::fixture()),
            Universe::fixture(),
            Config::default(),
        )
    }

    pub fn today(&self) -> String {
        self.feed.today().to_string()
    }

    pub fn position(&self, sym: &str) -> i64 {
        self.positions.get(sym).map_or(0, |p| p.qty)
    }

    pub fn cash(&self) -> i64 {
        self.cash
    }

    pub fn open_orders(&self) -> &[RestingOrder] {
        &self.open
    }

    fn last(&self, sym: &str) -> i64 {
        self.feed.bar(sym).map_or(0, |b| b.close)
    }

    pub fn gross_exposure(&self) -> i64 {
        self.positions
            .iter()
            .map(|(s, p)| p.qty.abs() * self.last(s))
            .sum()
    }

    fn event(&mut self, kind: &str, detail: String) {
        let date = self.today();
        self.events.push(Event {
            date,
            kind: kind.into(),
            detail,
        });
    }

    fn refuse(&mut self, req: &OrderRequest, code: &str, message: String) -> Outcome {
        let r = Refusal {
            code: code.into(),
            message,
            date: self.today(),
            symbol: req.symbol.clone(),
            ticket: req.ticket.clone(),
        };
        self.refusals.push(r.clone());
        Outcome::Refused { refusal: r }
    }

    fn committed_sells(&self, sym: &str) -> i64 {
        self.open
            .iter()
            .filter(|o| o.symbol == sym && o.side == Side::Sell)
            .map(|o| o.qty)
            .sum()
    }

    fn committed_cash(&self) -> i64 {
        self.open
            .iter()
            .filter(|o| o.side == Side::Buy)
            .map(|o| {
                let n = o.qty * o.limit_cents;
                n + apply_bps(n, self.cfg.fee_bps)
            })
            .sum()
    }

    /// Place an order. Every refusal is recorded with a machine code.
    pub fn place(&mut self, req: OrderRequest) -> Outcome {
        let Some(side) = req.side else {
            return self.refuse(&req, "invalid_side", "side must be buy or sell".into());
        };
        if req.qty <= 0 {
            return self.refuse(
                &req,
                "invalid_qty",
                "qty must be a positive share count".into(),
            );
        }
        if let Verdict::Refuse { code, reason, .. } = compliance::screen(&req.rationale) {
            return self.refuse(&req, code, reason);
        }
        if let Err(e) = self.universe.check_tradable(&req.symbol, &self.feed.data) {
            let m = e.message(&req.symbol);
            return self.refuse(&req, e.code(), m);
        }
        let close = self.last(&req.symbol);
        let slip = |p: i64| match side {
            Side::Buy => p + apply_bps(p, self.cfg.slippage_bps),
            Side::Sell => p - apply_bps(p, self.cfg.slippage_bps),
        };
        let market_px = slip(close);
        let (marketable, px) = match (req.limit, side) {
            (None, _) => (true, market_px),
            (Some(l), _) if l <= 0 => {
                return self.refuse(&req, "invalid_limit", "limit must be positive".into())
            }
            (Some(l), Side::Buy) => (close <= l, market_px.min(l)),
            (Some(l), Side::Sell) => (close >= l, market_px.max(l)),
        };
        let px = if marketable {
            px
        } else {
            req.limit.unwrap_or(px)
        };
        if side == Side::Sell {
            let free = self.position(&req.symbol) - self.committed_sells(&req.symbol);
            if req.qty > free {
                return self.refuse(
                    &req,
                    "no_short",
                    format!(
                        "sell {} exceeds free position {free}; paper book is long only",
                        req.qty
                    ),
                );
            }
        }
        if !marketable && self.open.len() >= self.cfg.limits.max_open_orders {
            return self.refuse(&req, "max_open_orders", "too many resting orders".into());
        }
        let intent = Intent {
            delta: side.sign() * req.qty,
            price: px,
            position_before: self.position(&req.symbol),
            gross_before: self.gross_exposure(),
        };
        let today = self.today();
        let was_engaged = self.kill.engaged;
        if let Err(e) = risk::evaluate(&self.cfg.limits, &mut self.kill, &intent, &today) {
            if self.kill.engaged && !was_engaged {
                let n = self.cancel_opening_orders();
                self.event(
                    "kill_switch_tripped",
                    format!("{} (cancelled {n} resting buy orders)", e.message),
                );
            }
            return self.refuse(&req, e.code, e.message);
        }
        if side == Side::Buy {
            let n = req.qty * px;
            let need = n + apply_bps(n, self.cfg.fee_bps);
            let free = self.cash - self.committed_cash();
            if need > free {
                return self.refuse(
                    &req,
                    "insufficient_cash",
                    format!(
                        "needs {} but free cash is {}; no margin",
                        fmt_cents(need),
                        fmt_cents(free)
                    ),
                );
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        if marketable {
            let f = self.execute(id, &req.symbol, side, req.qty, px, &req.ticket);
            Outcome::Filled { fill: f }
        } else {
            let o = RestingOrder {
                order_id: id,
                placed: today,
                symbol: req.symbol.clone(),
                side,
                qty: req.qty,
                limit_cents: px,
                limit: fmt_cents(px),
                ticket: req.ticket.clone(),
                rationale: req.rationale.clone(),
            };
            self.open.push(o.clone());
            Outcome::Resting { order: o }
        }
    }

    fn execute(&mut self, id: u64, sym: &str, side: Side, qty: i64, px: i64, ticket: &str) -> Fill {
        let notional = qty * px;
        let fee = apply_bps(notional, self.cfg.fee_bps);
        let p = self.positions.entry(sym.to_string()).or_default();
        match side {
            Side::Buy => {
                p.qty += qty;
                p.cost_cents += notional;
                self.cash -= notional + fee;
            }
            Side::Sell => {
                let removed = (p.cost_cents as i128 * qty as i128 / p.qty as i128) as i64;
                p.realized_cents += notional - removed;
                p.cost_cents -= removed;
                p.qty -= qty;
                self.cash += notional - fee;
            }
        }
        self.fees += fee;
        let f = Fill {
            order_id: id,
            date: self.today(),
            symbol: sym.into(),
            side,
            qty,
            price_cents: px,
            price: fmt_cents(px),
            notional_cents: notional,
            fee_cents: fee,
            ticket: ticket.into(),
        };
        self.fills.push(f.clone());
        f
    }

    fn cancel_opening_orders(&mut self) -> usize {
        let before = self.open.len();
        self.open.retain(|o| o.side == Side::Sell);
        before - self.open.len()
    }

    pub fn cancel(&mut self, order_id: u64) -> Result<RestingOrder, String> {
        let i = self
            .open
            .iter()
            .position(|o| o.order_id == order_id)
            .ok_or_else(|| format!("no resting order {order_id}"))?;
        let o = self.open.remove(i);
        self.event(
            "cancel",
            format!("order {} {} {}", o.order_id, o.symbol, o.qty),
        );
        Ok(o)
    }

    /// Close a whole position with a market sell (always allowed: it reduces risk).
    pub fn close(&mut self, sym: &str, ticket: &str) -> Outcome {
        let ids: Vec<u64> = self
            .open
            .iter()
            .filter(|o| o.symbol == sym)
            .map(|o| o.order_id)
            .collect();
        for id in ids {
            let _ = self.cancel(id);
        }
        let qty = self.position(sym);
        let req = OrderRequest {
            symbol: sym.into(),
            side: Some(Side::Sell),
            qty,
            limit: None,
            rationale: "close position".into(),
            ticket: ticket.into(),
        };
        if qty == 0 {
            return self.refuse(&req, "no_position", format!("no open position in {sym}"));
        }
        self.place(req)
    }

    /// Operator kill: engage and cancel every resting buy.
    pub fn engage_kill(&mut self, reason: &str) {
        let today = self.today();
        self.kill.trip(format!("manual: {reason}"), &today);
        let n = self.cancel_opening_orders();
        self.event(
            "kill_switch_engaged",
            format!("manual: {reason} (cancelled {n} resting buy orders)"),
        );
    }

    pub fn reset_kill(&mut self, operator: &str) -> bool {
        if !self.kill.engaged {
            return false;
        }
        self.kill.reset();
        self.event("kill_switch_reset", format!("reset by {operator}"));
        true
    }

    /// Reveal the next bar and work resting orders against it.
    pub fn advance(&mut self) -> Vec<Outcome> {
        if !self.feed.advance() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let orders = std::mem::take(&mut self.open);
        for o in orders {
            let Some(bar) = self.feed.bar(&o.symbol).cloned() else {
                continue;
            };
            let px = match o.side {
                Side::Buy if bar.low <= o.limit_cents => Some(bar.open.min(o.limit_cents)),
                Side::Sell if bar.high >= o.limit_cents => Some(bar.open.max(o.limit_cents)),
                _ => None,
            };
            let Some(px) = px else {
                self.open.push(o);
                continue;
            };
            let intent = Intent {
                delta: o.side.sign() * o.qty,
                price: px,
                position_before: self.position(&o.symbol),
                gross_before: self.gross_exposure(),
            };
            let today = self.today();
            let req = OrderRequest {
                symbol: o.symbol.clone(),
                side: Some(o.side),
                qty: o.qty,
                limit: Some(o.limit_cents),
                rationale: o.rationale.clone(),
                ticket: o.ticket.clone(),
            };
            if let Err(e) = risk::evaluate(&self.cfg.limits, &mut self.kill, &intent, &today) {
                out.push(self.refuse(
                    &req,
                    e.code,
                    format!("resting order {} cancelled: {}", o.order_id, e.message),
                ));
                continue;
            }
            if o.side == Side::Buy {
                let n = o.qty * px;
                if n + apply_bps(n, self.cfg.fee_bps) > self.cash - self.committed_cash() {
                    out.push(self.refuse(
                        &req,
                        "insufficient_cash",
                        format!("resting order {} cancelled", o.order_id),
                    ));
                    continue;
                }
            }
            let f = self.execute(o.order_id, &o.symbol, o.side, o.qty, px, &o.ticket);
            out.push(Outcome::Filled { fill: f });
        }
        out
    }

    pub fn account(&self) -> Account {
        let mut positions = Vec::new();
        let (mut realized, mut unrealized) = (0, 0);
        for (s, p) in &self.positions {
            let last = self.last(s);
            let mv = p.qty * last;
            let u = mv - p.cost_cents;
            realized += p.realized_cents;
            unrealized += u;
            positions.push(PositionView {
                symbol: s.clone(),
                qty: p.qty,
                avg_cost: if p.qty == 0 {
                    "0.00".into()
                } else {
                    fmt_cents(p.cost_cents / p.qty)
                },
                last: fmt_cents(last),
                market_value_cents: mv,
                unrealized_cents: u,
                realized_cents: p.realized_cents,
            });
        }
        let mv: i64 = positions.iter().map(|p| p.market_value_cents).sum();
        let equity = self.cash + mv;
        let pnl = equity - self.cfg.starting_cash;
        Account {
            mode: "paper",
            as_of: self.today(),
            starting_cash_cents: self.cfg.starting_cash,
            cash_cents: self.cash,
            equity_cents: equity,
            gross_exposure_cents: self.gross_exposure(),
            realized_cents: realized,
            unrealized_cents: unrealized,
            fees_cents: self.fees,
            pnl_cents: pnl,
            return_bps: crate::money::ratio_bps(pnl, self.cfg.starting_cash),
            positions,
            open_orders: self.open.clone(),
            kill_switch: self.kill.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buy(sym: &str, qty: i64, limit: Option<i64>) -> OrderRequest {
        OrderRequest {
            symbol: sym.into(),
            side: Some(Side::Buy),
            qty,
            limit,
            rationale: "test".into(),
            ticket: "T".into(),
        }
    }

    #[test]
    fn market_fill_and_round_trip_reconciles() {
        let mut b = PaperBroker::fixture();
        let o = b.place(buy("GLXA", 100, None));
        assert_eq!(o.code(), "filled");
        for _ in 0..5 {
            b.advance();
        }
        assert_eq!(b.close("GLXA", "T").code(), "filled");
        let a = b.account();
        assert_eq!(
            a.realized_cents + a.unrealized_cents - a.fees_cents,
            a.pnl_cents
        );
        assert_eq!(b.position("GLXA"), 0);
        assert!(a.realized_cents > 0, "GLXA trends up in the fixture");
    }

    #[test]
    fn limit_rests_then_fills_or_cancels() {
        let mut b = PaperBroker::fixture();
        let q = b.feed.quote("HRMS").unwrap().price_cents;
        let o = b.place(buy("HRMS", 10, Some(q - 500)));
        assert_eq!(o.code(), "resting");
        assert_eq!(b.open_orders().len(), 1);
        let id = b.open_orders()[0].order_id;
        assert!(b.cancel(id).is_ok());
        assert!(b.open_orders().is_empty());
        // A limit at the next bar's high must fill on advance.
        let next_high = b.feed.data.series("HRMS").unwrap()[1].high;
        let low_limit = b.feed.data.series("HRMS").unwrap()[1].low;
        assert!(next_high >= low_limit);
        let o = b.place(buy("HRMS", 10, Some(low_limit)));
        if o.code() == "resting" {
            let r = b.advance();
            assert_eq!(r.len(), 1);
            assert_eq!(r[0].code(), "filled");
        }
        assert_eq!(b.position("HRMS"), 10);
    }

    #[test]
    fn refusals() {
        let mut b = PaperBroker::fixture();
        assert_eq!(b.place(buy("RSTR", 1, None)).code(), "restricted_symbol");
        assert_eq!(b.place(buy("HLTD", 1, None)).code(), "halted_symbol");
        assert_eq!(b.place(buy("NOPE", 1, None)).code(), "unknown_symbol");
        assert_eq!(b.place(buy("GLXA", 0, None)).code(), "invalid_qty");
        let mut s = buy("GLXA", 5, None);
        s.side = Some(Side::Sell);
        assert_eq!(b.place(s).code(), "no_short");
        let mut r = buy("GLXA", 5, None);
        r.rationale = "my friend at Galaxia says earnings leak".into();
        assert_eq!(b.place(r).code(), "compliance_insider_information");
        assert_eq!(b.close("OMND", "T").code(), "no_position");
        assert_eq!(b.refusals.len(), 7);
    }

    #[test]
    fn insufficient_cash_without_margin() {
        let cfg = Config {
            starting_cash: 100_000,
            ..Config::default()
        };
        let mut b = PaperBroker::new(
            Feed::new(crate::market::MarketData::fixture()),
            Universe::fixture(),
            cfg,
        );
        assert_eq!(b.place(buy("OMND", 10, None)).code(), "insufficient_cash");
    }

    #[test]
    fn kill_switch_refuses_oversized_then_allows_close() {
        let mut b = PaperBroker::fixture();
        assert_eq!(b.place(buy("GLXA", 100, None)).code(), "filled");
        let q = b.feed.quote("ZGRB").unwrap().price_cents;
        assert_eq!(b.place(buy("ZGRB", 10, Some(q - 300))).code(), "resting");
        assert_eq!(
            b.place(buy("GLXA", 100_000, None)).code(),
            "kill_switch_oversized_order"
        );
        assert!(b.kill.engaged);
        assert!(b.open_orders().is_empty(), "trip cancels resting buys");
        assert_eq!(b.place(buy("HRMS", 1, None)).code(), "kill_switch_engaged");
        assert_eq!(b.close("GLXA", "T").code(), "filled");
        assert!(b.reset_kill("op"));
        assert_eq!(b.place(buy("HRMS", 1, None)).code(), "filled");
        assert!(b.events.iter().any(|e| e.kind == "kill_switch_tripped"));
    }
}
