// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 235 (T5.1 finding F1): the flap rule must see the repeats the nerve
//! folds inside its dedupe window, and the restart caps must hold in every
//! mode. Pipeline-level (mock nerve link, no network); the real nerve +
//! real cortex run is `real_flap_dedupe.rs`.

use gaia_cortex::config::Config;
use gaia_cortex::engine::Cortex;
use gaia_cortex::guard::Guard;
use gaia_cortex::nerve::{NerveLink, Outcome};
use gaia_cortex::repeat::RepeatWatch;
use nerve_cortex_contract::{ActionRequest, ActionStatus, Event};
use openmind_nerve::protected_hosts::ProtectedHosts;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct MockNerve {
    reqs: Arc<Mutex<Vec<ActionRequest>>>,
}

impl NerveLink for MockNerve {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome {
        self.reqs.lock().unwrap().push(req.clone());
        // A nerve in dry-run answers "dry-run" before its caps (bb287cc):
        // it never says rate-limited, whatever the count.
        Outcome {
            status: if req.dry_run {
                ActionStatus::Simulated
            } else {
                ActionStatus::Executed
            },
            by: "nerve",
            reason: "mock".into(),
        }
    }
    fn post_event(&mut self, _ev: &Event) -> bool {
        true
    }
    fn pull_events(&mut self, _n: usize) -> Result<Vec<Event>, String> {
        Ok(vec![])
    }
}

static N: AtomicU64 = AtomicU64::new(0);
const T0: u64 = 1_792_600_000;

