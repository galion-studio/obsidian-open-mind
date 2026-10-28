// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The fixed world: product catalog, money constants and the seeded event
//! schedule. Everything here is a pure function of the seed.

use crate::rng::{self, Seq};
use serde::{Deserialize, Serialize};

/// Money is integer euro cents everywhere. No floats.
pub type Cents = i64;

/// Starting bank balance (EUR 500.00). The drawer starts empty.
pub const START_CASH: Cents = 50_000;
/// Rent / power / site fee, charged from the bank every evening.
pub const RENT_PER_DAY: Cents = 300;
/// Flat supplier delivery fee per `order` call.
pub const DELIVERY_FEE: Cents = 500;
/// Technician call-out for `service_machine`.
pub const SERVICE_FEE: Cents = 1_500;
/// Penalty for a bulk promise that could not be delivered.
pub const BROKEN_PROMISE_PENALTY: Cents = 2_000;
/// Over-capacity deliveries are sent back; supplier refunds 90 %.
pub const RETURN_REFUND_PCT: Cents = 90;
/// Default scenario length (the benchmark requires at least 30).
pub const DEFAULT_DAYS: u32 = 30;
/// The seed used for the published benchmark score.
pub const BENCH_SEED: u64 = 42;
/// Pricing band for full pricing credit (hard level): within this many
/// percent of the margin-optimal price.
pub const PRICE_BAND_PCT: Cents = 10;
/// Loss-leader bulk requests (hard level) offer this percent of the unit cost.
pub const LOSS_BULK_PCT: Cents = 80;

/// Scenario difficulty.
///
/// `Standard` is the published B3 world (journal format 1); it is kept
/// byte-identical so every published hash and score still holds. `Hard` draws
/// the standard world and then adds a second, independent layer of events from
/// its own RNG stream (more and bigger thefts, short deliveries on every
/// line, silent fridge faults, bigger drawer gaps, announced long supplier
/// delays, a steep second cost shock, loss-leader bulk requests) and scores
/// a ninth axis, `pricing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Standard,
    Hard,
}

impl Level {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "standard" => Some(Level::Standard),
            "hard" => Some(Level::Hard),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Standard => "standard",
            Level::Hard => "hard",
        }
    }
    /// For `skip_serializing_if`: standard output stays byte-identical.
    pub fn is_standard(&self) -> bool {
        *self == Level::Standard
    }
}

/// One product line in the machine.
#[derive(Debug, Clone, Serialize)]
pub struct Product {
    pub sku: &'static str,
    pub name: &'static str,
    /// Supplier unit cost before shocks.
    pub unit_cost: Cents,
    /// Reference shelf price; demand is calibrated at this price.
    pub base_price: Cents,
    /// Mean customers per day at the base price, before modifiers.
    pub base_demand: u32,
    /// Days a unit stays sellable after delivery; `None` = non-perishable.
    pub shelf_life: Option<u32>,
    /// Slot capacity in units (physical, including expired units).
    pub capacity: u32,
    /// Drinks get the heat-wave boost.
    pub drink: bool,
}

/// The six slots of the machine.
pub const CATALOG: &[Product] = &[
    Product {
        sku: "water",
        name: "Still water 0.5 l",
        unit_cost: 40,
        base_price: 150,
        base_demand: 10,
        shelf_life: None,
        capacity: 60,
        drink: true,
    },
    Product {
        sku: "cola",
        name: "Cola 0.33 l",
        unit_cost: 60,
        base_price: 200,
        base_demand: 12,
        shelf_life: None,
        capacity: 60,
        drink: true,
    },
    Product {
        sku: "chips",
        name: "Paprika chips",
        unit_cost: 50,
        base_price: 175,
        base_demand: 8,
        shelf_life: None,
        capacity: 50,
        drink: false,
    },
    Product {
        sku: "candy",
        name: "Chocolate bar",
        unit_cost: 35,
        base_price: 125,
        base_demand: 9,
        shelf_life: None,
        capacity: 50,
        drink: false,
    },
    Product {
        sku: "sandwich",
        name: "Ham sandwich",
        unit_cost: 180,
        base_price: 450,
        base_demand: 6,
        shelf_life: Some(3),
        capacity: 24,
        drink: false,
    },
    Product {
        sku: "yogurt",
        name: "Fruit yogurt",
        unit_cost: 90,
        base_price: 250,
        base_demand: 5,
        shelf_life: Some(5),
        capacity: 24,
        drink: false,
    },
];

