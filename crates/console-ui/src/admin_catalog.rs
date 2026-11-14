// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Display catalog for the admin plan and account screens.
//!
//! The three prices are the agreed list: Free forever (Galion OS, open
//! source), Plus for Galion One ($20 or EUR 20 per month), Ultra for Galion
//! Research ($220 or EUR 220 one-time). Nothing here talks to a payment
//! service. The live account list is empty because this console has no
//! account directory and must not invent one from a billing call.
//!
//! Colours stay in the console palette (`--c-*`, `--sans`, `--display`,
//! `--mono` in `css.rs`, owned by the Swarm 2 orchestrator). This module
//! adds no tokens. Paid plans stay off until Slavik says otherwise.

/// One plan row. Prices are already formatted for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub id: &'static str,
    pub name: &'static str,
    pub product: &'static str,
    pub summary: &'static str,
    pub usd: &'static str,
    pub eur: &'static str,
    /// `forever`, `per month`, or `one-time`.
    pub cadence: &'static str,
}

/// Paid plans are not open. Display only, until Slavik says otherwise.
/// Flipping this does not enable a payment call.
pub const PAID_PLANS_OPEN: bool = false;

/// The agreed plan list, in display order.
pub const PLANS: &[Plan] = &[
    Plan {
        id: "free",
        name: "Free",
        product: "Galion OS",
        summary: "Open source. Free forever.",
        usd: "$0",
        eur: "EUR 0",
        cadence: "forever",
    },
    Plan {
        id: "plus",
        name: "Plus",
        product: "Galion One",
        summary: "Galion One.",
        usd: "$20",
        eur: "EUR 20",
        cadence: "per month",
    },
    Plan {
        id: "ultra",
        name: "Ultra",
        product: "Galion Research",
        summary: "Galion Research.",
        usd: "$220",
        eur: "EUR 220",
        cadence: "one-time",
    },
];

/// A directory row the account screen may show. The live route passes
/// [`LIVE_ACCOUNTS`], which is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub plan_id: String,
}

/// Accounts rendered on `/admin/accounts` in production. Empty on purpose.
pub const LIVE_ACCOUNTS: &[Account] = &[];

/// The only upstream read the admin screens share with the rest of the
/// console: the header origin dot. There is no billing path.
pub const UPSTREAM_READS: &[&str] = &["/api/v1/health"];

pub fn plan_by_id(id: &str) -> Option<&'static Plan> {
    PLANS.iter().find(|p| p.id == id)
}

/// `Open` for Galion OS, and for a paid plan only after [`PAID_PLANS_OPEN`].
pub fn availability(plan: &Plan) -> &'static str {
    if plan.id == "free" || PAID_PLANS_OPEN {
        "Open"
    } else {
        "Not open"
    }
}

/// How a plan's price is spoken on the page.
pub fn price_label(plan: &Plan) -> String {
    if plan.cadence == "forever" {
        format!("{} · {} · forever", plan.usd, plan.eur)
    } else {
        format!("{} or {} {}", plan.usd, plan.eur, plan.cadence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agreed_prices() {
        assert_eq!(PLANS.len(), 3);
        let free = plan_by_id("free").unwrap();
        assert_eq!(free.product, "Galion OS");
        assert!(free.summary.contains("Open source"));
        assert!(free.summary.contains("Free forever"));
        assert_eq!(price_label(free), "$0 · EUR 0 · forever");

        let plus = plan_by_id("plus").unwrap();
        assert_eq!(plus.product, "Galion One");
        assert_eq!(price_label(plus), "$20 or EUR 20 per month");

        let ultra = plan_by_id("ultra").unwrap();
        assert_eq!(ultra.product, "Galion Research");
        assert_eq!(price_label(ultra), "$220 or EUR 220 one-time");

        assert_eq!(LIVE_ACCOUNTS.len(), 0);
        assert_eq!(availability(free), "Open");
        assert_eq!(availability(plus), "Not open");
        assert_eq!(availability(ultra), "Not open");
        assert_eq!(UPSTREAM_READS, &["/api/v1/health"]);
        assert!(plan_by_id("nope").is_none());
    }
}
