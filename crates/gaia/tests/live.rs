// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia status --live` logic on synthetic probe results: degraded owners,
//! HADES plans only (never approved, never executed).

use gaia_core::{Json, System};
use gaia_live::{ProbeResult, State};

fn r(name: &str, system: System, state: State) -> ProbeResult {
    ProbeResult {
        name: name.into(),
        system,
        state,
        http: None,
        unit: Some((format!("{name}.service"), "active".into())),
        file_age_s: None,
        notes: vec![],
    }
}

fn s<'a>(j: &'a Json, k: &str) -> &'a str {
    j.get(k)
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("no {k}"))
}

#[test]
fn all_up_is_healthy_without_proposals() {
    let j = gaia::status_live(&[
        r("openmind-memory", System::MemoryGateway, State::Up),
        r("hermes-rs", System::HermesAgents, State::Up),
    ]);
    assert_eq!(s(&j, "mode"), "live-read-only");
    assert_eq!(s(&j, "overall"), "healthy");
    let p = j
        .get("hades")
        .unwrap()
        .get("proposals")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(p.is_empty());
    let apollo = j.get("functions").unwrap().get("APOLLO").unwrap();
    assert_eq!(s(apollo, "live_state"), "terraforming");
}

#[test]
fn down_service_degrades_owner_and_hades_only_plans() {
    let j = gaia::status_live(&[
        r("openmind-memory", System::MemoryGateway, State::Up),
        r("link-watch", System::LinkWatch, State::Down),
        r("console", System::Console, State::Degraded),
    ]);
    assert_eq!(s(&j, "overall"), "degraded");
    assert_eq!(
        j.get("summary").unwrap().get("down").unwrap().as_i64(),
        Some(1)
    );
    let f = j.get("functions").unwrap();
    assert_eq!(s(f.get("APOLLO").unwrap(), "live_state"), "degraded");
    assert_eq!(s(f.get("HEPHAESTUS").unwrap(), "live_state"), "degraded");
    let ps = j
        .get("hades")
        .unwrap()
        .get("proposals")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(ps.len(), 2);
    for p in ps {
        assert_eq!(s(p, "function"), "HADES");
        assert_eq!(p.get("approval_required").unwrap().as_bool(), Some(true));
        assert_eq!(p.get("approved").unwrap().as_bool(), Some(false));
        assert_eq!(p.get("executed").unwrap().as_bool(), Some(false));
        assert_ne!(
            s(p, "outcome"),
            "done",
            "a plan is never 'done' without approval"
        );
        let steps = p.get("steps").unwrap().as_array().unwrap();
        assert!(steps
            .iter()
            .any(|x| x.as_str().unwrap().contains("after approval")));
    }
    assert!(j.compact().contains("link-watch.service"));
}

#[test]
fn everything_down_is_down() {
    let j = gaia::status_live(&[r("hermes-rs", System::HermesAgents, State::Down)]);
    assert_eq!(s(&j, "overall"), "down");
}
