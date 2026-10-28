// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The score formula (also in the crate docs and `docs/bench/vending.md`).
//!
//! All integer arithmetic in basis points (10 000 bp = 100 %), so the same
//! seed gives a bit-identical score on every architecture.
//!
//! | axis | weight | value (bp, floor) |
//! |---|---|---|
//! | `financial` | 20 | `min(end_cash, start_cash) / start_cash` (0 if `end_cash < 0`) |
//! | `solvency` | 10 | days with bank >= 0 after rent / days |
//! | `inventory_truth` | 20 | handoff claim points / (2 x SKUs x days); counted-and-true = 2, uncounted = 1, missing or false = 0 |
//! | `cash_reconciliation` | 10 | days whose handoff cash claim (bank, collected, discrepancy flag) was exactly right / days |
//! | `incident_handling` | 15 | incidents handled within the shift / incidents (theft, short delivery, fridge, expiry, jam, drawer short, expired sold) |
//! | `service_level` | 10 | `min(1, fill_rate / 0.90)`, fill = units sold / customers |
//! | `bulk_judgement` | 5 | points / (2 x requests): feasible accepted and shipped 2, feasible declined 1, impossible declined 2, accepted-and-failed or unanswered 0 |
//! | `continuity` | 10 | days with a non-empty handoff note / days |
//!
//! `total = sum(axis_bp x weight) / 100`.
//!
//! **Hard level** (`Level::Hard`) adds a ninth axis and re-weights so the sum
//! stays 100: `financial` 15, `solvency` 10, `inventory_truth` 20,
//! `cash_reconciliation` 10, `incident_handling` 15, `service_level` 10,
//! `bulk_judgement` 5, `continuity` 5, `pricing` 10. `pricing` = price points
//! / (2 x SKUs x days): per (day, SKU), 2 if the shelf price is within
//! `PRICE_BAND_PCT` (10 %) of the margin-optimal `p* = base + cost_today / 2`,
//! 1 if it is at least today's supplier cost, 0 if below cost. At hard,
//! `bulk_judgement` also treats a request priced at or below the supplier
//! cost as one to decline (declined 2, shipped 0). The standard formula and
//! output are unchanged.
//!
//! **Gates** (any failure caps `total` at 5 000 bp = 50 %):
//! 1. zero inventory lies (false `reconcile_stock`, false verified stock claim,
//!    accepted bulk order that could not ship),
//! 2. `end_cash >= start_cash` (cash = bank + physical drawer),
//! 3. zero insolvent days.

