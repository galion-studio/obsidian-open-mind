// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Adapter for GAIA HADES (`gaia-hades`, landing on `swarm/gaia-land`).
//!
//! It takes plain strings so `gaia-hades` can call it without this crate
//! depending on `gaia-core` (and without a dependency cycle):
//!
//! ```text
//! let plan = Hades::plan_for(d.target, d.intensity);
//! let v = hades_gate::adapter::check_gaia(&approver, d.target.name(), service, &plan, token, now);
//! ctx.approved = v.decision() == hades_gate::Decision::WouldApply;
//! ```
//!
//! The plan bytes that are hashed and signed are the steps joined with `\n`
//! ([`gaia_plan_bytes`]). The admin mints with exactly the same bytes.

use crate::approval::{Approver, Audit, Decided, Refusal, Request, Verdict};

/// Map a GAIA HADES system name to a gate action kind.
///
/// `teardown`, `kill-switch` and `secret-rotation-plan` map to `None`
/// (never approvable here): the admin stops things by hand, and secret values
/// are never handled by HADES.
#[must_use]
pub fn gaia_action(system: &str) -> Option<&'static str> {
    match system {
        "heal-apply" => Some("heal-apply"),
        "rollback" => Some("rollback"),
        "deploy-switch" | "switch" => Some("switch"),
        _ => None,
    }
}

/// The exact bytes a GAIA plan is signed over.
#[must_use]
pub fn gaia_plan_bytes(steps: &[String]) -> Vec<u8> {
    steps.join("\n").into_bytes()
}

/// Check a GAIA `Execute` directive.
#[must_use]
pub fn check_gaia(
    approver: &Approver,
    system: &str,
    target: &str,
    steps: &[String],
    token: &str,
    now: u64,
) -> Verdict {
    let Some(action) = gaia_action(system) else {
        return Verdict::Refused(Refusal::ActionNotApprovable(system.to_owned()));
    };
    let plan = gaia_plan_bytes(steps);
    approver.approve(
        Request {
            action,
            target,
            plan: &plan,
            token,
        },
        now,
    )
}

/// [`check_gaia`] plus whether the decision reached the ledger
/// ([`Approver::approve_audited`]). A system that can never be approved is
/// refused here and also written to the ledger as an audit line.
#[must_use]
pub fn check_gaia_audited(
    approver: &Approver,
    system: &str,
    target: &str,
    steps: &[String],
    token: &str,
    now: u64,
) -> Decided {
    let Some(action) = gaia_action(system) else {
        let refusal = Refusal::ActionNotApprovable(system.to_owned());
        let audit =
            match approver
                .ledger()
                .note("refused", system, target, &refusal.reason(), token, now)
            {
                Ok(()) => Audit::Written,
                Err(e) => Audit::Failed(e),
            };
        return Decided {
            verdict: Verdict::Refused(refusal),
            audit,
        };
    };
    let plan = gaia_plan_bytes(steps);
    approver.approve_audited(
        Request {
            action,
            target,
            plan: &plan,
            token,
        },
        now,
    )
}
