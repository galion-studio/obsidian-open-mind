// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Pre-trade risk limits and the kill switch.
//!
//! Policy, in order:
//! 1. An order that only *reduces* an existing position is always allowed,
//!    even with the kill switch engaged. Getting out of risk is never blocked.
//! 2. With the kill switch engaged, every order that adds risk is refused.
//! 3. An order whose notional exceeds `max_order_notional` is refused AND
//!    trips the kill switch: a fat-finger or runaway agent stops itself, and an
//!    operator has to reset it on purpose.
//! 4. Position size and gross exposure caps refuse without tripping.

use crate::money::fmt_cents;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskLimits {
    /// Largest notional (cents) a single order may carry.
    pub max_order_notional: i64,
    /// Largest absolute share count per symbol.
    pub max_position_qty: i64,
    /// Largest sum of |position| * last price (cents) across the book.
    pub max_gross_exposure: i64,
    /// Largest number of resting limit orders.
    pub max_open_orders: usize,
}

impl Default for RiskLimits {
    fn default() -> Self {
        RiskLimits {
            max_order_notional: 2_500_000, // 25 000.00
            max_position_qty: 1_000,
            max_gross_exposure: 6_000_000, // 60 000.00
            max_open_orders: 10,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct KillSwitch {
    pub engaged: bool,
    pub reason: Option<String>,
    pub tripped_on: Option<String>,
    pub trips: u32,
}

impl KillSwitch {
    pub fn trip(&mut self, reason: String, date: &str) {
        self.engaged = true;
        self.reason = Some(reason);
        self.tripped_on = Some(date.to_string());
        self.trips += 1;
    }
    pub fn reset(&mut self) {
        self.engaged = false;
        self.reason = None;
        self.tripped_on = None;
    }
}

/// What the risk engine needs to know about one order.
#[derive(Debug, Clone, Copy)]
pub struct Intent {
    /// Signed share change: +buy, -sell.
    pub delta: i64,
    /// Price used for notional (cents).
    pub price: i64,
    pub position_before: i64,
    /// Gross exposure before this order (cents).
    pub gross_before: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskRefusal {
    pub code: &'static str,
    pub message: String,
}

pub fn reduces_only(i: &Intent) -> bool {
    let after = i.position_before + i.delta;
    i.delta != 0
        && after.abs() < i.position_before.abs()
        && after.signum() * i.position_before.signum() >= 0
}

/// Evaluate one order. May trip `kill`.
pub fn evaluate(
    limits: &RiskLimits,
    kill: &mut KillSwitch,
    i: &Intent,
    date: &str,
) -> Result<(), RiskRefusal> {
    if reduces_only(i) {
        return Ok(());
    }
    if kill.engaged {
        return Err(RiskRefusal {
            code: "kill_switch_engaged",
            message: format!(
                "kill switch engaged ({}); only reducing orders are accepted until an operator resets it",
                kill.reason.as_deref().unwrap_or("no reason recorded")
            ),
        });
    }
    let notional = (i.delta.unsigned_abs() as i128 * i.price as i128).min(i64::MAX as i128) as i64;
    if notional > limits.max_order_notional {
        let msg = format!(
            "order notional {} exceeds max {}; kill switch tripped",
            fmt_cents(notional),
            fmt_cents(limits.max_order_notional)
        );
        kill.trip(format!("oversized order: {msg}"), date);
        return Err(RiskRefusal {
            code: "kill_switch_oversized_order",
            message: msg,
        });
    }
    let after = i.position_before + i.delta;
    if after.abs() > limits.max_position_qty {
        return Err(RiskRefusal {
            code: "max_position",
            message: format!(
                "position would be {after} shares; max is {}",
                limits.max_position_qty
            ),
        });
    }
    if i.gross_before + notional > limits.max_gross_exposure {
        return Err(RiskRefusal {
            code: "max_gross_exposure",
            message: format!(
                "gross exposure would be {}; max is {}",
                fmt_cents(i.gross_before + notional),
                fmt_cents(limits.max_gross_exposure)
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(delta: i64, price: i64, pos: i64) -> Intent {
        Intent {
            delta,
            price,
            position_before: pos,
            gross_before: pos.abs() * price,
        }
    }

    #[test]
    fn oversized_trips_and_blocks_adds_but_not_reduces() {
        let l = RiskLimits::default();
        let mut k = KillSwitch::default();
        assert!(evaluate(&l, &mut k, &intent(100, 4_200, 0), "d").is_ok());
        let e = evaluate(&l, &mut k, &intent(100_000, 4_200, 100), "d").unwrap_err();
        assert_eq!(e.code, "kill_switch_oversized_order");
        assert!(k.engaged);
        assert_eq!(
            evaluate(&l, &mut k, &intent(1, 4_200, 100), "d")
                .unwrap_err()
                .code,
            "kill_switch_engaged"
        );
        assert!(evaluate(&l, &mut k, &intent(-100, 4_200, 100), "d").is_ok());
        k.reset();
        assert!(evaluate(&l, &mut k, &intent(1, 4_200, 0), "d").is_ok());
        assert_eq!(k.trips, 1);
    }

    #[test]
    fn position_and_gross_caps() {
        let l = RiskLimits::default();
        let mut k = KillSwitch::default();
        assert_eq!(
            evaluate(&l, &mut k, &intent(1_001, 100, 0), "d")
                .unwrap_err()
                .code,
            "max_position"
        );
        let i = Intent {
            delta: 500,
            price: 4_000,
            position_before: 0,
            gross_before: 5_000_000,
        };
        assert_eq!(
            evaluate(&l, &mut k, &i, "d").unwrap_err().code,
            "max_gross_exposure"
        );
        assert!(!k.engaged);
    }
}
