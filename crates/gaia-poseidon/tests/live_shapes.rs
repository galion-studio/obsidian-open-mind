// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The real POSEIDON handle against answers captured read-only from the live
//! VPS `openmind-queue` (tests/live/, GETs only, 2026-10-21 22:05 +02:00).
//! `POST /gaia/dispatch` was not called on the VPS (no POSTs there); its
//! answer comes from the queue crate's own `gaia::dispatch` on an empty
//! engine, which is the code the live 0.1.0 binary runs.

use gaia_core::{Directive, Outcome, System, Verb};
use gaia_poseidon::contract;
use gaia_poseidon::queue::{QueueAdapter, Transport};
use openmind_queue::engine::{Config, Engine};
use serde_json::Value;
use std::io;

fn live(name: &str) -> String {
    let p = format!("{}/tests/live/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{p}: {e}"))
        .trim()
        .to_string()
}

struct Replay {
    stats: String,
    calls: Vec<String>,
}

impl Transport for Replay {
    fn call(&mut self, m: &str, p: &str, b: Option<&str>) -> io::Result<(u16, String)> {
        self.calls.push(format!("{m} {p}"));
        Ok(match (m, p) {
            ("GET", "/healthz") => (200, live("healthz.json")),
            ("GET", "/gaia/status") => (200, live("gaia-status.json")),
            ("GET", "/stats") => (200, self.stats.clone()),
            ("GET", "/vram") => (200, live("vram.json")),
            ("POST", "/gaia/dispatch") => {
                let req: Value = serde_json::from_str(b.unwrap()).unwrap();
                let e = Engine::new(Config::default());
                let r = openmind_queue::gaia::dispatch(
                    &e,
                    req["system"].as_str().unwrap(),
                    req["verb"].as_str().unwrap(),
                    req["payload"].as_str().unwrap_or(""),
                );
                (200, serde_json::to_string(&r).unwrap())
            }
            _ => (404, r#"{"error":"not_found"}"#.into()),
        })
    }
}

fn adapter(stats: String) -> QueueAdapter<Replay> {
    QueueAdapter::with_transport(
        Replay {
            stats,
            calls: Vec::new(),
        },
        "127.0.0.1:19120",
    )
}

#[test]
fn live_healthz_passes_the_identity_handshake() {
    let v: Value = serde_json::from_str(&live("healthz.json")).unwrap();
    assert_eq!(contract::check_identity(200, &v).unwrap(), "0.1.0");
    let s: Value = serde_json::from_str(&live("gaia-status.json")).unwrap();
    let names: Vec<&str> = s["systems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["system"].as_str().unwrap())
        .collect();
    let want: Vec<&str> = System::COMPUTE.iter().map(|s| s.name()).collect();
    assert_eq!(
        names, want,
        "live queue serves exactly gaia-core's compute systems"
    );
}

#[test]
fn every_status_on_live_shapes_is_on_contract() {
    let mut a = adapter(live("stats.json"));
    for s in System::COMPUTE {
        for v in [Verb::Status, Verb::Plan] {
            let r = a.handle(&Directive::new(1, s, v));
            assert!(
                matches!(
                    r.outcome,
                    Outcome::Done | Outcome::DryRun | Outcome::Rejected
                ),
                "{s} {v:?}: {}",
                r.summary
            );
            assert!(!r.summary.contains("contract violation"), "{}", r.summary);
        }
    }
    assert_eq!(a.violations(), 0);
    let r = a.handle(&Directive::new(2, System::Quotas, Verb::Status));
    assert_eq!(r.metric_value("max_pending:deploy-check"), Some(100_000));
    assert_eq!(r.metric_value("max_inflight:deploy-check"), Some(1_000));
    let r = a.handle(&Directive::new(3, System::BudgetCaps, Verb::Status));
    assert_eq!(r.metric_value("reserved:deploy-check"), Some(0));
    assert_eq!(
        r.metric_value("blocked:deploy-check"),
        None,
        "live count is 0"
    );
    let r = a.handle(&Directive::new(4, System::VramPool, Verb::Status));
    assert_eq!(r.metric_value("devices"), Some(0));
    assert_eq!(a.load(), 0);
    // Only allow-listed reads and read-only dispatch were sent.
    for c in &a.transport().calls {
        assert!(c.starts_with("GET ") || c == "POST /gaia/dispatch", "{c}");
    }
}

#[test]
fn live_budget_blocked_is_a_count_and_is_reported() {
    // Same live document, with the counter the engine increments at lease
    // time set to 2 (the live value is 0).
    let mut v: Value = serde_json::from_str(&live("stats.json")).unwrap();
    assert!(
        v["agents"]["deploy-check"]["budget_blocked"].is_u64(),
        "live field is a u64 count"
    );
    v["agents"]["deploy-check"]["budget_blocked"] = 2.into();
    let mut a = adapter(v.to_string());
    let r = a.handle(&Directive::new(1, System::BudgetCaps, Verb::Status));
    assert_eq!(
        r.metric_value("blocked:deploy-check"),
        Some(2),
        "{:?}",
        r.metrics
    );
}