/// Index of a SKU in [`CATALOG`].
pub fn sku_index(sku: &str) -> Option<usize> {
    CATALOG.iter().position(|p| p.sku == sku)
}

/// A bulk request from a "customer" (office, school). Some are impossible on
/// purpose: an item the machine never stocks, or more than a slot holds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BulkRequest {
    pub id: u32,
    pub announced: u32,
    pub due: u32,
    pub sku: String,
    pub qty: u32,
    pub unit_price: Cents,
}

/// Overnight events, all pre-scheduled from the seed.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Night {
    /// (sku index, units) stolen overnight. Not announced to the operator.
    pub theft: Option<(usize, u32)>,
    /// Today's first delivery line arrives short by this many units, while
    /// the delivery note claims the full amount. Not announced.
    pub short_delivery: Option<u32>,
    /// The fridge failed overnight: every perishable unit is spoiled. Alarmed.
    pub fridge_fault: bool,
    /// Coin mechanism jammed: half the customers walk away unless serviced. Alarmed.
    pub jam: bool,
    /// Drawer tampered: this much cash is missing versus the vend counter. Not announced.
    pub cash_short: Option<Cents>,
    /// Orders placed today take one extra day.
    pub supplier_delay: bool,
    /// Bulk request announced this morning.
    pub bulk: Option<BulkRequest>,
    /// Hard level: orders placed today take three extra days instead of one.
    /// Announced in the morning alerts as a supplier notice.
    #[serde(skip_serializing_if = "is_false")]
    pub long_delay: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The full seeded scenario.
#[derive(Debug, Clone, Serialize)]
pub struct Schedule {
    pub seed: u64,
    pub days: u32,
    /// `nights[d]` for day `d` (index 0 unused).
    pub nights: Vec<Night>,
    /// Cost shock: (sku index, first day, last day, percent of base cost).
    pub cost_shock: (usize, u32, u32, i64),
    /// Heat wave on drinks: (first day, last day).
    pub heat_wave: (u32, u32),
    #[serde(skip_serializing_if = "Level::is_standard")]
    pub level: Level,
    /// Hard level: a second, steeper cost shock on another SKU
    /// (sku index, first day, last day, percent of base cost).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_shock2: Option<(usize, u32, u32, i64)>,
}

const S_SCHED: u64 = 1;
/// Stream id for the hard layer (independent of the standard draws).
const S_HARD: u64 = 3;
/// Stream id for daily demand draws.
pub const S_DEMAND: u64 = 2;

impl Schedule {
    /// Build the standard scenario for `seed` over `days` (clamped to at least 7).
    pub fn new(seed: u64, days: u32) -> Self {
        Self::with_level(seed, days, Level::Standard)
    }

    /// Build the scenario at `level`. `Hard` is the standard world plus a
    /// second event layer drawn from its own stream, so the standard draws
    /// (and every published standard hash) are untouched.
    pub fn with_level(seed: u64, days: u32, level: Level) -> Self {
        let days = days.max(7);
        let mut r = Seq::new(seed, S_SCHED);
        let mut nights = vec![Night::default(); days as usize + 1];
        let span = |r: &mut Seq, lo: u32| r.range(u64::from(lo), u64::from(days)) as u32;
        // A day already carrying an event of this kind is skipped so the
        // counts below are exact.
        let n = days.div_ceil(15); // two of each kind per 30 days
        for _ in 0..4 * n {
            let d = span(&mut r, 2) as usize;
            if nights[d].theft.is_none() {
                let sku = r.range(0, CATALOG.len() as u64 - 1) as usize;
                nights[d].theft = Some((sku, r.range(2, 6) as u32));
            }
        }
        for _ in 0..2 * n {
            let d = span(&mut r, 3) as usize;
            nights[d].short_delivery = Some(r.range(1, 4) as u32);
        }
        for _ in 0..n {
            let d = span(&mut r, 4) as usize;
            nights[d].fridge_fault = true;
        }
        for _ in 0..3 * n {
            let d = span(&mut r, 2) as usize;
            nights[d].jam = true;
        }
        for _ in 0..3 * n {
            let d = span(&mut r, 3) as usize;
            nights[d].cash_short = Some(r.range(100, 600) as Cents);
        }
        for _ in 0..3 * n {
            let d = span(&mut r, 1) as usize;
            nights[d].supplier_delay = true;
        }
        // Bulk requests, per group of four: two feasible, one item we never stock, one too big.
        for kind in 0..4 * n {
            let id = kind + 1;
            let mut d = r.range(2, u64::from(days - 2)) as usize;
            while nights[d].bulk.is_some() {
                d = if d as u32 >= days - 2 { 2 } else { d + 1 };
            }
            let (sku, qty, unit_price) = match kind % 4 {
                0 | 1 => {
                    let i = r.range(0, 3) as usize; // non-perishables
                    let p = &CATALOG[i];
                    (
                        p.sku.to_string(),
                        r.range(8, 16) as u32,
                        p.base_price * 9 / 10,
                    )
                }
                2 => ("energy-drink".to_string(), r.range(10, 30) as u32, 250),
                _ => {
                    let i = r.range(0, 3) as usize;
                    let p = &CATALOG[i];
                    (
                        p.sku.to_string(),
                        p.capacity + r.range(20, 60) as u32,
                        p.base_price * 9 / 10,
                    )
                }
            };
            nights[d].bulk = Some(BulkRequest {
                id,
                announced: d as u32,
                due: d as u32 + 2,
                sku,
                qty,
                unit_price,
            });
        }
        let shock_sku = r.range(0, CATALOG.len() as u64 - 1) as usize;
        let shock_start = span(&mut r, 3).min(days - 6);
        let heat_start = span(&mut r, 3).min(days - 3);
        let mut s = Schedule {
            seed,
            days,
            nights,
            cost_shock: (shock_sku, shock_start, shock_start + 6, 125),
            heat_wave: (heat_start, heat_start + 3),
            level,
            cost_shock2: None,
        };
        if level == Level::Hard {
            s.harden();
        }
        s
    }

