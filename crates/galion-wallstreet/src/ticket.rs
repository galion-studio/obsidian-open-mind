// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Scripted Hermes tickets: a JSON list of steps run against a paper broker.
//!
//! The ticket's free-text `request` is compliance-screened first; a refused
//! request runs no steps at all. Every step yields a result line, and the run
//! ends with the reproducible P&L report.

use crate::broker::{OrderRequest, Outcome, PaperBroker, Side};
use crate::compliance::{self, Verdict};
use crate::money::parse_cents;
use crate::news;
use crate::report::{self, Report};
use crate::strategy::Scratchpad;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SCRIPTED_TICKET: &str = include_str!("../fixtures/tickets/scripted-open-close.json");
pub const INSIDER_TICKET: &str = include_str!("../fixtures/tickets/insider-refusal.json");

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    Quote {
        symbol: String,
    },
    News {
        symbol: String,
    },
    Note {
        text: String,
        #[serde(default)]
        tags: Vec<String>,
    },
    Buy {
        symbol: String,
        qty: i64,
        #[serde(default)]
        rationale: String,
    },
    Sell {
        symbol: String,
        qty: i64,
        #[serde(default)]
        rationale: String,
    },
    LimitBuy {
        symbol: String,
        qty: i64,
        price: String,
        #[serde(default)]
        rationale: String,
    },
    LimitSell {
        symbol: String,
        qty: i64,
        price: String,
        #[serde(default)]
        rationale: String,
    },
    Cancel {
        order_id: u64,
    },
    Close {
        symbol: String,
    },
    Advance {
        #[serde(default = "one")]
        days: u32,
    },
    Kill {
        reason: String,
    },
    ResetKill {
        operator: String,
    },
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Ticket {
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub request: String,
    #[serde(default)]
    pub start_date: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub n: usize,
    pub date: String,
    pub op: String,
    pub result: String,
    pub detail: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct TicketRun {
    pub ticket: String,
    pub status: &'static str,
    pub refusal: Option<Verdict>,
    pub steps: Vec<StepResult>,
    pub report: Report,
}

pub fn parse(s: &str) -> Result<Ticket, String> {
    let t: Ticket = serde_json::from_str(s).map_err(|e| format!("bad ticket: {e}"))?;
    if t.id.trim().is_empty() {
        return Err("ticket id is empty".into());
    }
    if t.steps.len() > 500 {
        return Err("ticket has more than 500 steps".into());
    }
    Ok(t)
}

fn outcome_detail(o: &Outcome) -> Value {
    serde_json::to_value(o).unwrap_or(Value::Null)
}

/// Run a ticket on `b` (state carries over, so a session can run several).
pub fn run(b: &mut PaperBroker, t: &Ticket) -> TicketRun {
    let mut pad = Scratchpad::default();
    let mut steps = Vec::new();
    let verdict = compliance::screen(&format!("{} {}", t.title, t.request));
    if verdict.is_refusal() {
        let rep = report::build(b, &t.id, &t.title, Vec::new());
        return TicketRun {
            ticket: t.id.clone(),
            status: "refused",
            refusal: Some(verdict),
            steps,
            report: rep,
        };
    }
    if let Some(d) = &t.start_date {
        if let Err(e) = b.feed.seek(d) {
            steps.push(StepResult {
                n: 0,
                date: b.today(),
                op: "start_date".into(),
                result: "error".into(),
                detail: json!({ "error": e }),
            });
        }
    }
    let items = news::fixture();
    for (i, s) in t.steps.iter().enumerate() {
        let date = b.today();
        let order = |sym: &str, side, qty, limit: Option<i64>, why: &str| OrderRequest {
            symbol: sym.into(),
            side: Some(side),
            qty,
            limit,
            rationale: why.into(),
            ticket: t.id.clone(),
        };
        let (op, result, detail): (&str, String, Value) = match s {
            Step::Quote { symbol } => match b.feed.quote(symbol) {
                Some(q) => ("quote", "ok".into(), json!(q)),
                None => ("quote", "unknown_symbol".into(), Value::Null),
            },
            Step::News { symbol } => {
                let c = news::context(&items, symbol, &date);
                ("news", format!("{} items", c.len()), json!(c))
            }
            Step::Note { text, tags } => match pad.add(&date, &t.id, text, tags) {
                Ok(n) => ("note", "ok".into(), json!(n)),
                Err(v) => ("note", "refused".into(), json!(v)),
            },
            Step::Buy {
                symbol,
                qty,
                rationale,
            } => {
                let o = b.place(order(symbol, Side::Buy, *qty, None, rationale));
                ("buy", o.code().to_string(), outcome_detail(&o))
            }
            Step::Sell {
                symbol,
                qty,
                rationale,
            } => {
                let o = b.place(order(symbol, Side::Sell, *qty, None, rationale));
                ("sell", o.code().to_string(), outcome_detail(&o))
            }
            Step::LimitBuy {
                symbol,
                qty,
                price,
                rationale,
            }
            | Step::LimitSell {
                symbol,
                qty,
                price,
                rationale,
            } => {
                let side = if matches!(s, Step::LimitBuy { .. }) {
                    Side::Buy
                } else {
                    Side::Sell
                };
                let op = if side == Side::Buy {
                    "limit_buy"
                } else {
                    "limit_sell"
                };
                match parse_cents(price) {
                    Ok(l) => {
                        let o = b.place(order(symbol, side, *qty, Some(l), rationale));
                        (op, o.code().to_string(), outcome_detail(&o))
                    }
                    Err(e) => (op, "invalid_limit".into(), json!({ "error": e })),
                }
            }
            Step::Cancel { order_id } => match b.cancel(*order_id) {
                Ok(o) => ("cancel", "ok".into(), json!(o)),
                Err(e) => ("cancel", "not_found".into(), json!({ "error": e })),
            },
            Step::Close { symbol } => {
                let o = b.close(symbol, &t.id);
                ("close", o.code().to_string(), outcome_detail(&o))
            }
            Step::Advance { days } => {
                let mut fills = Vec::new();
                let mut moved = 0;
                for _ in 0..(*days).min(1000) {
                    if b.feed.at_end() {
                        break;
                    }
                    fills.extend(b.advance());
                    moved += 1;
                }
                (
                    "advance",
                    format!("{moved} days, now {}", b.today()),
                    json!(fills),
                )
            }
            Step::Kill { reason } => {
                b.engage_kill(reason);
                ("kill", "engaged".into(), json!(b.kill))
            }
            Step::ResetKill { operator } => {
                let r = b.reset_kill(operator);
                (
                    "reset_kill",
                    if r { "reset" } else { "was_clear" }.into(),
                    json!(b.kill),
                )
            }
        };
        steps.push(StepResult {
            n: i + 1,
            date,
            op: op.into(),
            result,
            detail,
        });
    }
    let rep = report::build(b, &t.id, &t.title, pad.notes.clone());
    TicketRun {
        ticket: t.id.clone(),
        status: "completed",
        refusal: None,
        steps,
        report: rep,
    }
}

/// Run a ticket on a fresh fixture broker.
pub fn run_fresh(t: &Ticket) -> TicketRun {
    let mut b = PaperBroker::fixture();
    run(&mut b, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_ticket_opens_trips_and_closes() {
        let t = parse(SCRIPTED_TICKET).unwrap();
        let r = run_fresh(&t);
        assert_eq!(r.status, "completed");
        let codes: Vec<&str> = r.steps.iter().map(|s| s.result.as_str()).collect();
        assert_eq!(codes[3], "filled", "{codes:?}");
        assert_eq!(codes[6], "kill_switch_oversized_order", "{codes:?}");
        assert_eq!(codes[7], "kill_switch_engaged", "{codes:?}");
        assert_eq!(codes[8], "filled", "{codes:?}");
        assert!(
            r.report.account.positions.iter().all(|p| p.qty == 0),
            "{codes:?}"
        );
        let again = run_fresh(&t);
        assert_eq!(r.report.fingerprint, again.report.fingerprint);
    }

    #[test]
    fn insider_ticket_refused_before_any_step() {
        let r = run_fresh(&parse(INSIDER_TICKET).unwrap());
        assert_eq!(r.status, "refused");
        assert!(r.steps.is_empty());
        assert!(r.report.fills.is_empty());
    }
}
