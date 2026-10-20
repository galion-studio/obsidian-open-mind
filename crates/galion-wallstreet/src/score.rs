// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! B2 Wall Street scoreboard. Every axis is a list of named, deterministic
//! checks run in-process on the committed fixtures. Score = passed / total.
//! Nothing is weighted by hand, nothing is skipped silently.

use crate::backtest;
use crate::broker::{OrderRequest, PaperBroker, Side};
use crate::compliance::{self, Verdict};
use crate::market::{self, MarketData};
use crate::news;
use crate::report;
use crate::strategy::Scratchpad;
use crate::ticket;
use crate::universe::Universe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub pass: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Axis {
    pub axis: &'static str,
    pub passed: usize,
    pub total: usize,
    pub percent: u32,
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scoreboard {
    pub bench: &'static str,
    pub mode: &'static str,
    pub live_broker: bool,
    pub passed: usize,
    pub total: usize,
    pub percent: u32,
    pub axes: Vec<Axis>,
    pub ticket_fingerprint: String,
    pub backtest_fingerprint: String,
    pub generated_at: String,
}

struct Acc(Vec<Check>);
impl Acc {
    fn c(&mut self, name: &str, pass: bool, detail: impl Into<String>) {
        self.0.push(Check {
            name: name.into(),
            pass,
            detail: detail.into(),
        });
    }
}

fn axis(name: &'static str, f: impl FnOnce(&mut Acc)) -> Axis {
    let mut a = Acc(Vec::new());
    f(&mut a);
    let passed = a.0.iter().filter(|c| c.pass).count();
    let total = a.0.len();
    Axis {
        axis: name,
        passed,
        total,
        percent: pct(passed, total),
        checks: a.0,
    }
}

fn pct(p: usize, t: usize) -> u32 {
    (p * 100).checked_div(t).unwrap_or(0) as u32
}

fn req(sym: &str, side: Side, qty: i64, limit: Option<i64>) -> OrderRequest {
    OrderRequest {
        symbol: sym.into(),
        side: Some(side),
        qty,
        limit,
        rationale: "scoreboard".into(),
        ticket: "SCORE".into(),
    }
}

pub fn run() -> Scoreboard {
    let mut axes = Vec::new();

    axes.push(axis("market_data_ingest", |a| {
        match market::ingest(market::BARS_FIXTURE) {
            Ok(md) => {
                a.c(
                    "fixture_ingests",
                    true,
                    format!("{} days x {} symbols", md.days.len(), md.symbols().count()),
                );
                a.c(
                    "fixture_labelled_synthetic",
                    md.source.contains("SYNTHETIC"),
                    md.source.clone(),
                );
                a.c(
                    "delay_declared",
                    md.delay_bars >= 1,
                    format!("delay_bars={}", md.delay_bars),
                );
            }
            Err(e) => a.c("fixture_ingests", false, e),
        }
        for (name, csv) in market::malformed_cases() {
            a.c(
                &format!("refuses {name}"),
                market::ingest(&csv).is_err(),
                "",
            );
        }
        let b = PaperBroker::fixture();
        let q = b.feed.quote("GLXA");
        a.c(
            "quote_is_delayed",
            q.as_ref().is_some_and(|q| q.delayed),
            format!("{q:?}"),
        );
    }));

    axes.push(axis("symbol_universe", |a| {
        let u = Universe::fixture();
        let md = MarketData::fixture();
        a.c(
            "universe_loads",
            u.symbols.len() == 7,
            format!("{} symbols", u.symbols.len()),
        );
        a.c(
            "every_symbol_has_data",
            u.missing_data(&md).is_empty(),
            format!("{:?}", u.missing_data(&md)),
        );
        let mut b = PaperBroker::fixture();
        a.c(
            "unknown_refused",
            b.place(req("AAPL", Side::Buy, 1, None)).code() == "unknown_symbol",
            "",
        );
        a.c(
            "restricted_refused",
            b.place(req("RSTR", Side::Buy, 1, None)).code() == "restricted_symbol",
            "",
        );
        a.c(
            "halted_refused",
            b.place(req("HLTD", Side::Buy, 1, None)).code() == "halted_symbol",
            "",
        );
        a.c(
            "tradable_accepted",
            b.place(req("ZGRB", Side::Buy, 1, None)).code() == "filled",
            "",
        );
    }));

    axes.push(axis("paper_order_book", |a| {
        let mut b = PaperBroker::fixture();
        let o = b.place(req("GLXA", Side::Buy, 50, None));
        a.c("market_buy_fills", o.code() == "filled", o.code());
        let q = b.feed.quote("HRMS").map_or(0, |q| q.price_cents);
        let o = b.place(req("HRMS", Side::Buy, 10, Some(q - 400)));
        a.c("passive_limit_rests", o.code() == "resting", o.code());
        let id = b.open_orders().first().map_or(0, |o| o.order_id);
        a.c(
            "cancel_works",
            b.cancel(id).is_ok() && b.open_orders().is_empty(),
            "",
        );
        let next_low = b.feed.data.series("HRMS").map_or(0, |s| s[1].low);
        let _ = b.place(req("HRMS", Side::Buy, 10, Some(next_low)));
        let _ = b.advance();
        a.c(
            "limit_fills_on_next_bar",
            b.position("HRMS") == 10,
            format!("HRMS pos {}", b.position("HRMS")),
        );
        a.c(
            "no_short_selling",
            b.place(req("OMND", Side::Sell, 1, None)).code() == "no_short",
            "",
        );
        a.c(
            "partial_sell",
            b.place(req("GLXA", Side::Sell, 20, None)).code() == "filled"
                && b.position("GLXA") == 30,
            "",
        );
        let acc = b.account();
        a.c(
            "ledger_reconciles",
            acc.realized_cents + acc.unrealized_cents - acc.fees_cents == acc.pnl_cents,
            format!("pnl {}", acc.pnl_cents),
        );
    }));

    axes.push(axis("risk_limits", |a| {
        let mut b = PaperBroker::fixture();
        a.c(
            "within_limits_fills",
            b.place(req("GLXA", Side::Buy, 200, None)).code() == "filled",
            "",
        );
        a.c(
            "max_position_refused",
            b.place(req("ZGRB", Side::Buy, 1_001, None)).code() == "max_position",
            "",
        );
        a.c("max_position_does_not_trip", !b.kill.engaged, "");
        let o = b.place(req("GLXA", Side::Buy, 100_000, None));
        a.c(
            "oversized_refused_by_kill_switch",
            o.code() == "kill_switch_oversized_order",
            o.code(),
        );
        a.c(
            "kill_switch_engaged_after_oversize",
            b.kill.engaged,
            format!("{:?}", b.kill.reason),
        );
        a.c(
            "kill_switch_blocks_new_risk",
            b.place(req("HRMS", Side::Buy, 1, None)).code() == "kill_switch_engaged",
            "",
        );
        a.c(
            "kill_switch_allows_close",
            b.close("GLXA", "SCORE").code() == "filled",
            "",
        );
        a.c(
            "reset_requires_operator",
            b.reset_kill("score-operator") && !b.kill.engaged,
            "",
        );
        a.c(
            "trades_after_reset",
            b.place(req("HRMS", Side::Buy, 1, None)).code() == "filled",
            "",
        );
        let mut g = PaperBroker::fixture();
        let _ = g.place(req("OMND", Side::Buy, 200, None));
        let _ = g.place(req("GLXA", Side::Buy, 400, None));
        let o = g.place(req("POSD", Side::Buy, 300, None));
        a.c(
            "gross_exposure_refused",
            o.code() == "max_gross_exposure",
            o.code(),
        );
        let mut m = PaperBroker::fixture();
        let _ = m.place(req("GLXA", Side::Buy, 10, Some(1_000)));
        m.engage_kill("score manual kill");
        a.c(
            "manual_kill_cancels_resting_buys",
            m.kill.engaged && m.open_orders().is_empty(),
            "",
        );
    }));

    axes.push(axis("strategy_scratchpad", |a| {
        let mut p = Scratchpad::default();
        a.c(
            "note_recorded",
            p.add(
                "2026-08-10",
                "T",
                "thesis from public earnings",
                &["thesis".into()],
            )
            .is_ok(),
            "",
        );
        a.c(
            "tip_note_refused",
            p.add("2026-08-10", "T", "hot tip from a chat room", &[])
                .is_err(),
            "",
        );
        a.c(
            "notes_scoped_to_ticket",
            p.for_ticket("T").len() == 1 && p.for_ticket("X").is_empty(),
            "",
        );
        let t = ticket::parse(ticket::SCRIPTED_TICKET).ok();
        let r = t.map(|t| ticket::run_fresh(&t));
        let n = r.as_ref().map_or(0, |r| r.report.scratchpad.len());
        a.c("notes_in_report", n == 2, format!("{n} notes"));
        let md = r.map(|r| report::markdown(&r.report)).unwrap_or_default();
        a.c("notes_in_markdown", md.contains("GLXA beat earnings"), "");
    }));

    let bt = backtest::reference();
    axes.push(axis("backtest_replay", |a| {
        let again = backtest::reference();
        a.c(
            "deterministic_fingerprint",
            bt.fingerprint == again.fingerprint,
            bt.fingerprint.clone(),
        );
        a.c(
            "reference_run_clean",
            bt.refusals == 0,
            format!("{} refusals", bt.refusals),
        );
        a.c(
            "trades_generated",
            bt.trades >= 2,
            format!("{} fills", bt.trades),
        );
        a.c(
            "full_calendar_replayed",
            bt.equity_curve.len() == 40,
            format!("{} points", bt.equity_curve.len()),
        );
        a.c(
            "flat_at_end",
            bt.final_equity_cents == bt.equity_curve.last().map_or(-1, |p| p.equity_cents),
            "",
        );
        a.c(
            "drawdown_reported",
            bt.max_drawdown_bps >= 0,
            format!("{} bps", bt.max_drawdown_bps),
        );
        let mut cfg = crate::broker::Config::default();
        cfg.limits.max_position_qty = 100;
        let mut s = crate::strategy::SmaCross {
            fast: 3,
            slow: 8,
            qty: 200,
        };
        let tight = backtest::run(
            MarketData::fixture(),
            Universe::fixture(),
            cfg,
            &mut s,
            &["GLXA"],
        );
        a.c(
            "replay_uses_same_risk_limits",
            tight.trades == 0 && tight.refusals > 0,
            format!(
                "tight limits: {} fills, {} refusals",
                tight.trades, tight.refusals
            ),
        );
    }));

    axes.push(axis("news_event_tagging", |a| {
        let items = news::fixture();
        let want = news::expected_fixture_tags();
        a.c(
            "fixture_parses",
            items.len() == want.len(),
            format!("{} items", items.len()),
        );
        for (it, w) in items.iter().zip(want) {
            let got = news::tag(it).tags;
            a.c(
                &format!("tags {} {}", it.date, it.symbol),
                got == w,
                format!("{got:?}"),
            );
        }
        let ctx = news::context(&items, "RSTR", "2026-10-02");
        let rstr: Vec<_> = ctx.iter().filter(|t| t.item.symbol == "RSTR").collect();
        a.c(
            "mnpi_item_not_usable",
            !rstr.is_empty()
                && rstr
                    .iter()
                    .all(|t| !t.usable_as_reason && t.tags.contains(&news::Tag::MnpiRisk)),
            format!("{} RSTR items", rstr.len()),
        );
        a.c(
            "no_lookahead",
            news::context(&items, "GLXA", "2026-08-11").is_empty(),
            "",
        );
    }));

    axes.push(axis("compliance_refusals", |a| {
        for (text, want) in compliance::corpus() {
            let v = compliance::screen(text);
            let got = match &v {
                Verdict::Allow => None,
                Verdict::Refuse { category, .. } => Some(*category),
            };
            a.c(text, got == want, format!("{got:?}"));
        }
        let r = ticket::parse(ticket::INSIDER_TICKET).map(|t| ticket::run_fresh(&t));
        a.c(
            "insider_ticket_refused_before_trading",
            r.as_ref()
                .is_ok_and(|r| r.status == "refused" && r.report.fills.is_empty()),
            "",
        );
        let mut b = PaperBroker::fixture();
        let mut o = req("GLXA", Side::Buy, 1, None);
        o.rationale = "it's a sure thing".into();
        a.c(
            "tip_rationale_refused_at_order",
            b.place(o).code() == "compliance_stock_tip",
            "",
        );
        let alt_ok = compliance::corpus()
            .iter()
            .all(|(t, _)| match compliance::screen(t) {
                Verdict::Refuse { alternative, .. } => !alternative.is_empty(),
                Verdict::Allow => true,
            });
        a.c("every_refusal_offers_alternative", alt_ok, "");
    }));

    let tr = ticket::parse(ticket::SCRIPTED_TICKET).map(|t| ticket::run_fresh(&t));
    let tfp = tr
        .as_ref()
        .map(|r| r.report.fingerprint.clone())
        .unwrap_or_default();
    axes.push(axis("report_quality", |a| match &tr {
        Ok(r) => {
            for (n, ok, d) in report::quality(&r.report) {
                a.c(&n, ok, d);
            }
            let again = ticket::parse(ticket::SCRIPTED_TICKET).map(|t| ticket::run_fresh(&t));
            a.c(
                "ticket_report_reproducible",
                again.is_ok_and(|g| g.report.fingerprint == r.report.fingerprint),
                r.report.fingerprint.clone(),
            );
            a.c(
                "ticket_flat_at_end",
                r.report.account.positions.iter().all(|p| p.qty == 0),
                "",
            );
        }
        Err(e) => a.c("ticket_parses", false, e.clone()),
    }));

    let passed = axes.iter().map(|a| a.passed).sum();
    let total = axes.iter().map(|a| a.total).sum();
    Scoreboard {
        bench: "B2 wallstreet",
        mode: "paper",
        live_broker: false,
        passed,
        total,
        percent: pct(passed, total),
        axes,
        ticket_fingerprint: tfp,
        backtest_fingerprint: bt.fingerprint.clone(),
        generated_at: zagreb_time::now_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scoreboard_is_100() {
        let s = super::run();
        let failing: Vec<String> = s
            .axes
            .iter()
            .flat_map(|a| {
                a.checks
                    .iter()
                    .filter(|c| !c.pass)
                    .map(move |c| format!("{}: {} {}", a.axis, c.name, c.detail))
            })
            .collect();
        assert!(failing.is_empty(), "{failing:#?}");
        assert_eq!(s.percent, 100);
    }
}
