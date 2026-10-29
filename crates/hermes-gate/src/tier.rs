// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Model tiers for the gated flow (owner t379u/t380u): deep reasoning on the
//! high OpenRouter tier, routine steps on the low-cost tier, and the gate's
//! own steps on no model at all. The names are hermes-rs model aliases
//! (`model_aliases` in config.yaml): `boss` is the high tier today
//! (claude-opus-5.5 via OpenRouter); `worker` is the low tier the owner has
//! to point at a low-cost model. A paid step is refused when the remaining
//! budget is below its floor, so routine work can never eat the budget the
//! deep steps need.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Write the patch for a non-trivial change (design, multi-file fix).
    Propose,
    /// Explain why a pass record failed and what to change.
    ReviewFailure,
    /// One-line summary, status note, changelog line.
    Summarize,
    /// Static checks, sandbox deploy, signing, verify, apply.
    Gate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    High,
    Low,
    NoModel,
}

pub fn tier_for(s: Step) -> Tier {
    match s {
        Step::Propose | Step::ReviewFailure => Tier::High,
        Step::Summarize => Tier::Low,
        Step::Gate => Tier::NoModel,
    }
}

pub fn alias(t: Tier) -> Option<&'static str> {
    match t {
        Tier::High => Some("boss"),
        Tier::Low => Some("worker"),
        Tier::NoModel => None,
    }
}

/// USD that must remain after a call on this tier (the high tier keeps a
/// reserve of its own; the low tier stops earlier so it never starves it).
pub fn floor_usd(t: Tier) -> f64 {
    match t {
        Tier::High => 0.50,
        Tier::Low => 1.50,
        Tier::NoModel => 0.0,
    }
}

/// May a step run a paid call with `remaining_usd` left of the budget?
/// `est_usd` is the caller's estimate of the call.
pub fn may_call(s: Step, remaining_usd: f64, est_usd: f64) -> Result<Option<&'static str>, String> {
    let t = tier_for(s);
    let Some(a) = alias(t) else { return Ok(None) };
    if !(remaining_usd.is_finite() && est_usd.is_finite()) || est_usd < 0.0 {
        return Err("budget numbers must be finite".into());
    }
    if remaining_usd - est_usd < floor_usd(t) {
        return Err(format!(
            "{s:?} on {a}: ${est_usd:.2} would leave ${:.2}, below the {t:?} floor ${:.2}",
            remaining_usd - est_usd,
            floor_usd(t)
        ));
    }
    Ok(Some(a))
}
