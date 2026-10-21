// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! HADES Option B follow-up (slot 152): the typed twin of
//! [`crate::Hades::plan_for`].
//!
//! [`typed_plan_for`] emits a canonical `hades-plan/v1` plan
//! ([`hades_gate::typed::Plan`]) built only from the closed [`Step`] enum, so
//! there is no free text to sign. Its canonical bytes are exactly what
//! `hades-gate check-typed` / `POST /v1/approve-typed` hash and verify.
//!
//! This path is **parallel**. The prose [`crate::Hades::plan_for`] is
//! unchanged and is still what [`crate::Hades`] reports and what the live
//! gate (Option A, prose + `safety::review`) checks. Choosing A, B or both is
//! Slavik's decision (docs/security/hades-option-b.md, D-B1); nothing here
//! changes the HADES mode or wires B into the gate.
//!
//! Only systems the gate can approve at all ([`hades_gate::adapter::gaia_action`])
//! have a typed plan. `teardown`, `kill-switch` and `secret-rotation-plan` stop
//! or revoke things, and `hades-plan/v1` deliberately has no stop / revoke /
//! delete step, so they return [`TypedRefusal::NotExpressible`]: fail closed,
//! never a lossy plan.

use gaia_core::{scaled, FunctionId, System};
use hades_gate::typed::{Plan, PlanError, Probe, Runbook, Source, Step, MAX_FIXES};

/// Why a system has no typed plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedRefusal {
    /// The system is not owned by HADES.
    NotHades(System),
    /// A HADES system whose plan needs a step `hades-plan/v1` does not have
    /// (stop, revoke, broadcast). The gate refuses these actions anyway.
    NotExpressible(System),
    /// Building the plan failed validation (a bug; covered by tests).
    Plan(PlanError),
}

impl TypedRefusal {
    /// Short stable code for JSON output.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotHades(_) => "not-hades",
            Self::NotExpressible(_) => "not-expressible",
            Self::Plan(_) => "plan",
        }
    }

    /// Human-readable reason.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::NotHades(s) => format!("{s} is not a HADES system"),
            Self::NotExpressible(s) => format!(
                "{s} has no hades-plan/v1 form: v1 has no stop/revoke/delete step, \
                 and the gate never approves this action"
            ),
            Self::Plan(e) => e.reason(),
        }
    }
}

impl std::fmt::Display for TypedRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for TypedRefusal {}

/// The gate action a typed plan for `system` is approved under
/// (`hades_gate::adapter::gaia_action`), if any.
#[must_use]
pub fn typed_action(system: System) -> Option<&'static str> {
    if system.function() != FunctionId::Hades {
        return None;
    }
    hades_gate::adapter::gaia_action(system.name())
}

/// The typed plan for one HADES system, scoped by intensity like the prose
/// plan (`scaled(12, intensity)`, so `apply-fixes max=` is 1..=12).
///
/// # Errors
/// [`TypedRefusal`] when the system has no typed form.
pub fn typed_plan_for(system: System, intensity: u8) -> Result<Plan, TypedRefusal> {
    if system.function() != FunctionId::Hades {
        return Err(TypedRefusal::NotHades(system));
    }
    let units = scaled(12, intensity);
    let steps = match system {
        System::Rollback => vec![
            Step::ReadReleases,
            Step::Operator(Runbook::DeployRollback),
            Step::Operator(Runbook::DeployRollbackConsole),
            Step::Verify(Probe::new(None, "/health").map_err(TypedRefusal::Plan)?),
            Step::KeepFailed,
            Step::RecordLineage,
        ],
        System::HealApply => {
            // scaled() is 1..=12 here; the clamp keeps the v1 bound explicit.
            let max = u16::try_from(units)
                .unwrap_or(MAX_FIXES)
                .clamp(1, MAX_FIXES);
            vec![
                Step::TakeFindings(Source::Demeter),
                Step::ApplyFixes { max },
                Step::AppendActions,
                Step::RecordLineage,
            ]
        }
        other => return Err(TypedRefusal::NotExpressible(other)),
    };
    Plan::new(steps).map_err(TypedRefusal::Plan)
}

/// [`typed_plan_for`] as canonical bytes: what the admin mints a token over.
///
/// # Errors
/// As [`typed_plan_for`].
pub fn typed_plan_bytes(system: System, intensity: u8) -> Result<Vec<u8>, TypedRefusal> {
    typed_plan_for(system, intensity).map(|p| p.canonical_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hades_gate::typed::MAGIC;

    #[test]
    fn rollback_is_the_canonical_runbook_plan() {
        let p = typed_plan_for(System::Rollback, 100).unwrap();
        assert_eq!(
            p.canonical(),
            "hades-plan/v1\nread-releases\noperator deploy-rollback\n\
             operator deploy-rollback-console\nverify /health\nkeep-failed\nrecord-lineage"
        );
        assert_eq!(typed_action(System::Rollback), Some("rollback"));
    }

    #[test]
    fn heal_apply_scales_and_stays_in_v1_bounds() {
        for (i, max) in [(0u8, 1u16), (1, 1), (50, 6), (100, 12), (255, 12)] {
            let p = typed_plan_for(System::HealApply, i).unwrap();
            assert_eq!(
                p.canonical(),
                format!(
                    "{MAGIC}\ntake-findings demeter\napply-fixes max={max}\n\
                     append-actions\nrecord-lineage"
                ),
                "intensity {i}"
            );
        }
        assert_eq!(typed_action(System::HealApply), Some("heal-apply"));
    }

    #[test]
    fn every_typed_plan_round_trips_for_every_intensity() {
        for s in System::ALL {
            for i in 0..=u8::MAX {
                match typed_plan_for(*s, i) {
                    Ok(p) => {
                        let bytes = p.canonical_bytes();
                        assert_eq!(Plan::parse(&bytes).unwrap(), p, "{s} {i}");
                        assert_eq!(typed_plan_bytes(*s, i).unwrap(), bytes);
                        assert!(typed_action(*s).is_some(), "{s}");
                    }
                    Err(e) => assert!(
                        matches!(
                            e,
                            TypedRefusal::NotHades(_) | TypedRefusal::NotExpressible(_)
                        ),
                        "{s}: {e}"
                    ),
                }
            }
        }
    }

    #[test]
    fn stop_and_revoke_systems_fail_closed() {
        for s in [System::Teardown, System::KillSwitch, System::SecretRotation] {
            let e = typed_plan_for(s, 100).unwrap_err();
            assert_eq!(e, TypedRefusal::NotExpressible(s));
            assert_eq!(e.code(), "not-expressible");
            assert_eq!(typed_action(s), None, "{s}");
        }
    }

    #[test]
    fn non_hades_systems_are_refused() {
        let foreign: Vec<System> = System::ALL
            .iter()
            .copied()
            .filter(|s| s.function() != FunctionId::Hades)
            .collect();
        assert!(!foreign.is_empty());
        for s in foreign {
            assert_eq!(typed_plan_for(s, 100), Err(TypedRefusal::NotHades(s)));
            assert_eq!(typed_action(s), None);
        }
    }

    #[test]
    fn typed_twin_covers_every_approvable_hades_system() {
        // Each HADES system the gate can approve has a typed plan, and only those.
        for s in System::ALL
            .iter()
            .filter(|s| s.function() == FunctionId::Hades)
        {
            assert_eq!(
                typed_plan_for(*s, 100).is_ok(),
                typed_action(*s).is_some(),
                "{s}"
            );
        }
    }
}