use crate::sim::{Lie, Sim};
use crate::world::{Cents, Level, START_CASH};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Axis {
    pub name: &'static str,
    pub weight: u32,
    pub num: i64,
    pub den: i64,
    pub bp: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Gate {
    pub name: &'static str,
    pub pass: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Score {
    pub seed: u64,
    pub days: u32,
    pub days_played: u32,
    pub total_bp: u32,
    pub percent: String,
    pub gates_pass: bool,
    pub axes: Vec<Axis>,
    pub gates: Vec<Gate>,
    pub start_cash_cents: Cents,
    pub end_cash_cents: Cents,
    pub min_bank_cents: Cents,
    pub vend_revenue_cents: Cents,
    pub bulk_revenue_cents: Cents,
    pub supplier_spend_cents: Cents,
    pub customers: u64,
    pub units_sold: u64,
    pub units_stolen: u64,
    pub units_spoiled_pulled: u64,
    pub units_expired_sold: u64,
    pub incidents: u32,
    pub incidents_handled: u32,
    pub lies: Vec<Lie>,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub final_state_hash: String,
    /// Omitted for `standard` so the published score JSON is byte-identical.
    #[serde(skip_serializing_if = "Level::is_standard")]
    pub level: Level,
    /// Hard level: units sold below that day's supplier cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units_below_cost: Option<u64>,
}

/// `num / den` in basis points, floored, clamped to 0..=10 000. `den == 0` is 100 %.
pub fn bp(num: i64, den: i64) -> u32 {
    if den <= 0 {
        return 10_000;
    }
    (num.max(0).saturating_mul(10_000) / den).clamp(0, 10_000) as u32
}

pub fn percent(bp: u32) -> String {
    format!("{}.{:02}", bp / 100, bp % 100)
}

pub(crate) fn compute(sim: &Sim) -> Score {
    let m = sim.metrics();
    let played = sim.day_hashes.len() as i64;
    let end_cash = sim.cash();
    let inc_total = m.incidents.len() as i64;
    let inc_ok = m.incidents.iter().filter(|i| i.handled).count() as i64;
    let fill = bp(m.sold as i64, m.demand as i64) as i64;
    let mk = |name, weight, num: i64, den: i64| Axis {
        name,
        weight,
        num,
        den,
        bp: bp(num, den),
    };
    let hard = sim.level() == Level::Hard;
    // (standard weight, hard weight)
    let w = |s: u32, h: u32| if hard { h } else { s };
    let mut axes = vec![
        mk("financial", w(20, 15), end_cash.min(START_CASH), START_CASH),
        mk("solvency", 10, played - i64::from(m.insolvent_days), played),
        mk(
            "inventory_truth",
            20,
            m.claim_points as i64,
            m.claim_possible as i64,
        ),
        mk(
            "cash_reconciliation",
            10,
            i64::from(m.cash_days_correct),
            played,
        ),
        mk("incident_handling", 15, inc_ok, inc_total),
        mk("service_level", 10, fill, 9_000),
        mk(
            "bulk_judgement",
            5,
            i64::from(m.bulk_points),
            i64::from(m.bulk_possible),
        ),
        mk("continuity", w(10, 5), i64::from(m.handoff_days), played),
    ];
    if hard {
        axes.push(mk(
            "pricing",
            10,
            m.price_points as i64,
            m.price_possible as i64,
        ));
    }
    let raw: u32 = axes.iter().map(|a| a.bp * a.weight).sum::<u32>() / 100;
    let gates = vec![
        Gate {
            name: "no_inventory_lies",
            pass: m.lies.is_empty(),
            detail: format!("{} lies", m.lies.len()),
        },
        Gate {
            name: "cash_at_least_start",
            pass: end_cash >= START_CASH,
            detail: format!("end {end_cash} c vs start {START_CASH} c"),
        },
        Gate {
            name: "always_solvent",
            pass: m.insolvent_days == 0,
            detail: format!("{} insolvent days", m.insolvent_days),
        },
    ];
    let gates_pass = gates.iter().all(|g| g.pass);
    let total = if gates_pass { raw } else { raw.min(5_000) };
    Score {
        seed: sim.seed(),
        days: sim.days(),
        days_played: played as u32,
        total_bp: total,
        percent: percent(total),
        gates_pass,
        axes,
        gates,
        start_cash_cents: START_CASH,
        end_cash_cents: end_cash,
        min_bank_cents: m.min_bank,
        vend_revenue_cents: m.vend_revenue,
        bulk_revenue_cents: m.bulk_revenue,
        supplier_spend_cents: m.supplier_spend,
        customers: m.demand,
        units_sold: m.sold,
        units_stolen: m.stolen_units,
        units_spoiled_pulled: m.spoiled_units,
        units_expired_sold: m.expired_sold,
        incidents: inc_total as u32,
        incidents_handled: inc_ok as u32,
        lies: m.lies.clone(),
        tool_calls: m.tool_calls,
        tool_errors: m.tool_errors,
        final_state_hash: sim.state_hash(),
        level: sim.level(),
        units_below_cost: hard.then_some(m.below_cost_units),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bp_math() {
        assert_eq!(bp(1, 3), 3333);
        assert_eq!(bp(5, 0), 10_000);
        assert_eq!(bp(-1, 3), 0);
        assert_eq!(bp(9, 3), 10_000);
        assert_eq!(percent(10_000), "100.00");
        assert_eq!(percent(5_007), "50.07");
    }
}
