// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! One universal subscription for everything on galion.app (Galion One, the
//! console, Hermes, the OpenMind stack). Prices in euros (Mate, t550u,
//! 2026-11-01 14:26 CET). Features beyond these lines are not confirmed yet.

/// A plan. `Free` needs no account at all (open source, run it yourself).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Free,
    Plus,
    Pro,
}

impl Plan {
    pub const PAID: [Plan; 2] = [Plan::Plus, Plan::Pro];

    pub fn id(self) -> &'static str {
        match self {
            Plan::Free => "free",
            Plan::Plus => "plus",
            Plan::Pro => "pro",
        }
    }

    pub fn parse(s: &str) -> Option<Plan> {
        match s {
            "free" => Some(Plan::Free),
            "plus" => Some(Plan::Plus),
            "pro" => Some(Plan::Pro),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Plan::Free => "Free",
            Plan::Plus => "Galion Plus",
            Plan::Pro => "Galion Pro",
        }
    }

    /// Monthly price in euro cents.
    pub fn eur_cents(self) -> i64 {
        match self {
            Plan::Free => 0,
            Plan::Plus => 2_000,
            Plan::Pro => 22_000,
        }
    }

    /// The Stripe Price `lookup_key` the setup command creates.
    pub fn stripe_lookup_key(self) -> Option<&'static str> {
        match self {
            Plan::Free => None,
            Plan::Plus => Some("galion_plus_monthly"),
            Plan::Pro => Some("galion_pro_monthly"),
        }
    }

    /// One line a person can read. Only what is confirmed.
    pub fn blurb(self) -> &'static str {
        match self {
            Plan::Free => "Private and open source. Run it yourself, no account needed.",
            Plan::Plus => "One person, the whole Galion platform.",
            Plan::Pro => "Your Hermes agent gets its own cloud computer and SSD.",
        }
    }

    /// Rank for "at least this plan" checks.
    pub fn rank(self) -> u8 {
        match self {
            Plan::Free => 0,
            Plan::Plus => 1,
            Plan::Pro => 2,
        }
    }
}

/// Days of time bought by `cents` in `plan` (30-day months, rounded down).
/// Used for cash and crypto, which top up time instead of renewing.
pub fn days_for(plan: Plan, cents: i64) -> i64 {
    if plan.eur_cents() == 0 || cents <= 0 {
        return 0;
    }
    cents * 30 / plan.eur_cents()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_and_days() {
        assert_eq!(Plan::Plus.eur_cents(), 2000);
        assert_eq!(Plan::Pro.eur_cents(), 22000);
        assert_eq!(days_for(Plan::Plus, 2000), 30);
        assert_eq!(days_for(Plan::Plus, 6000), 90);
        assert_eq!(days_for(Plan::Pro, 22000), 30);
        assert_eq!(days_for(Plan::Pro, 1000), 1);
        assert_eq!(days_for(Plan::Free, 1000), 0);
        for p in [Plan::Free, Plan::Plus, Plan::Pro] {
            assert_eq!(Plan::parse(p.id()), Some(p));
        }
    }
}
