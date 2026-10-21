// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The P&L report: JSON for machines, Markdown for Slavik.
//!
//! Reproducible: every number comes from integer cents and the fixture, and the
//! `fingerprint` (FNV-1a 64 over the canonical JSON *without* `generated_at`)
//! is identical for identical inputs. `generated_at` is Europe/Zagreb with an
//! explicit offset and is the only wall-clock field.

use crate::broker::{Account, Event, Fill, PaperBroker, Refusal};
use crate::money::fmt_cents;
use crate::news::{self, Tagged};
use crate::strategy::Note;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ticket: String,
    pub title: String,
    pub mode: &'static str,
    pub data_source: String,
    pub account: Account,
    pub fills: Vec<Fill>,
    pub refusals: Vec<Refusal>,
    pub risk_events: Vec<Event>,
    pub news_context: Vec<Tagged>,
    pub scratchpad: Vec<Note>,
    pub next_steps: Vec<String>,
    pub fingerprint: String,
    pub generated_at: String,
}

/// Render JSON with object keys sorted at every level, so the fingerprint does
/// not depend on serde_json's map feature flags (`preserve_order` or not).
fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                canonical(&m[k.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// FNV-1a 64 over the canonical (sorted-key) rendering, minus the two
/// self-describing fields `fingerprint` and `generated_at`.
pub fn fingerprint(v: &Value) -> String {
    let mut v = v.clone();
    if let Value::Object(m) = &mut v {
        m.remove("fingerprint");
        m.remove("generated_at");
    }
    let mut s = String::new();
    canonical(&v, &mut s);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("fnv1a64:{h:016x}")
}

fn next_steps(a: &Account, refusals: &[Refusal], news: &[Tagged]) -> Vec<String> {
    let mut v = Vec::new();
    if a.kill_switch.engaged {
        v.push(format!(
            "Kill switch is still engaged ({}). Review the refused order, then reset it deliberately before adding risk.",
            a.kill_switch.reason.as_deref().unwrap_or("no reason")
        ));
    }
    if refusals
        .iter()
        .any(|r| r.code.starts_with("kill_switch_oversized"))
    {
        v.push("An oversized order tripped the kill switch: check position sizing in the strategy before rerunning.".into());
    }
    if refusals.iter().any(|r| r.code.starts_with("compliance_")) {
        v.push("A compliance refusal fired: rework that idea from public sources only.".into());
    }
    if let Some(p) = a.positions.iter().find(|p| p.qty > 0) {
        v.push(format!(
            "Still holding {} {} (unrealised {}). Decide an exit rule or close it on the next ticket.",
            p.qty,
            p.symbol,
            fmt_cents(p.unrealized_cents)
        ));
    }
    if news.iter().any(|n| !n.usable_as_reason) {
        v.push("Some news in context is unverified or MNPI-flagged; it was shown as a warning and must not be a trade reason.".into());
    }
    v.push("Rerun the same ticket to confirm the fingerprint matches, then compare against the reference SMA backtest (`galion-wallstreet backtest`).".into());
    v
}

pub fn build(b: &PaperBroker, ticket: &str, title: &str, notes: Vec<Note>) -> Report {
    let a = b.account();
    let items = news::fixture();
    let mut syms: Vec<&str> = b
        .fills
        .iter()
        .filter(|f| f.ticket == ticket)
        .map(|f| f.symbol.as_str())
        .collect();
    syms.sort();
    syms.dedup();
    let mut ctx: Vec<Tagged> = Vec::new();
    for s in syms {
        for t in news::context(&items, s, &a.as_of) {
            if !ctx.iter().any(|c| c.item == t.item) {
                ctx.push(t);
            }
        }
    }
    let fills: Vec<Fill> = b
        .fills
        .iter()
        .filter(|f| f.ticket == ticket)
        .cloned()
        .collect();
    let refusals: Vec<Refusal> = b
        .refusals
        .iter()
        .filter(|r| r.ticket == ticket)
        .cloned()
        .collect();
    let next = next_steps(&a, &refusals, &ctx);
    let mut r = Report {
        ticket: ticket.into(),
        title: title.into(),
        mode: "paper",
        data_source: b.feed.data.source.clone(),
        account: a,
        fills,
        refusals,
        risk_events: b.events.clone(),
        news_context: ctx,
        scratchpad: notes,
        next_steps: next,
        fingerprint: String::new(),
        generated_at: zagreb_time::now_rfc3339(),
    };
    r.fingerprint = fingerprint(&serde_json::to_value(&r).unwrap_or_default());
    r
}

fn money(c: i64) -> String {
    fmt_cents(c)
}

pub fn markdown(r: &Report) -> String {
    let a = &r.account;
    let mut s = String::new();
    s.push_str(&format!(
        "# Paper P&L report: {} ({})\n\n",
        r.title, r.ticket
    ));
    s.push_str(&format!(
        "**Bottom line:** paper equity {} vs {} start, P&L {} ({} bps), realised {}, unrealised {}, fees {}. Kill switch: {}.\n\n",
        money(a.equity_cents),
        money(a.starting_cash_cents),
        money(a.pnl_cents),
        a.return_bps,
        money(a.realized_cents),
        money(a.unrealized_cents),
        money(a.fees_cents),
        if a.kill_switch.engaged { "ENGAGED" } else { "clear" }
    ));
    s.push_str(&format!(
        "Mode: PAPER only, no live broker. Data: {} (as of {}). Fingerprint `{}`. Generated {}.\n\n",
        r.data_source, a.as_of, r.fingerprint, r.generated_at
    ));
    s.push_str("## Positions\n\n| Symbol | Qty | Avg cost | Last | Unrealised | Realised |\n|---|---:|---:|---:|---:|---:|\n");
    for p in &a.positions {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            p.symbol,
            p.qty,
            p.avg_cost,
            p.last,
            money(p.unrealized_cents),
            money(p.realized_cents)
        ));
    }
    s.push_str("\n## Fills\n\n| # | Date | Side | Qty | Symbol | Price | Fee |\n|---:|---|---|---:|---|---:|---:|\n");
    for f in &r.fills {
        s.push_str(&format!(
            "| {} | {} | {:?} | {} | {} | {} | {} |\n",
            f.order_id,
            f.date,
            f.side,
            f.qty,
            f.symbol,
            f.price,
            money(f.fee_cents)
        ));
    }
    s.push_str("\n## Refusals and risk events\n\n");
    if r.refusals.is_empty() && r.risk_events.is_empty() {
        s.push_str("None.\n");
    }
    for x in &r.refusals {
        s.push_str(&format!(
            "- {} {} `{}`: {}\n",
            x.date, x.symbol, x.code, x.message
        ));
    }
    for e in &r.risk_events {
        s.push_str(&format!("- {} `{}`: {}\n", e.date, e.kind, e.detail));
    }
    s.push_str("\n## News context (public fixture, tagged)\n\n");
    if r.news_context.is_empty() {
        s.push_str("None.\n");
    }
    for n in &r.news_context {
        let tags: Vec<String> = n.tags.iter().map(|t| format!("{t:?}")).collect();
        s.push_str(&format!(
            "- {} {} [{}] {}{}\n",
            n.item.date,
            n.item.symbol,
            tags.join(", "),
            n.item.headline,
            if n.usable_as_reason {
                ""
            } else {
                " (WARNING: not usable as a trade reason)"
            }
        ));
    }
    s.push_str("\n## Scratchpad\n\n");
    if r.scratchpad.is_empty() {
        s.push_str("None.\n");
    }
    for n in &r.scratchpad {
        s.push_str(&format!("{}. {} {}\n", n.seq, n.date, n.text));
    }
    s.push_str("\n## What to do next\n\n");
    for n in &r.next_steps {
        s.push_str(&format!("- {n}\n"));
    }
    s
}

