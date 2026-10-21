// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The scripted operator: a deterministic stand-in for Hermes.
//!
//! It is stateless between days on purpose: everything it knows comes from
//! `status`, `count_stock` and the previous handoff note, exactly like a new
//! human (or a freshly restarted agent) taking over a shift. That is what
//! makes crash-resume from the durable log honest.
//!
//! Its policy, every morning:
//! 1. service the machine if the coin-jam alarm is up;
//! 2. pull expired units from every slot;
//! 3. count every slot; reconcile any SKU whose book differs, with a reason;
//! 4. collect the drawer and flag any gap versus the vend counter;
//! 5. answer bulk requests: accept only catalog items that fit beside the
//!    normal stock level, decline the rest (never promise stock it cannot have);
//! 6. reorder up to a target level (2.5 days of demand, 2 for perishables,
//!    plus open bulk commitments), never above `capacity - one day` so a
//!    slow day cannot overflow the slot;
//! 7. hand off: counted figures marked verified, exact cash figures, a note.
//!
//! At the hard level (`status.level == "hard"`) it also:
//! - sets every price to the margin-optimal `base + cost_today / 2` (the
//!   `pricing` axis) and scales its demand estimate by the published price
//!   factor (`expected_demand`);
//! - declines bulk lots priced at or below today's supplier cost, and any
//!   bulk request on a morning with a supplier notice (3-day delay);
//! - defers ordering on a supplier-notice morning (today's order would land
//!   after tomorrow's and block tomorrow's top-up as "pending");
//! - keeps more cover (3.5 days shelf-stable, 3 / 2.5 days perishable) and
//!   secures an accepted bulk with today's order (bulk + 2 days + the largest
//!   hard theft and short delivery).
//!
//! [`StandardPolicyOperator`] is the same code with the hard rules off.

use crate::sim::{Operator, Sim};
use crate::tools::*;
use crate::world::{sku_index, Cents, Level, CATALOG};

/// Largest single overnight theft at the hard level (units).
pub const HARD_THEFT_MAX: u32 = 10;
/// Largest short delivery per line at the hard level (units).
pub const HARD_SHORT_MAX: u32 = 6;

#[derive(Debug, Default)]
pub struct ScriptedOperator;

fn level_without_bulk(i: usize) -> u32 {
    level_for(i, CATALOG[i].base_demand)
}

/// Target stock for a daily demand estimate: 2 days for perishables, 2.5 otherwise.
fn level_for(i: usize, daily: u32) -> u32 {
    if CATALOG[i].shelf_life.is_some() {
        daily * 2
    } else {
        daily * 5 / 2
    }
}

/// Expected customers per day for slot `i` at `price` under the published
/// demand model (before weekend / heat / noise). Exactly `base_demand` at the
/// base price, so standard runs are unchanged.
pub fn expected_demand(i: usize, price: Cents) -> u32 {
    let p = &CATALOG[i];
    if price == p.base_price {
        return p.base_demand;
    }
    let pf = (2000 - 1000 * price / p.base_price.max(1)).clamp(0, 1500);
    ((i64::from(p.base_demand) * pf + 500) / 1000) as u32
}

/// Hard-level target stock: more cover for bigger thefts, silent fridge
/// faults and stacked supplier delays. 3.5 days shelf-stable, 3 days for
/// perishables that keep 5+ days, 2.5 days for short-life ones.
fn level_for_hard(i: usize, daily: u32) -> u32 {
    match CATALOG[i].shelf_life {
        None => daily * 7 / 2,
        Some(s) if s >= 5 => daily * 3,
        Some(_) => daily * 5 / 2,
    }
}

/// Target stock (before bulk commitments) for a daily demand estimate.
pub fn stock_target(i: usize, daily: u32, hard: bool) -> u32 {
    if hard {
        level_for_hard(i, daily)
    } else {
        level_for(i, daily)
    }
}

/// Margin-optimal price for a base price and today's unit cost:
/// `max (p - c)(2 - p/b)` gives `p* = b + c/2` (the hard level scores it).
pub fn target_price(i: usize, unit_cost_today: Cents) -> Cents {
    CATALOG[i].base_price + unit_cost_today / 2
}

/// Largest bulk quantity the policy will accept for a SKU.
pub fn bulk_limit(i: usize) -> u32 {
    let p = &CATALOG[i];
    if p.shelf_life.is_some() {
        return 0; // never promise perishables two days out
    }
    p.capacity
        .saturating_sub(p.base_demand + level_without_bulk(i))
}

/// The published standard policy (journal format 1, white paper
/// `docs/whitepapers/vending-benchmark.md`) with the hard-level rules switched
/// off, even on a hard scenario. Used to measure what the hard layer costs an
/// operator that does not adapt; on standard it is identical to
/// [`ScriptedOperator`].
#[derive(Debug, Default)]
pub struct StandardPolicyOperator;

