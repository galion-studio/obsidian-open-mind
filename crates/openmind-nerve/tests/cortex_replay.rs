// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! No-behaviour-change gate for the `Sensor` trait (cortex plan T1.2):
//! replaying `fixtures/cortex-replay` must give exactly the report and
//! markdown the pre-trait build (integration-1 8fd26f6) gave, apart from the
//! two additive finding keys `kind` and `source`. Also checks the B33/B41
//! event contract against `openmind_schema::NerveEvent`.

use openmind_nerve::config::Config;
use openmind_nerve::replay::{replay_dir, ReplayOut};
use openmind_schema::NerveEvent;
use serde_json::Value;
use std::path::PathBuf;

mod support;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn run() -> ReplayOut {
    let data = std::env::temp_dir().join(format!("nerve-cortex-replay-{}", std::process::id()));
    let d = data.display().to_string();
    let cfg = Config::from_env(&|k| (k == "NERVE_DATA_DIR").then(|| d.clone())).unwrap();
    replay_dir(&fixture().join("cortex-replay"), cfg).unwrap()
}

#[test]
fn replay_matches_the_pre_trait_build() {
    let out = run();
    let sh = out.engine.shared.lock().unwrap();
    let mut want: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture().join("cortex-replay.expected.json")).unwrap(),
    )
    .unwrap();
    support::canon(&mut want);
    let mut got = sh.report.clone();
    support::canon(&mut got);
    assert_eq!(got, want, "report differs from the pre-trait golden");
    assert_eq!(
        sh.report_md,
        support::expected_md(&fixture()),
        "markdown differs from the pre-trait golden"
    );
    assert_eq!(
        (
            want["score"].as_u64(),
            want["grade"].as_str(),
            want["pass"].as_bool()
        ),
        (Some(21), Some("F"), Some(false))
    );
}

#[test]
fn every_finding_names_its_kind_and_sensor() {
    let out = run();
    let cx = &out.engine.cortex;
    let want = [
        ("public-listener-9999", "vulnerability", "listeners"),
        ("unmanaged-listeners", "vulnerability", "listeners"),
        ("scanner-probes-console", "vulnerability", "scanners"),
        ("edge-dead-origins", "bleeding", "edge"),
        ("stuck-loading-galion-one-mouth", "bleeding", "components"),
        ("restarts-galion-desk", "bleeding", "components"),
        ("memory-heavy-console", "bleeding", "components"),
    ];
    for (id, kind, source) in want {
        let f = cx
            .findings()
            .find(|f| f.id == id)
            .unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!((f.kind.as_str(), f.source.as_str()), (kind, source), "{id}");
    }
    assert!(cx.ddos_firing);
    assert_eq!(
        out.engine.sensors.names(),
        ["listeners", "scanners", "edge", "components", "ddos"]
    );
    let ran: Vec<(&str, usize)> = out
        .engine
        .sensors
        .last
        .iter()
        .map(|r| (r.name, r.findings))
        .collect();
    assert_eq!(
        ran.iter().map(|r| r.0).collect::<Vec<_>>(),
        out.engine.sensors.names()
    );
    assert!(ran.iter().take(4).all(|r| r.1 > 0), "{ran:?}");
    assert!(!out.engine.live, "replay never reads the host");
}

#[test]
fn findings_are_valid_b33_nerve_events() {
    let out = run();
    let now = out.engine.now;
    let cx = &out.engine.cortex;
    for f in cx.findings() {
        let e: NerveEvent = serde_json::from_value(f.to_nerve_event(now)).expect("B33 shape");
        assert_eq!(e.ts, "2026-10-20T19:06:05+02:00");
        assert!(["vulnerability", "bleeding", "ddos"].contains(&e.kind.as_str()));
        assert!(["info", "warn", "high", "crit"].contains(&e.severity.as_str()));
        assert_eq!(e.source, f.source);
    }
    let evs = cx.nerve_events(now);
    let pairs: Vec<(String, String, String)> = evs
        .iter()
        .map(|v| serde_json::from_value::<NerveEvent>(v.clone()).expect("B33 shape"))
        .map(|e| (e.source, e.kind, e.severity))
        .collect();
    let p = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(
        pairs,
        [
            p("components", "bleeding", "high"),
            p("edge", "bleeding", "high"),
            p("listeners", "vulnerability", "high"),
            p("scanners", "vulnerability", "high"),
        ]
    );
    // B41: counts and tokens, never evidence lines (hostnames, addresses).
    let all = serde_json::to_string(&evs).unwrap();
    assert!(!all.contains("example.test"), "{all}");
}
