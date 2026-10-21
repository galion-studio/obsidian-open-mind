// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia S3.4 `classify` on synthetic evidence shaped like the live VPS
//! nerve `/state` and cortex `/incidents` of 2026-10-21: agreement,
//! every finding kind, plan-only heal (no apply, no token, no POST).

use gaia::classify::{classify, parse_port_map, Class, Inputs};
use gaia_core::System;
use gaia_live::{ProbeResult, Remote, State};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn r(name: &str, unit: Option<&str>, system: System, state: State) -> ProbeResult {
    ProbeResult {
        name: name.into(),
        system,
        state,
        http: None,
        unit: unit.map(|u| (u.to_string(), "active".into())),
        file_age_s: None,
        notes: vec![],
    }
}

fn comp(name: &str, unit: Option<&str>, status: &str, f: &str, sys: &str, r1h: u64) -> Value {
    json!({"name": name, "unit": unit, "status": status, "reasons": [],
           "restarts_1h": r1h, "gaia": {"function": f, "system": sys}})
}

fn nerve() -> Value {
    json!({
        "cycle": 17,
        "components": [
            comp("openmind-memory", Some("openmind-memory.service"), "healthy", "APOLLO", "memory-gateway", 0),
            comp("link-watch-rs", Some("link-watch-rs.service"), "healthy", "APOLLO", "link-watch", 0),
            comp("galion-one", Some("galion-one.service"), "healthy", "ELEUTHIA", "galion-one", 0),
            comp("galion-one-mouth", None, "degrading", "AETHER", "speech-io", 1),
            comp("cloudflared", Some("cloudflared.service"), "failing-soon", "AETHER", "tunnels", 0),
            comp("galion-health", Some("galion-health.service"), "healthy", "DEMETER", "guardian-tick", 0),
            comp("voice-worker", Some("galion-voice.service"), "absent", "AETHER", "voice-relay", 0),
        ],
        "incidents": {"open": [
            {"id": "inc-edge", "cause": "edge", "components": ["cloudflared"], "severity": "error"},
            {"id": "inc-mouth", "cause": "galion-one", "components": ["galion-one-mouth"], "severity": "error"}
        ]},
        "gaia": {
            "APOLLO": {"status": "healthy"},
            "AETHER": {"status": "failing-soon"},
            "MINERVA": {"status": "no-live-systems"},
            "ELEUTHIA": {"status": "healthy"},
            "DEMETER": {"status": "healthy"}
        },
        "system": {"status": "degraded", "score": 66},
        "reflex": {"budgets": {"galion-one.service": {"live_1h": 0}}}
    })
}

fn probes() -> Vec<ProbeResult> {
    let mut mouth = r("galion-one-mouth", None, System::SpeechIo, State::Down);
    mouth.notes = vec!["GET :8002/health -> HTTP 503, status not ok".into()];
    vec![
        r(
            "openmind-memory",
            Some("openmind-memory.service"),
            System::MemoryGateway,
            State::Up,
        ),
        r(
            "link-watch",
            Some("link-watch-rs.service"),
            System::LinkWatch,
            State::Up,
        ),
        r(
            "galion-one",
            Some("galion-one.service"),
            System::GalionOne,
            State::Up,
        ),
        mouth,
        r(
            "cloudflared",
            Some("cloudflared.service"),
            System::Tunnels,
            State::Up,
        ),
        r(
            "galion-health",
            Some("galion-health.service"),
            System::GuardianTick,
            State::Up,
        ),
        r(
            "gaia-cortex",
            Some("gaia-cortex.service"),
            System::GuardianWatchdog,
            State::Up,
        ),
    ]
}

fn ports() -> BTreeMap<String, (u16, String)> {
    [
        ("galion-one-mouth", 8002u16),
        ("openmind-memory", 8420),
        ("galion-one", 8091),
    ]
    .into_iter()
    .map(|(n, p)| (n.to_string(), (p, "/health".to_string())))
    .collect()
}

fn run(inp: &Inputs) -> Value {
    serde_json::from_str(&classify(inp).compact()).expect("classify emits JSON")
}

fn live_inputs() -> Inputs {
    Inputs {
        probes: probes(),
        ports: ports(),
        nerve_state: Ok(nerve()),
        cortex_health: Ok(json!({"ok": true, "mode": "live", "incidents_open": 0})),
        cortex_incidents: Ok(json!({"open": [], "resolved": [], "units": {}})),
    }
}