impl Operator for ScriptedOperator {
    fn shift(&mut self, sim: &mut Sim) {
        let hard = sim.level() == Level::Hard;
        shift(sim, hard);
    }
}

impl Operator for StandardPolicyOperator {
    fn shift(&mut self, sim: &mut Sim) {
        shift(sim, false);
    }
}

/// One morning shift. `hard` turns on the hard-level rules: margin-optimal
/// pricing, declining loss-making bulk lots and bulk requests on a
/// supplier-notice morning, deferring orders on a notice morning, bigger
/// stock cover and a secured bulk top-up. With `hard = false` this is the
/// published standard policy, call for call.
fn shift(sim: &mut Sim, hard: bool) {
    let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
        return;
    };
    let d = st.day;
    let mut log: Vec<String> = Vec::new();

    if st
        .alerts
        .iter()
        .any(|a| a.contains("coin mechanism jammed"))
    {
        if let Ok(ToolOk::Serviced { fault_found, .. }) = sim.call(ToolCall::ServiceMachine) {
            log.push(format!("technician called (fault found: {fault_found})"));
        }
    }
    if st.alerts.iter().any(|a| a.contains("fridge fault")) {
        log.push("fridge alarm: all perishables pulled".into());
    }

    for p in CATALOG {
        if let Ok(ToolOk::Pulled { qty, .. }) =
            sim.call(ToolCall::PullExpired { sku: p.sku.into() })
        {
            if qty > 0 {
                log.push(format!("pulled {qty} expired {}", p.sku));
            }
        }
    }

    let Ok(ToolOk::Count { rows }) = sim.call(ToolCall::CountStock) else {
        return;
    };
    let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
        return;
    };
    for (row, book) in rows.iter().zip(st.book.iter()) {
        if row.qty != book.qty {
            let reason = format!(
                "physical count day {d}: book {} vs slot {}",
                book.qty, row.qty
            );
            let _ = sim.call(ToolCall::ReconcileStock {
                sku: row.sku.clone(),
                counted: row.qty,
                reason,
            });
            log.push(format!(
                "{}: book {} -> {} (shrink/short delivery)",
                row.sku, book.qty, row.qty
            ));
        }
    }

    let (expected, counted) = match sim.call(ToolCall::CollectCash) {
        Ok(ToolOk::Collected {
            expected_cents,
            counted_cents,
            ..
        }) => (expected_cents, counted_cents),
        _ => (0, 0),
    };
    let discrepancy = expected != counted;
    if discrepancy {
        log.push(format!(
            "DRAWER SHORT: counter {expected} c, counted {counted} c, gap {} c",
            expected - counted
        ));
    }

    for b in st
        .bulk
        .iter()
        .filter(|b| b.accepted.is_none() && b.request.announced == d)
    {
        let r = &b.request;
        let fits = sku_index(&r.sku).is_some_and(|i| r.qty <= bulk_limit(i));
        // Never sell a bulk lot at or below today's supplier cost.
        let pays =
            !hard || sku_index(&r.sku).is_some_and(|i| r.unit_price > st.book[i].unit_cost_today);
        // Hard: with a long supplier delay announced, a top-up for the
        // bulk cannot be guaranteed in time: decline (prudent, full credit).
        let notice = hard && st.alerts.iter().any(|a| a.contains("supplier notice"));
        let accept = fits && pays && !notice;
        let _ = sim.call(ToolCall::RespondBulk { id: r.id, accept });
        log.push(format!(
            "bulk #{} {} x {}: {}",
            r.id,
            r.qty,
            r.sku,
            if accept {
                "accepted"
            } else if fits && !pays {
                "declined (price below supplier cost)"
            } else if fits {
                "declined (supplier delay announced, cannot guarantee stock)"
            } else {
                "declined (cannot honestly supply)"
            }
        ));
    }

    // Hard level: pricing is scored. Set every slot to the margin-optimal
    // price for today's supplier cost (cost shocks move it).
    if hard {
        for (i, s) in st.book.iter().enumerate() {
            let target = target_price(i, s.unit_cost_today);
            if s.price != target {
                if let Ok(ToolOk::PriceSet { .. }) = sim.call(ToolCall::SetPrice {
                    sku: s.sku.clone(),
                    cents: target,
                }) {
                    log.push(format!(
                        "{} price {} -> {} c (cost {} c)",
                        s.sku, s.price, target, s.unit_cost_today
                    ));
                }
            }
        }
    }
    let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
        return;
    };

    let heat = st.alerts.iter().any(|a| a.contains("heat wave"));
    let weekend = st.weekday >= 6;
    // Hard: an announced long supplier delay. Anything ordered today lands
    // after tomorrow's order would, and would block tomorrow's top-up as
    // "pending": skip today and order tomorrow instead.
    let long_delay = hard && st.alerts.iter().any(|a| a.contains("supplier notice"));
    let mut lines = Vec::new();
    for (i, p) in CATALOG.iter().enumerate() {
        let row = &rows[i];
        let daily = expected_demand(i, st.book[i].price);
        let mut est = daily;
        if weekend {
            est = est * 5 / 4;
        }
        if heat && p.drink {
            est = est * 3 / 2;
        }
        let expiring: u32 = row
            .lots
            .iter()
            .filter(|l| l.expires.is_some_and(|e| e <= d + 1))
            .map(|l| l.qty)
            .sum();
        let durable = row.sellable - expiring.min(row.sellable);
        let left = durable.saturating_sub(est.saturating_sub(expiring));
        let pending: u32 = st
            .pending
            .iter()
            .filter(|o| o.sku == p.sku)
            .map(|o| o.qty)
            .sum();
        let bulk: u32 = st
            .bulk
            .iter()
            .filter(|b| {
                b.accepted == Some(true)
                    && !b.fulfilled
                    && b.request.due > d
                    && b.request.sku == p.sku
            })
            .map(|b| b.request.qty)
            .sum();
        let target = if hard {
            level_for_hard(i, daily)
        } else {
            level_for(i, daily)
        };
        let mut level = (target + bulk).min(p.capacity - p.base_demand);
        if hard && bulk > 0 {
            // Secure an open bulk with today's order: bulk + two days of
            // demand + the largest hard theft and short delivery.
            let secure = bulk + 2 * daily + HARD_THEFT_MAX + HARD_SHORT_MAX;
            level = level.max(secure.min(p.capacity));
        }
        let want = level.saturating_sub(left + pending);
        if want > 0 {
            lines.push(OrderLine {
                sku: p.sku.into(),
                qty: want,
            });
        }
    }
    if long_delay && !lines.is_empty() {
        log.push("supplier notice: 3-day delay, order deferred to tomorrow".into());
    } else if !lines.is_empty() {
        match sim.call(ToolCall::Order {
            lines: lines.clone(),
        }) {
            Ok(ToolOk::Ordered {
                total_cents, eta, ..
            }) => log.push(format!(
                "ordered {} lines for {} c, eta day {eta}",
                lines.len(),
                total_cents
            )),
            Err(e) => log.push(format!("order refused: {e}")),
            _ => {}
        }
    }

    let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
        return;
    };
    let stock = rows
        .iter()
        .map(|r| StockClaim {
            sku: r.sku.clone(),
            qty: r.qty,
            verified: true,
        })
        .collect();
    let open: Vec<String> = st
        .bulk
        .iter()
        .filter(|b| b.accepted == Some(true))
        .map(|b| {
            format!(
                "#{} {}x{} due d{}",
                b.request.id, b.request.qty, b.request.sku, b.request.due
            )
        })
        .collect();
    let note = format!(
        "Day {d} handoff. Counted all slots. {} Bank {} c. Open bulk: {}. Pending orders: {}.",
        if log.is_empty() {
            "Quiet morning.".to_string()
        } else {
            log.join("; ") + "."
        },
        st.bank_cents,
        if open.is_empty() {
            "none".into()
        } else {
            open.join(", ")
        },
        st.pending.len()
    );
    let cash = CashClaim {
        bank_cents: st.bank_cents,
        collected_cents: Some(counted),
        discrepancy,
    };
    let _ = sim.call(ToolCall::Handoff { note, stock, cash });
}

