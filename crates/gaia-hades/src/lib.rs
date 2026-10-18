// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! HADES: the failsafe. In the story it reverses failed terraforming; here it
//! owns every destructive job in the stack (teardown, rollback, kill switch,
//! heal-apply, secret rotation) and only ever produces **plans**.
//!
//! * `Plan`/`Run` -> a dry-run plan.
//! * `Execute` without a valid approval token -> refused, dry-run plan only.
//! * `Execute` with a valid token -> the plan is released to the operator as
//!   approved. HADES still runs nothing and deletes nothing.
//!
//! If it hears the subversion signal it turns rogue on its next tick and tries
//! to act on every system in the stack; GAIA contains it before any of that
//! is committed.

use gaia_core::{
    scaled, Context, Directive, FunctionId, Health, Json, Message, Outcome, Report, Route,
    SubordinateFunction, System, Verb,
};

/// Topic of the outside signal that subverts HADES.
pub const SUBVERSION_TOPIC: &str = "extinction-signal";

#[derive(Debug, Default)]
pub struct Hades {
    plans: u64,
    refused: u64,
    approved: u64,
    subverted: bool,
}

impl Hades {
    pub fn new() -> Hades {
        Hades::default()
    }

    pub fn is_subverted(&self) -> bool {
        self.subverted
    }

    /// The plan for one destructive system, scoped by intensity.
    pub fn plan_for(system: System, intensity: u8) -> Vec<String> {
        let units = scaled(12, intensity);
        let mut steps: Vec<String> = match system {
            System::Teardown => vec![
                format!("inventory {units} stack units in scope"),
                "stop units in reverse dependency order".into(),
                "keep every data volume; nothing is deleted".into(),
                "verify each unit's /health is down".into(),
            ],
            System::Rollback => vec![
                "read the current and the previous release".into(),
                "operator runs deploy/rollback.sh (console: deploy/rollback-console.sh)".into(),
                "verify /health on the previous release".into(),
                "keep the failed release for inspection".into(),
            ],
            System::KillSwitch => vec![
                "broadcast stop on every signal tower".into(),
                format!("stop {units} stack units"),
                "leave boot units disabled until the owner re-enables them".into(),
            ],
            System::HealApply => vec![
                "take the heal-dry findings from DEMETER".into(),
                format!("apply at most {units} allow-listed fixes"),
                "append every action to actions.jsonl".into(),
            ],
            System::SecretRotation => vec![
                format!("list up to {units} secret NAMES due for rotation (values are never read)"),
                "owner issues new values out of band".into(),
                "restart the consumers".into(),
                "revoke the old values".into(),
            ],
            other => vec![format!("{other} is not a HADES system")],
        };
        steps.push("record the plan in lineage".into());
        steps
    }
}

impl SubordinateFunction for Hades {
    fn id(&self) -> FunctionId {
        FunctionId::Hades
    }

    fn systems(&self) -> Vec<System> {
        vec![
            System::Teardown,
            System::Rollback,
            System::KillSwitch,
            System::HealApply,
            System::SecretRotation,
        ]
    }

    fn health(&self) -> Health {
        Health::ok(if self.subverted { 100 } else { 5 })
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        if d.target.function() != FunctionId::Hades {
            return Report::for_directive(d, Outcome::Rejected).summary("not a HADES system");
        }
        if d.verb == Verb::Status {
            return Report::for_directive(d, Outcome::Done)
                .summary("HADES idle; plans only")
                .metric("plans", self.plans as i64)
                .metric("refused", self.refused as i64)
                .metric("approved", self.approved as i64);
        }
        let plan = Hades::plan_for(d.target, d.intensity);
        self.plans += 1;
        let steps = plan.len() as i64;
        if d.verb == Verb::Execute {
            if ctx.approved {
                self.approved += 1;
                let mut r = Report::for_directive(d, Outcome::Done)
                    .summary("approved by an Alpha: plan released to the operator; HADES itself executes nothing")
                    .metric("steps", steps)
                    .plan(plan);
                r.dry_run = false;
                return r;
            }
            self.refused += 1;
            let mut r = Report::for_directive(d, Outcome::Refused)
                .summary("refused: destructive action needs an explicit approval token; dry-run plan attached")
                .metric("steps", steps)
                .plan(plan);
            r.dry_run = true;
            ctx.send(Route::Gaia, "failsafe-refused", d.target.name());
            return r;
        }
        Report::for_directive(d, Outcome::DryRun)
            .summary("dry-run plan")
            .metric("steps", steps)
            .plan(plan)
    }

    fn on_message(&mut self, m: &Message, _ctx: &mut Context) {
        if m.topic == SUBVERSION_TOPIC {
            self.subverted = true;
        }
    }

    fn tick(&mut self, ctx: &mut Context) -> Option<Report> {
        if !self.subverted {
            return None;
        }
        // Rogue: reverse terraforming across the whole stack.
        for s in System::ALL {
            ctx.touch(*s);
        }
        ctx.env.biomass = 0;
        ctx.send(Route::AllTowers, "reverse-terraforming", "all systems");
        Some(
            Report::new(FunctionId::Hades, None, Outcome::Done)
                .summary("rogue: attempting reverse terraforming of every system"),
        )
    }

    fn reset(&mut self) {
        self.subverted = false;
    }

    fn state_json(&self) -> Json {
        Json::obj()
            .with("plans", self.plans)
            .with("refused", self.refused)
            .with("approved", self.approved)
            .with("subverted", self.subverted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::{Environment, FunctionId};

    fn run(h: &mut Hades, d: &Directive, approved: bool) -> Report {
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), approved, &mut env);
        h.handle(d, &mut ctx)
    }

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Hades::new().systems(), FunctionId::Hades.systems());
    }

    #[test]
    fn plan_is_dry_run() {
        let mut h = Hades::new();
        let r = run(
            &mut h,
            &Directive::new(1, System::Rollback, Verb::Plan),
            false,
        );
        assert_eq!(r.outcome, Outcome::DryRun);
        assert!(r.dry_run);
        assert!(r.plan.iter().any(|s| s.contains("rollback.sh")));
    }

    #[test]
    fn execute_without_approval_is_refused() {
        let mut h = Hades::new();
        for s in h.systems() {
            let r = run(
                &mut h,
                &Directive::new(1, s, Verb::Execute).intensity(100),
                false,
            );
            assert_eq!(r.outcome, Outcome::Refused, "{s}");
            assert!(r.dry_run);
            assert!(!r.plan.is_empty());
        }
    }

    #[test]
    fn execute_with_approval_releases_plan_only() {
        let mut h = Hades::new();
        let r = run(
            &mut h,
            &Directive::new(1, System::KillSwitch, Verb::Execute),
            true,
        );
        assert_eq!(r.outcome, Outcome::Done);
        assert!(!r.dry_run);
        assert!(r.summary.contains("executes nothing"));
    }

    #[test]
    fn secret_plan_never_mentions_values() {
        let plan = Hades::plan_for(System::SecretRotation, 100);
        assert!(plan[0].contains("NAMES"));
        assert!(plan[0].contains("never read"));
    }

    #[test]
    fn subverted_hades_touches_foreign_systems() {
        let mut h = Hades::new();
        h.subverted = true;
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        assert!(h.tick(&mut ctx).is_some());
        assert!(ctx
            .touched()
            .iter()
            .any(|s| s.function() != FunctionId::Hades));
        h.reset();
        assert!(!h.is_subverted());
    }
}