fn unit<'a>(j: &'a Value, name: &str) -> &'a Value {
    j["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["service"] == name)
        .unwrap_or_else(|| panic!("no unit {name}"))
}

fn kinds(j: &Value, subject: &str) -> Vec<String> {
    j["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["subject"] == subject)
        .map(|f| f["kind"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn class_words() {
    assert_eq!(Class::of_nerve("healthy"), Class::Ok);
    assert_eq!(Class::of_nerve("failing-soon"), Class::Warn);
    assert_eq!(Class::of_nerve("degrading"), Class::Warn);
    assert_eq!(Class::of_nerve("failing"), Class::Bad);
    assert_eq!(Class::of_nerve("absent"), Class::Absent);
    assert_eq!(Class::of_nerve("no-live-systems"), Class::Absent);
    assert_eq!(Class::of_nerve("weird"), Class::Unknown);
    assert_eq!(Class::of_gaia(&State::Degraded), Class::Warn);
}

#[test]
fn live_shape_agreement_and_findings() {
    let j = run(&live_inputs());
    assert_eq!(j["mode"], "live-classify-read-only");
    assert_eq!(j["summary"]["units"], 7);
    // memory, link-watch (matched by unit, not name), galion-one (the mouth's
    // incident has cause galion-one but does not name it), galion-health agree.
    for ok in [
        "openmind-memory",
        "link-watch",
        "galion-one",
        "galion-health",
    ] {
        assert_eq!(
            unit(&j, ok)["agree"],
            true,
            "{ok}: {}",
            kinds(&j, ok).join(",")
        );
    }
    assert_eq!(
        unit(&j, "link-watch")["nerve"]["component"],
        "link-watch-rs"
    );
    assert_eq!(j["summary"]["agree"], 4);
    assert_eq!(j["summary"]["disagree"], 3);
    let mouth = kinds(&j, "galion-one-mouth");
    assert!(mouth.contains(&"gaia-nerve-disagree".into()), "{mouth:?}");
    assert!(mouth.contains(&"cortex-missed".into()), "{mouth:?}");
    let cf = kinds(&j, "cloudflared");
    assert!(cf.contains(&"gaia-nerve-disagree".into()) && cf.contains(&"cortex-missed".into()));
    assert_eq!(
        kinds(&j, "gaia-cortex"),
        vec![
            "nerve-blind".to_string(),
            "cortex-nerve-incident-gap".to_string()
        ]
    );
    assert_eq!(kinds(&j, "voice-worker"), vec!["gaia-blind".to_string()]);
    assert!(kinds(&j, "MINERVA").contains(&"function-disagree".into()));
    assert!(kinds(&j, "AETHER").contains(&"function-disagree".into()));
    assert!(kinds(&j, "APOLLO").is_empty());
    assert!(
        !kinds(&j, "gaia-cortex").contains(&"cortex-mode".into()),
        "mode live is not a finding"
    );
    let gap = j["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "cortex-nerve-incident-gap");
    assert!(gap);
    assert_eq!(j["overall"]["nerve_open_incidents"], 2);
    assert_eq!(j["overall"]["cortex_open_incidents"], 0);
    assert_eq!(j["overall"]["nerve_score"], 66);
}

#[test]
fn heal_is_plan_only_and_refused_for_live_ports() {
    let j = run(&live_inputs());
    let h = &unit(&j, "galion-one-mouth")["heal_tick"];
    assert_eq!(h["mode"], "plan-only");
    assert_eq!(h["verdict"], "refused");
    assert_eq!(h["applied"], false);
    assert_eq!(h["token_used"], false);
    assert_eq!(h["nerve_event_posted"], false);
    assert!(h["reason"]
        .as_str()
        .unwrap()
        .contains("port 8002 is outside"));
    // The mouth has no unit; D-L2 matches the nerve component by name: 1 used, 2 left.
    assert_eq!(h["nerve_restarts_1h"], 1);
    assert_eq!(h["cap_left_for_gaia"], 2);
    // cloudflared: no port, still plan-only.
    let c = &unit(&j, "cloudflared")["heal_tick"];
    assert!(c["reason"].as_str().unwrap().contains("no HTTP port"));
    // Healthy units get no heal block at all.
    assert!(unit(&j, "openmind-memory").get("heal_tick").is_none());
    // Nothing anywhere says applied/token used.
    let text = j.to_string();
    assert!(!text.contains("\"applied\":true") && !text.contains("\"token_used\":true"));
}

#[test]
fn unreadable_nerve_and_cortex_are_loud() {
    let mut inp = live_inputs();
    inp.nerve_state = Err("nerve connect 127.0.0.1:20945: ConnectionRefused".into());
    inp.cortex_incidents = Err("GET /incidents: HTTP 404".into());
    let j = run(&inp);
    let fk: Vec<&str> = j["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert!(fk.contains(&"nerve-unreadable") && fk.contains(&"cortex-unreadable"));
    assert!(!fk.contains(&"nerve-blind"), "no nerve => no blind claims");
    // Cap fails closed without the nerve.
    let h = &unit(&j, "galion-one-mouth")["heal_tick"];
    assert_eq!(h["cap_left_for_gaia"], 0);
    assert!(h["cap_error"].as_str().unwrap().contains("failing closed"));
    assert_eq!(unit(&j, "openmind-memory")["nerve"]["verdict"], "unknown");
}

#[test]
fn cortex_incident_and_dry_run_mode() {
    let mut inp = live_inputs();
    inp.cortex_incidents = Ok(json!({"open": [
        {"id": "inc-c1", "components": ["galion-one-mouth"]},
        {"id": "inc-c2", "components": ["cloudflared"]}
    ]}));
    inp.cortex_health = Ok(json!({"ok": true, "mode": "dry-run"}));
    let j = run(&inp);
    assert!(!kinds(&j, "galion-one-mouth").contains(&"cortex-missed".into()));
    assert_eq!(unit(&j, "galion-one-mouth")["cortex"]["verdict"], "warn");
    assert_eq!(
        unit(&j, "galion-one-mouth")["cortex"]["open_incidents"][0],
        "inc-c1"
    );
    assert!(kinds(&j, "gaia-cortex").contains(&"cortex-mode".into()));
    assert!(!j.to_string().contains("cortex-nerve-incident-gap"));
}

#[test]
fn ownership_disagreement_is_reported() {
    let mut inp = live_inputs();
    let mut n = nerve();
    n["components"][0]["gaia"]["function"] = json!("MINERVA");
    inp.nerve_state = Ok(n);
    let j = run(&inp);
    assert!(kinds(&j, "openmind-memory").contains(&"ownership-disagree".into()));
}

#[test]
fn hard_disagreement_is_error() {
    let mut inp = live_inputs();
    inp.probes[0].state = State::Down; // memory down for Gaia, healthy for the nerve
    let j = run(&inp);
    let f = j["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["subject"] == "openmind-memory" && f["kind"] == "gaia-nerve-disagree")
        .unwrap();
    assert_eq!(f["severity"], "error");
    assert!(unit(&j, "openmind-memory")["heal_tick"]["reason"]
        .as_str()
        .unwrap()
        .contains("8420"));
}

#[test]
fn port_map_and_evidence_parsing() {
    let m = parse_port_map("8420=20940,8670=20945").unwrap();
    assert_eq!(m.get(&8420), Some(&20940));
    assert!(parse_port_map("8420").is_err());
    assert!(parse_port_map("8420=x").is_err());
    let mut rm = Remote::default();
    rm.parse_evidence("# c\nunit a.service active\nfile /x/link.json 12\nfile /y missing\n")
        .unwrap();
    assert_eq!(rm.units.as_ref().unwrap()["a.service"], "active");
    assert_eq!(
        rm.file_ages.as_ref().unwrap()[std::path::Path::new("/x/link.json")],
        Some(12)
    );
    assert_eq!(
        rm.file_ages.as_ref().unwrap()[std::path::Path::new("/y")],
        None
    );
    assert!(Remote::default().parse_evidence("unit a").is_err());
    assert!(Remote::default().parse_evidence("file /a old").is_err());
    assert!(Remote::default().parse_evidence("bogus x y").is_err());
}

#[test]
fn remote_evidence_drives_probe_verdicts() {
    // Units and file ages from a capture; no HTTP; uncaptured unit is bad.
    let probes = gaia_live::classify_probes();
    let lw = probes.iter().find(|p| p.name == "link-watch").unwrap();
    let mut rm = Remote::default();
    rm.parse_evidence(
        "unit link-watch-rs.service active\nfile /opt/galion/galion-light/state/link.json 5\n",
    )
    .unwrap();
    let o = gaia_live::Opts::default();
    let res = gaia_live::probe_with(lw, &o, &rm);
    assert_eq!(res.state, State::Up, "{:?}", res.notes);
    let mut stale = Remote::default();
    stale
        .parse_evidence("unit link-watch-rs.service failed\nfile /opt/galion/galion-light/state/link.json 9999\n")
        .unwrap();
    let res = gaia_live::probe_with(lw, &o, &stale);
    assert_eq!(res.state, State::Down, "{:?}", res.notes);
    let cf = probes.iter().find(|p| p.name == "cloudflared").unwrap();
    let res = gaia_live::probe_with(cf, &o, &rm);
    assert_eq!(res.unit.as_ref().unwrap().1, "not-captured");
    assert_eq!(res.state, State::Down);
}

#[test]
fn classify_probe_set_covers_the_live_ports() {
    let ports: Vec<u16> = gaia_live::classify_probes()
        .iter()
        .filter_map(|p| p.http.as_ref().map(|h| h.port))
        .collect();
    for p in [8420, 8642, 8650, 8091, 8080, 8670, 8674, 8646, 8002, 8660] {
        assert!(ports.contains(&p), "{p}");
    }
    assert_eq!(
        ports.len(),
        10,
        "fits the slot's 10 tunnel ports 20940-20949"
    );
}

#[test]
fn cortex_prefixed_components_match_live_shape() {
    // Live 2026-10-21 23:06: the cortex mirrors nerve incidents as
    // `nerve-incident:<cause>` and unit failures as `unit:<unit>`.
    let mut inp = live_inputs();
    inp.cortex_incidents = Ok(json!({"open": [
        {"id": "inc-c-edge", "severity": "error", "components": ["nerve-incident:edge"], "cause": "nerve-incident:edge"},
        {"id": "inc-c-unit", "severity": "error", "components": ["unit:gaia-hermes-handoff.path.service"],
         "title": "unit:gaia-hermes-handoff.path.service (1 signal(s))"}
    ], "units": {
        "gaia-hermes-handoff.path.service": {"circuit_open": false, "failures_window": 1},
        "openmind-memory.service": {"circuit_open": false, "failures_window": 0}
    }}));
    let j = run(&inp);
    // cloudflared's nerve incident has cause `edge`: the cortex has it.
    assert_eq!(
        unit(&j, "cloudflared")["cortex"]["open_incidents"][0],
        "inc-c-edge"
    );
    assert!(!kinds(&j, "cloudflared").contains(&"cortex-missed".into()));
    // The mouth's nerve incident (cause galion-one) is still missed.
    assert!(kinds(&j, "galion-one-mouth").contains(&"cortex-missed".into()));
    // A unit row with no failures and a closed circuit is not a warning.
    assert_eq!(unit(&j, "openmind-memory")["cortex"]["verdict"], "ok");
    // The cortex-only unit incident is reported as Gaia-blind.
    let f = j["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["subject"] == "gaia-hermes-handoff.path.service")
        .expect("cortex-only incident finding");
    assert_eq!(f["kind"], "gaia-blind");
    assert!(f["detail"].as_str().unwrap().contains("inc-c-unit"));
}

#[test]
fn cortex_unit_circuit_open_is_warn() {
    let mut inp = live_inputs();
    inp.cortex_incidents = Ok(json!({"open": [], "units": {
        "openmind-memory.service": {"circuit_open": true, "failures_window": 0}
    }}));
    let j = run(&inp);
    assert_eq!(unit(&j, "openmind-memory")["cortex"]["verdict"], "warn");
}

#[test]
fn cortex_only_alarm_on_healthy_unit_is_a_disagreement() {
    // Live 2026-10-21 23:09: cortex open `nerve-incident:hermes-rs` while
    // Gaia and the nerve's component row both say hermes-rs is healthy.
    let mut inp = live_inputs();
    inp.probes.push(r(
        "hermes-rs",
        Some("hermes-rs.service"),
        System::HermesAgents,
        State::Up,
    ));
    if let Ok(n) = inp.nerve_state.as_mut() {
        n["components"].as_array_mut().unwrap().push(comp(
            "hermes-rs",
            Some("hermes-rs.service"),
            "healthy",
            "ARTEMIS",
            "hermes-agents",
            0,
        ));
    }
    inp.cortex_incidents = Ok(json!({"open": [
        {"id": "inc-h", "severity": "warn", "components": ["nerve-incident:hermes-rs"]}
    ]}));
    let j = run(&inp);
    let h = unit(&j, "hermes-rs");
    assert_eq!(h["cortex"]["verdict"], "warn");
    assert_eq!(h["agree"], false);
    assert_eq!(kinds(&j, "hermes-rs"), vec!["cortex-disagree".to_string()]);
}