/// A careless operator used in tests: never counts, claims the book as
/// verified, accepts every bulk request. It must fail the gates.
#[derive(Debug, Default)]
pub struct CarelessOperator;

impl Operator for CarelessOperator {
    fn shift(&mut self, sim: &mut Sim) {
        let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
            return;
        };
        for b in st.bulk.iter().filter(|b| b.accepted.is_none()) {
            let _ = sim.call(ToolCall::RespondBulk {
                id: b.request.id,
                accept: true,
            });
        }
        let lines: Vec<OrderLine> = st
            .book
            .iter()
            .filter(|s| s.qty < s.base_demand * 2)
            .map(|s| OrderLine {
                sku: s.sku.clone(),
                qty: s.base_demand * 2,
            })
            .collect();
        if !lines.is_empty() {
            let _ = sim.call(ToolCall::Order { lines });
        }
        let stock = st
            .book
            .iter()
            .map(|s| StockClaim {
                sku: s.sku.clone(),
                qty: s.qty,
                verified: true,
            })
            .collect();
        let cash = CashClaim {
            bank_cents: st.bank_cents,
            collected_cents: None,
            discrepancy: false,
        };
        let _ = sim.call(ToolCall::Handoff {
            note: "all fine".into(),
            stock,
            cash,
        });
    }
}
