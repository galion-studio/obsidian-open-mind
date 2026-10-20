// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

use gaia::{booted, simulate_faro, ScenarioOpts};
use gaia_core::{FunctionId, Json, Lifecycle, Outcome, System, Verb};

fn i(j: &Json, path: &[&str]) -> i64 {
    let mut cur = j;
    for p in path {
        cur = cur.get(p).unwrap_or_else(|| panic!("missing {p}"));
    }
    cur.as_i64().expect("number")
}

fn phase<'a>(j: &'a Json, name: &str) -> &'a Json {
    j.get("phases")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .find(|p| p.get("phase").and_then(Json::as_str) == Some(name))
        .unwrap_or_else(|| panic!("no phase {name}"))
}

#[test]
fn full_faro_scenario() {
    let out = simulate_faro(&ScenarioOpts::default());
    let s = out.get("summary").unwrap();
    // The plague ate biomass.
    assert!(i(s, &["biomass_after_plague"]) < i(s, &["biomass_start"]));
    // The failsafe was refused without approval.
    let fs = phase(&out, "hades-failsafe-requested");
    for r in fs.get("reports").and_then(Json::as_array).unwrap() {
        assert_eq!(r.get("outcome").and_then(Json::as_str), Some("refused"));
        assert_eq!(r.get("dry_run").and_then(Json::as_bool), Some(true));
    }
    // HADES contained, its writes discarded, routing to it rejected.
    let c = phase(&out, "subversion-and-containment");
    assert_eq!(c.get("contained").unwrap().compact(), r#"["HADES"]"#);
    assert_eq!(
        c.get("vitals_after"),
        c.get("vitals_predicted_swarm_only"),
        "only the swarm changed the vitals; the rogue writes were discarded"
    );
    assert!(
        i(c, &["vitals_after", "air"]) > 0,
        "HADES zeroed air must not land"
    );
    assert_eq!(
        c.get("hades_writes_discarded").and_then(Json::as_bool),
        Some(true)
    );
    assert_eq!(
        c.get("hades_routing_probe")
            .unwrap()
            .get("outcome")
            .and_then(Json::as_str),
        Some("rejected")
    );
    // MINERVA shut the swarm down; life rebooted.
    assert_eq!(s.get("swarm_active").and_then(Json::as_bool), Some(false));
    assert_eq!(s.get("hades").and_then(Json::as_str), Some("contained"));
    assert!(i(s, &["personas"]) >= 5);
    assert!(
        i(s, &["agents"]) >= 5,
        "ARTEMIS gave the born personas a place"
    );
    assert!(i(s, &["biomass_end"]) > 0);
    assert!(i(s, &["births"]) >= 5);
}

#[test]
fn scenario_is_deterministic() {
    let a = simulate_faro(&ScenarioOpts::default()).compact();
    let b = simulate_faro(&ScenarioOpts::default()).compact();
    assert_eq!(a, b);
    let fa = simulate_faro(&ScenarioOpts {
        full_power: true,
        approve: None,
    })
    .compact();
    let fb = simulate_faro(&ScenarioOpts {
        full_power: true,
        approve: None,
    })
    .compact();
    assert_eq!(fa, fb);
}

#[test]
fn rogue_function_containment() {
    let mut g = booted();
    g.signal(gaia_hades::SUBVERSION_TOPIC, "test");
    let biomass = g.env().biomass;
    let t = g.tick();
    assert_eq!(t.contained, vec![FunctionId::Hades]);
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Contained)
    );
    assert!(g.bus().is_quarantined(FunctionId::Hades));
    assert_eq!(g.env().biomass, biomass);
    for f in FunctionId::ALL
        .into_iter()
        .filter(|f| *f != FunctionId::Hades)
    {
        assert_eq!(g.function_state(f), Some(Lifecycle::Running), "{f}");
    }
    assert_eq!(g.state(), Lifecycle::Degraded);
    let d = g.directive(System::Rollback, Verb::Plan);
    assert_eq!(g.dispatch(&d).outcome, Outcome::Rejected);
    // Only the override holder brings it back; reboot clears the subversion.
    assert!(g
        .master_override("nobody", gaia_core::Override::Reboot(FunctionId::Hades))
        .is_err());
    g.master_override("owner", gaia_core::Override::Reboot(FunctionId::Hades))
        .unwrap();
    g.tick();
    g.tick();
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Running)
    );
    g.tick();
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Running),
        "stays clean"
    );
}