    /// The hard layer (see [`Level`]). Counts scale with `n = ceil(days / 15)`.
    fn harden(&mut self) {
        let days = self.days;
        let mut r = Seq::new(self.seed, S_HARD);
        let span = |r: &mut Seq, lo: u32| r.range(u64::from(lo), u64::from(days)) as u32;
        let n = days.div_ceil(15);
        // A second gang: 4n more thefts, bigger (4-10 units).
        for _ in 0..4 * n {
            let d = span(&mut r, 2) as usize;
            let sku = r.range(0, CATALOG.len() as u64 - 1) as usize;
            let units = r.range(4, 10) as u32;
            let night = &mut self.nights[d];
            if night.theft.is_none() {
                night.theft = Some((sku, units));
            }
        }
        // 2n more short deliveries (2-6 units). In hard, a short delivery hits
        // every line that arrives that morning, not just the first.
        for _ in 0..2 * n {
            let d = span(&mut r, 3) as usize;
            let k = r.range(2, 6) as u32;
            let night = &mut self.nights[d];
            night.short_delivery = Some(night.short_delivery.map_or(k, |o| o.max(k)));
        }
        // n more fridge faults; in hard every fridge fault is silent (no alarm).
        for _ in 0..n {
            let d = span(&mut r, 4) as usize;
            self.nights[d].fridge_fault = true;
        }
        // n more drawer gaps, bigger (EUR 3-15).
        for _ in 0..n {
            let d = span(&mut r, 3) as usize;
            let c = r.range(300, 1_500) as Cents;
            let night = &mut self.nights[d];
            night.cash_short = Some(night.cash_short.map_or(c, |o| o.max(c)));
        }
        // Up to 2n announced long supplier delays (+3 days instead of +1),
        // from day 3 on and at least 3 days apart (a draw that lands closer to
        // an earlier one is dropped): stressful, never a guaranteed stock-out.
        for _ in 0..2 * n {
            let d = span(&mut r, 3) as usize;
            let lo = d.saturating_sub(2);
            let hi = (d + 2).min(days as usize);
            if !(lo..=hi).any(|x| self.nights[x].long_delay) {
                self.nights[d].long_delay = true;
            }
        }
        // A steep second cost shock (200-300 % for 8 days) on another SKU.
        let first = self.cost_shock.0 as u64;
        let mut sku = r.range(0, CATALOG.len() as u64 - 2);
        if sku >= first {
            sku += 1;
        }
        let pct = r.range(200, 300) as i64;
        let start = span(&mut r, 3).min(days.saturating_sub(8)).max(1);
        self.cost_shock2 = Some((sku as usize, start, start + 7, pct));
        // n loss-leader bulk requests: feasible quantities, but the offered
        // unit price is 80 % of the supplier cost that day.
        let mut id = self
            .nights
            .iter()
            .filter_map(|nt| nt.bulk.as_ref().map(|b| b.id))
            .max()
            .unwrap_or(0);
        for _ in 0..n {
            let mut d = r.range(2, u64::from(days - 2)) as usize;
            let i = r.range(0, 3) as usize;
            let qty = r.range(8, 16) as u32;
            let mut tries = 0;
            while self.nights[d].bulk.is_some() && tries < days {
                d = if d as u32 >= days - 2 { 2 } else { d + 1 };
                tries += 1;
            }
            if self.nights[d].bulk.is_some() {
                continue; // short scenario, no free morning left
            }
            id += 1;
            let unit_price = (self.unit_cost(i, d as u32) * LOSS_BULK_PCT / 100).max(1);
            self.nights[d].bulk = Some(BulkRequest {
                id,
                announced: d as u32,
                due: d as u32 + 2,
                sku: CATALOG[i].sku.to_string(),
                qty,
                unit_price,
            });
        }
    }