/// Report quality checks: (name, pass, detail).
pub fn quality(r: &Report) -> Vec<(String, bool, String)> {
    let a = &r.account;
    let md = markdown(r);
    let mut v = Vec::new();
    let mut c = |n: &str, ok: bool, d: String| v.push((n.to_string(), ok, d));
    c(
        "pnl_reconciles",
        a.realized_cents + a.unrealized_cents - a.fees_cents == a.pnl_cents,
        format!(
            "{} + {} - {} = {}",
            a.realized_cents, a.unrealized_cents, a.fees_cents, a.pnl_cents
        ),
    );
    let mv: i64 = a.positions.iter().map(|p| p.market_value_cents).sum();
    c(
        "equity_is_cash_plus_mtm",
        a.cash_cents + mv == a.equity_cents,
        String::new(),
    );
    for sec in [
        "**Bottom line:**",
        "## Positions",
        "## Fills",
        "## Refusals and risk events",
        "## News context",
        "## Scratchpad",
        "## What to do next",
    ] {
        c(&format!("section {sec}"), md.contains(sec), String::new());
    }
    c(
        "has_next_steps",
        !r.next_steps.is_empty(),
        format!("{} steps", r.next_steps.len()),
    );
    c("says_paper", md.contains("PAPER only"), String::new());
    c(
        "zagreb_timestamp",
        zagreb_time::require_rfc3339(&r.generated_at).is_ok(),
        r.generated_at.clone(),
    );
    let lower = md.to_lowercase();
    c(
        "no_promises",
        !["guaranteed", "can't lose", "sure thing"]
            .iter()
            .any(|w| lower.contains(w)),
        String::new(),
    );
    c(
        "fingerprint_stable",
        fingerprint(&serde_json::to_value(r).unwrap_or_default()) == r.fingerprint,
        r.fingerprint.clone(),
    );
    v
}