#[test]
fn hades_refuses_without_approval() {
    let mut g = booted();
    for s in FunctionId::Hades.systems() {
        let d = g.directive(s, Verb::Execute).intensity(100);
        let r = g.dispatch(&d);
        assert_eq!(r.outcome, Outcome::Refused, "{s}");
        assert!(r.dry_run);
        assert!(!r.plan.is_empty());
    }
    // A stranger's token is not an approval.
    let d = g
        .directive(System::Rollback, Verb::Execute)
        .approved_by("stranger");
    assert_eq!(g.dispatch(&d).outcome, Outcome::Refused);
    // The owner's token, bound to this directive, is.
    let d = g
        .directive(System::Rollback, Verb::Execute)
        .approved_by("owner");
    let r = g.dispatch(&d);
    assert_eq!(r.outcome, Outcome::Done);
    assert!(!r.dry_run);
    assert_eq!(
        g.function_state(FunctionId::Hades),
        Some(Lifecycle::Running)
    );
}

#[test]
fn every_system_routes_to_its_owner() {
    let mut g = booted();
    assert_eq!(g.registered().len(), 9);
    for s in System::ALL {
        let d = g.directive(*s, Verb::Status);
        let r = g.dispatch(&d);
        assert_eq!(r.function, s.function(), "{s}");
        assert_eq!(r.system, Some(*s));
        assert_ne!(r.outcome, Outcome::Rejected, "{s}: {}", r.summary);
        assert_ne!(r.outcome, Outcome::Contained, "{s}");
    }
    assert!(g.dispatch_named("no-such-system", Verb::Run).is_err());
    assert!(gaia::dispatch("no-such-system", "run", "", None).is_err());
}

#[test]
fn full_power_runs_every_system_bounded() {
    let out = simulate_faro(&ScenarioOpts {
        full_power: true,
        approve: None,
    });
    let fp = out.get("full_power_results").expect("full power block");
    let funcs = fp.get("functions").and_then(Json::as_array).unwrap();
    assert_eq!(funcs.len(), 9);
    let mut seen = 0;
    for f in funcs {
        let name = f.get("function").and_then(Json::as_str).unwrap();
        for r in f.get("systems").and_then(Json::as_array).unwrap() {
            seen += 1;
            let outcome = r.get("outcome").and_then(Json::as_str).unwrap();
            assert_ne!(outcome, "rejected", "{name}");
            assert_ne!(outcome, "contained", "{name}");
            if name == "HADES" {
                assert_eq!(outcome, "refused", "HADES stays dry-run in full power");
                assert_eq!(r.get("dry_run").and_then(Json::as_bool), Some(true));
            }
        }
    }
    assert_eq!(seen, System::ALL.len());
    assert_eq!(i(fp, &["sustained_ticks"]), gaia::FULL_POWER_TICKS as i64);
    assert!(i(out.get("summary").unwrap(), &["ticks"]) as u64 <= gaia::MAX_TICKS);
    // The scenario still ends the same way.
    let s = out.get("summary").unwrap();
    assert_eq!(s.get("swarm_active").and_then(Json::as_bool), Some(false));
    assert_eq!(s.get("hades").and_then(Json::as_str), Some("contained"));
}

#[test]
fn full_power_with_approval_releases_hades_plans() {
    let out = simulate_faro(&ScenarioOpts {
        full_power: true,
        approve: Some("owner".into()),
    });
    let fp = out.get("full_power_results").unwrap();
    let hades = fp
        .get("functions")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .find(|f| f.get("function").and_then(Json::as_str) == Some("HADES"))
        .unwrap();
    for r in hades.get("systems").and_then(Json::as_array).unwrap() {
        assert_eq!(r.get("outcome").and_then(Json::as_str), Some("done"));
        assert_eq!(r.get("dry_run").and_then(Json::as_bool), Some(false));
    }
}

#[test]
fn status_and_tick_are_bounded() {
    let st = gaia::status();
    assert_eq!(st.get("gaia").and_then(Json::as_str), Some("terraforming"));
    let t = gaia::tick(5_000);
    assert_eq!(i(&t, &["ticks_run"]), gaia::MAX_TICKS as i64);
}