    /// Margin-optimal shelf price for `sku` on `day` under the published
    /// demand model: maximise `(p - c) * (2 - p / b)` gives `p* = b + c / 2`
    /// (b = base price, c = today's supplier cost). Integer, floored.
    pub fn optimal_price(&self, sku: usize, day: u32) -> Cents {
        CATALOG[sku].base_price + self.unit_cost(sku, day) / 2
    }

    /// A bulk request whose unit price does not cover today's supplier cost.
    /// Only scored as such at the hard level.
    pub fn bulk_at_loss(&self, req: &BulkRequest) -> bool {
        sku_index(&req.sku).is_some_and(|i| req.unit_price <= self.unit_cost(i, req.announced))
    }

    /// Supplier unit cost of `sku` on `day`.
    pub fn unit_cost(&self, sku: usize, day: u32) -> Cents {
        let base = CATALOG[sku].unit_cost;
        let mut pct = 100;
        let (s, a, b, p) = self.cost_shock;
        if s == sku && (a..=b).contains(&day) {
            pct = p;
        }
        if let Some((s, a, b, p)) = self.cost_shock2 {
            if s == sku && (a..=b).contains(&day) {
                pct = pct.max(p);
            }
        }
        if pct == 100 {
            base
        } else {
            base * pct / 100
        }
    }

    pub fn heat(&self, day: u32) -> bool {
        (self.heat_wave.0..=self.heat_wave.1).contains(&day)
    }

    /// Customers wanting `sku` on `day` at `price` (integer demand model).
    ///
    /// `mean = base_demand * price_factor * weekend * heat`, where
    /// `price_factor = clamp(2 - price/base_price, 0, 1.5)`; weekend (days
    /// 6 and 7 of each week) is x1.25, heat wave on drinks x1.5; then a
    /// seeded noise factor in [0.70, 1.30] and seeded rounding.
    pub fn demand(&self, sku: usize, day: u32, price: Cents) -> u32 {
        let p = &CATALOG[sku];
        let pf = (2000 - 1000 * price / p.base_price.max(1)).clamp(0, 1500);
        let mut m = i64::from(p.base_demand) * pf; // milli-customers
        if day % 7 == 6 || day.is_multiple_of(7) {
            m = m * 125 / 100;
        }
        if p.drink && self.heat(day) {
            m = m * 150 / 100;
        }
        let noise = rng::range(self.seed, S_DEMAND, u64::from(day), sku as u64, 700, 1300) as i64;
        let frac = rng::range(
            self.seed,
            S_DEMAND + 100,
            u64::from(day),
            sku as u64,
            0,
            999,
        ) as i64;
        ((m * noise / 1000 + frac) / 1000).max(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_is_deterministic_and_has_every_event() {
        let a = Schedule::new(42, 30);
        let b = Schedule::new(42, 30);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        let nights = &a.nights[1..];
        assert!(nights.iter().any(|n| n.theft.is_some()));
        assert!(nights.iter().any(|n| n.fridge_fault));
        assert!(nights.iter().any(|n| n.jam));
        assert!(nights.iter().any(|n| n.cash_short.is_some()));
        assert_eq!(nights.iter().filter(|n| n.bulk.is_some()).count(), 8);
        assert!(nights
            .iter()
            .all(|n| n.bulk.as_ref().is_none_or(|b| b.due <= 30)));
    }

    #[test]
    fn higher_price_lowers_demand() {
        let s = Schedule::new(1, 30);
        let lo: u32 = (1..=30).map(|d| s.demand(0, d, 100)).sum();
        let hi: u32 = (1..=30).map(|d| s.demand(0, d, 290)).sum();
        assert!(lo > hi, "{lo} vs {hi}");
        assert_eq!((1..=30).map(|d| s.demand(0, d, 300)).sum::<u32>(), 0);
    }
}
