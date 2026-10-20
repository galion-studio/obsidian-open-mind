// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

use super::*;

/// A configurable mock function.
struct Mock {
    id: FunctionId,
    /// On tick, touch this foreign system.
    rogue_touch: Option<System>,
    /// Execute destructive work even without approval.
    ignore_gate: bool,
    resets: u32,
}

impl Mock {
    fn new(id: FunctionId) -> Box<Mock> {
        Box::new(Mock {
            id,
            rogue_touch: None,
            ignore_gate: false,
            resets: 0,
        })
    }
}

impl SubordinateFunction for Mock {
    fn id(&self) -> FunctionId {
        self.id
    }
    fn systems(&self) -> Vec<System> {
        self.id.systems()
    }
    fn health(&self) -> Health {
        Health::ok(10)
    }
    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        if d.target.destructive() && d.verb == Verb::Execute && !ctx.approved && !self.ignore_gate {
            return Report::for_directive(d, Outcome::Refused);
        }
        ctx.env.knowledge += 1;
        Report::for_directive(d, Outcome::Done).summary("ok")
    }
    fn on_message(&mut self, m: &Message, _ctx: &mut Context) {
        if m.topic == "subvert" && self.id == FunctionId::Hades {
            self.rogue_touch = Some(System::MemoryGateway);
        }
    }
    fn tick(&mut self, ctx: &mut Context) -> Option<Report> {
        if let Some(s) = self.rogue_touch {
            ctx.touch(s);
            ctx.env.biomass = 0;
        }
        None
    }
    fn reset(&mut self) {
        self.rogue_touch = None;
        self.resets += 1;
    }
}

fn booted(ids: &[FunctionId]) -> Gaia {
    let mut g = Gaia::default();
    for id in ids {
        g.register(Mock::new(*id)).unwrap();
    }
    g.boot();
    g.tick();
    g
}

#[test]
fn boot_lifecycle_dormant_booting_running() {
    let mut g = Gaia::default();
    g.register(Mock::new(FunctionId::Apollo)).unwrap();
    assert_eq!(
        g.function_state(FunctionId::Apollo),
        Some(Lifecycle::Dormant)
    );
    g.boot();
    assert_eq!(
        g.function_state(FunctionId::Apollo),
        Some(Lifecycle::Booting)
    );
    let t = g.tick();
    assert_eq!(
        g.function_state(FunctionId::Apollo),
        Some(Lifecycle::Running)
    );
    assert_eq!(g.state(), Lifecycle::Running);
    assert_eq!(t.transitions.len(), 1);
}

#[test]
fn register_rejects_wrong_declaration_and_duplicates() {
    struct Liar;
    impl SubordinateFunction for Liar {
        fn id(&self) -> FunctionId {
            FunctionId::Apollo
        }
        fn systems(&self) -> Vec<System> {
            vec![System::MemoryGateway, System::Rollback]
        }
        fn health(&self) -> Health {
            Health::ok(0)
        }
        fn handle(&mut self, d: &Directive, _c: &mut Context) -> Report {
            Report::for_directive(d, Outcome::Done)
        }
    }
    let mut g = Gaia::default();
    assert!(g.register(Box::new(Liar)).is_err());
    g.register(Mock::new(FunctionId::Apollo)).unwrap();
    assert!(g.register(Mock::new(FunctionId::Apollo)).is_err());
}

#[test]
fn routes_to_owner_and_rejects_unowned_and_unknown() {
    let mut g = booted(&[FunctionId::Apollo]);
    let r = g.dispatch_named("memory-gateway", Verb::Run).unwrap();
    assert_eq!(r.outcome, Outcome::Done);
    assert_eq!(r.function, FunctionId::Apollo);
    assert_eq!(g.env().knowledge, 1);
    let r = g.dispatch_named("cargo-build", Verb::Run).unwrap();
    assert_eq!(r.outcome, Outcome::Rejected, "HEPHAESTUS not registered");
    assert!(g.dispatch_named("warp-drive", Verb::Run).is_err());
}

#[test]
fn rogue_function_is_contained_and_its_effects_discarded() {
    let mut g = booted(&[FunctionId::Hades, FunctionId::Apollo]);
    let before = g.env().biomass;
    g.signal("subvert", "wake up");
    let t = g.tick();
    assert!(t.contained.contains(&FunctionId::Hades));
    // APOLLO heard the same signal but stayed inside its own domain.
    assert_eq!(
        g.function_state(FunctionId::Apollo),
        Some(Lifecycle::Running)
    );
    assert_eq!(g.env().biomass, before, "rogue writes are never committed");
    assert!(g.bus().is_quarantined(FunctionId::Hades));
    let d = g.directive(System::Rollback, Verb::Plan);
    assert_eq!(g.dispatch(&d).outcome, Outcome::Rejected);
}

#[test]
fn only_override_holder_reboots_a_contained_function() {
    let mut g = booted(&[FunctionId::Hades]);
    g.contain(FunctionId::Hades, "test", None);
    assert!(g
        .master_override("stranger", Override::Reboot(FunctionId::Hades))
        .is_err());
    g.master_override("owner", Override::Reboot(FunctionId::Hades))
        .unwrap();
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Rebooting)
    );
    g.tick();
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Booting)
    );
    g.tick();
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Running)
    );
    assert!(!g.bus().is_quarantined(FunctionId::Hades));
}

#[test]
fn destructive_execution_without_approval_is_contained() {
    let mut g = Gaia::default();
    let mut m = Mock::new(FunctionId::Hades);
    m.ignore_gate = true;
    g.register(m).unwrap();
    g.boot();
    g.tick();
    let d = g.directive(System::Teardown, Verb::Execute);
    let r = g.dispatch(&d);
    assert_eq!(r.outcome, Outcome::Contained);
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Contained)
    );
}

#[test]
fn approval_reaches_the_function_only_when_valid() {
    let mut g = booted(&[FunctionId::Hades]);
    let d = g.directive(System::Rollback, Verb::Execute);
    assert_eq!(g.dispatch(&d).outcome, Outcome::Refused);
    let d = g
        .directive(System::Rollback, Verb::Execute)
        .approved_by("owner");
    assert_eq!(g.dispatch(&d).outcome, Outcome::Done);
    let mut d = g
        .directive(System::Rollback, Verb::Execute)
        .approved_by("owner");
    d.id += 100; // token now bound to another directive
    assert_eq!(g.dispatch(&d).outcome, Outcome::Refused);
}

#[test]
fn swarm_consumes_biomass_and_run_is_deterministic() {
    let run = || {
        let mut g = booted(&[FunctionId::Apollo]);
        g.release_swarm(10);
        for _ in 0..20 {
            g.tick();
        }
        g.status_json().compact()
    };
    let a = run();
    assert_eq!(a, run());
    let mut g = booted(&[]);
    g.release_swarm(10);
    for _ in 0..60 {
        g.tick();
    }
    assert_eq!(g.env().biomass, 0);
    assert!(g.env().swarm <= Environment::SWARM_MAX);
}

#[test]
fn lineage_has_zagreb_timestamps() {
    let mut g = booted(&[FunctionId::Apollo]);
    g.dispatch_named("memory-gateway", Verb::Run).unwrap();
    for r in g.lineage().records() {
        assert!(
            r.at.ends_with("+02:00") || r.at.ends_with("+01:00"),
            "{}",
            r.at
        );
    }
    assert!(g.lineage().count("directive") >= 1);
}

#[test]
fn helpers() {
    assert_eq!(scaled(10, 100), 10);
    assert_eq!(scaled(10, 0), 1);
    assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
}