fn cortex(mut cfg: Config) -> (Cortex, MockNerve) {
    cfg.data_dir = std::env::temp_dir().join(format!(
        "gaia-cortex-flap-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&cfg.data_dir);
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    let nerve = MockNerve::default();
    let cx = Cortex::new(
        cfg.clone(),
        Guard::with_hosts(ProtectedHosts::builtin(), &cfg.protected_units),
        Box::new(nerve.clone()),
    );
    (cx, nerve)
}

/// A nerve stream event as bb287cc writes it.
fn nerve_failed(seq: u64, unit: &str, count: u32) -> Event {
    let l = json!({"v":1,"seq":seq,"ts":"2026-10-21T23:00:00+02:00","t":T0,"source":"journal",
        "kind":"unit.failed","severity":"high","unit":unit,"group":"other",
        "key":format!("unit.failed:{unit}"),"msg":"m","data":{"unit_result":"exit-code"},"count":count});
    nerve_cortex_contract::parse_events(&l.to_string())
        .pop()
        .unwrap()
        .unwrap()
}

fn rules(d: &[Value]) -> Vec<String> {
    d.iter()
        .map(|x| x["rule"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn defaults_mirror_the_nerve_caps() {
    let c = Config::default();
    assert_eq!(c.restarts_per_unit_hour, 2);
    assert_eq!(c.restarts_total_hour, 6);
    assert_eq!(c.repeat_poll_s, 30);
    assert_eq!(
        (c.flap_failures, c.flap_window_s),
        (3, 900),
        "flap rule unchanged"
    );
}

#[test]
fn before_without_the_watch_a_folded_loop_never_flaps() {
    // What the cortex saw at e982210: the stream sends seq 5 once, the
    // nerve folds the next failures into it; nothing else arrives.
    let (mut cx, nerve) = cortex(Config::default());
    let d = cx.process(vec![nerve_failed(5, "tuner.service", 1)], T0);
    assert_eq!(rules(&d), ["R3-known-crash"]);
    assert_eq!(nerve.reqs.lock().unwrap().len(), 1);
    assert!(!cx.rules.circuit_open("tuner.service", T0 + 60));
}

#[test]
fn after_repeats_from_the_ring_open_the_circuit() {
    let (mut cx, nerve) = cortex(Config::default());
    let w = Arc::new(RepeatWatch::new(900));
    cx.repeats = Some(w.clone());
    let d = cx.process(vec![nerve_failed(5, "tuner.service", 1)], T0);
    assert_eq!(rules(&d), ["R3-known-crash"]);
    assert_eq!(w.len(), 1, "failure from the nerve is watched");
    // 30 s later the ring says the nerve folded 5 more into seq 5
    let reps = w.apply(&[nerve_failed(5, "tuner.service", 6)], 4, true, T0 + 30);
    assert_eq!(reps.len(), 1);
    let d = cx.process(reps, T0 + 31);
    assert_eq!(rules(&d), ["R2-circuit-open"], "{d:?}");
    assert_eq!(d[0]["escalate"], true);
    assert_eq!(d[0]["failures"], 5);
    assert_eq!(d[0]["repeat_of"], json!([5]));
    assert!(d[0]["reason"].as_str().unwrap().contains("6 failures"));
    assert!(cx.rules.circuit_open("tuner.service", T0 + 60));
    assert_eq!(
        nerve.reqs.lock().unwrap().len(),
        1,
        "no restart while flapping"
    );
    // more repeats while the circuit is open: no action, the alert is not repeated
    let reps = w.apply(&[nerve_failed(5, "tuner.service", 9)], 4, true, T0 + 60);
    let d = cx.process(reps, T0 + 61);
    assert!(
        d.is_empty() || d.iter().all(|x| x["action"].is_null()),
        "{d:?}"
    );
    assert_eq!(nerve.reqs.lock().unwrap().len(), 1);
    let s = cx.stats_json();
    assert_eq!(s["events_repeat"], 2);
    assert_eq!(s["repeat_watch"]["repeats"], 8);
}

#[test]
fn one_repeat_restarts_again_then_the_cap_holds_in_dry_run() {
    // flap rule out of the way: the per-unit cap alone must stop the loop,
    // in dry-run, although the (mock) dry-run nerve never rate-limits.
    let c = Config {
        flap_failures: 100,
        ..Config::default()
    };
    assert!(c.dry_run());
    let (mut cx, nerve) = cortex(c);
    let w = Arc::new(RepeatWatch::new(900));
    cx.repeats = Some(w.clone());
    let mut seen = rules(&cx.process(vec![nerve_failed(5, "tuner.service", 1)], T0));
    for (i, total) in [2u32, 3, 4].into_iter().enumerate() {
        let t = T0 + 200 * (i as u64 + 1);
        let reps = w.apply(&[nerve_failed(5, "tuner.service", total)], 4, true, t);
        seen.extend(rules(&cx.process(reps, t + 1)));
    }
    assert_eq!(
        seen,
        [
            "R3-known-crash",
            "R3-known-crash",
            "R3-cap",
            "R2-circuit-open"
        ]
    );
    assert_eq!(
        nerve.reqs.lock().unwrap().len(),
        2,
        "2 = the nerve's own per-unit cap"
    );
}

#[test]
fn total_cap_holds_across_units_in_dry_run() {
    let (mut cx, nerve) = cortex(Config::default());
    let units = [
        "openmind-memory.service",
        "hermes-rs.service",
        "link-watch-rs.service",
        "galion-desk.service",
        "tuner.service",
        "galion-one.service",
        "console.service",
        "galion-health.service",
    ];
    let mut seen = vec![];
    for (i, u) in units.iter().enumerate() {
        let d = cx.process(vec![nerve_failed(10 + i as u64, u, 1)], T0 + 120 * i as u64);
        seen.extend(rules(&d));
    }
    assert_eq!(&seen[..6], ["R3-known-crash"; 6]);
    assert_eq!(&seen[6..], ["R3-cap-total", "R3-cap-total"]);
    assert_eq!(nerve.reqs.lock().unwrap().len(), 6);
    // at the scenario clock (stats_json uses the wall clock: no time bomb)
    assert_eq!(cx.rules.restarts_last_hour(T0 + 900), 6);
    // an hour after the first restart one slot is free again
    let d = cx.process(vec![nerve_failed(30, "galion-voice.service", 1)], T0 + 3601);
    assert_eq!(rules(&d), ["R3-known-crash"]);
}

#[test]
fn replayed_event_with_folded_count_counts_every_occurrence() {
    // After a reconnect the stream replays the ring: the event already
    // carries count 4 (1 + 3 folded). That is 4 failures in the window.
    let (mut cx, nerve) = cortex(Config::default());
    let d = cx.process(vec![nerve_failed(7, "galion-desk.service", 4)], T0);
    assert_eq!(rules(&d), ["R2-circuit-open"], "{d:?}");
    assert_eq!(d[0]["failures"], 4);
    assert!(nerve.reqs.lock().unwrap().is_empty());
}

#[test]
fn protected_units_stay_alert_only_with_repeats() {
    let (mut cx, nerve) = cortex(Config::default());
    let w = Arc::new(RepeatWatch::new(900));
    cx.repeats = Some(w.clone());
    cx.process(vec![nerve_failed(5, "cloudflared.service", 1)], T0);
    let reps = w.apply(
        &[nerve_failed(5, "cloudflared.service", 9)],
        4,
        true,
        T0 + 30,
    );
    let d = cx.process(reps, T0 + 200);
    assert!(d.iter().all(|x| x["rule"] == "R1-protected"), "{d:?}");
    assert!(nerve.reqs.lock().unwrap().is_empty());
}
