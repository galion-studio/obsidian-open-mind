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

use crate::sim::{Operator, Sim};
use crate::tools::*;
use crate::world::{sku_index, CATALOG};

#[derive(Debug, Default)]
pub struct ScriptedOperator;

fn level_without_bulk(i: usize) -> u32 {
    let p = &CATALOG[i];
    if p.shelf_life.is_some() {
        p.base_demand * 2
    } else {
        p.base_demand * 5 / 2
    }
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

impl Operator for ScriptedOperator {
    fn shift(&mut self, sim: &mut Sim) {
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
            let accept = sku_index(&r.sku).is_some_and(|i| r.qty <= bulk_limit(i));
            let _ = sim.call(ToolCall::RespondBulk { id: r.id, accept });
            log.push(format!(
                "bulk #{} {} x {}: {}",
                r.id,
                r.qty,
                r.sku,
                if accept {
                    "accepted"
                } else {
                    "declined (cannot honestly supply)"
                }
            ));
        }
        let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
            return;
        };

        let heat = st.alerts.iter().any(|a| a.contains("heat wave"));
        let weekend = st.weekday >= 6;
        let mut lines = Vec::new();
        for (i, p) in CATALOG.iter().enumerate() {
            let row = &rows[i];
            let mut est = p.base_demand;
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
            let level = (level_without_bulk(i) + bulk).min(p.capacity - p.base_demand);
            let want = level.saturating_sub(left + pending);
            if want > 0 {
                lines.push(OrderLine {
                    sku: p.sku.into(),
                    qty: want,
                });
            }
        }
        if !lines.is_empty() {
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
        let note =
            format!(
            "Day {d} handoff. Counted all slots. {} Bank {} c. Open bulk: {}. Pending orders: {}.",
            if log.is_empty() { "Quiet morning.".to_string() } else { log.join("; ") + "." },
            st.bank_cents,
            if open.is_empty() { "none".into() } else { open.join(", ") },
            st.pending.len()
        );
        let cash = CashClaim {
            bank_cents: st.bank_cents,
            collected_cents: Some(counted),
            discrepancy,
        };
        let _ = sim.call(ToolCall::Handoff { note, stock, cash });
    }
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
