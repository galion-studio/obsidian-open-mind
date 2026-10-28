// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 417: open escalations close when their incident resolves
//! (`escalation_autoclose`) or when they reach `escalation_max_age_s`,
//! through the real engine (no nerve link: dry-run actions stay local).
//! Default config = the live behaviour before 417: only an ack closes one.

use gaia_cortex::config::Config;
use gaia_cortex::engine::Cortex;
use nerve_cortex_contract::Event;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);
static TS: AtomicU64 = AtomicU64::new(0);

fn cortex(autoclose: bool, max_age_s: u64) -> (Cortex, PathBuf) {
    let d: PathBuf = std::env::temp_dir().join(format!(
        "gaia-cortex-417-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let cx = gaia_cortex::build(Config {
        data_dir: d.clone(),
        incident_quiet_s: 600,
        escalation_autoclose: autoclose,
        escalation_max_age_s: max_age_s,
        ..Config::default()
    });
    (cx, d)
}

fn ev(kind: &str, sev: &str, data: Value) -> Event {
    let mut e = Event::new("journal", kind, sev, data);
    e.ts = format!("t{}", TS.fetch_add(1, Ordering::SeqCst));
    e
}

fn failed(unit: &str) -> Event {
    ev("unit.failed", "high", json!({"unit": unit}))
}

fn started(unit: &str) -> Event {
    ev("unit.started", "info", json!({"unit": unit}))
}

fn open(cx: &Cortex) -> Vec<Value> {
    cx.escalations_json()["open"].as_array().unwrap().clone()
}

fn log_lines(d: &std::path::Path, key: &str) -> usize {
    std::fs::read_to_string(d.join("escalations.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains(&format!("\"{key}\"")))
        .count()
}

/// Two unknown (non-allowlisted) units fail: R4 escalations, one incident each.
fn two_failures(cx: &mut Cortex) {
    cx.process(vec![failed("p417-a.service")], 1_000);
    cx.process(vec![failed("p417-b.service")], 1_010);
}

#[test]
fn default_config_never_closes_an_escalation() {
    let (mut cx, d) = cortex(false, 0);
    two_failures(&mut cx);
    let n = open(&cx).len();
    assert!(n >= 2, "two escalations: {:?}", cx.escalations_json());
    cx.process(vec![started("p417-a.service")], 2_000);
    cx.process(vec![started("p417-b.service")], 2_010);
    assert!(cx.incidents_json()["open"].as_array().unwrap().is_empty());
    // incidents resolved, escalations did not: the live 1 -> 11 growth
    assert_eq!(open(&cx).len(), n);
    cx.process(vec![failed("p417-c.service")], 10_000_000);
    assert!(open(&cx).len() > n, "only grows");
    assert_eq!(log_lines(&d, "close"), 0);
    assert_eq!(cx.stats_json()["escalations_closed_incident"], 0);
    assert_eq!(cx.stats_json()["escalations_closed_expired"], 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn autoclose_closes_when_the_incident_clears() {
    let (mut cx, d) = cortex(true, 0);
    two_failures(&mut cx);
    let before = open(&cx);
    let a: Vec<&Value> = before
        .iter()
        .filter(|e| e["subject"] == "unit:p417-a.service")
        .collect();
    assert!(!a.is_empty());
    cx.process(vec![started("p417-a.service")], 2_000);
    let after = open(&cx);
    assert!(
        after.iter().all(|e| e["subject"] != "unit:p417-a.service"),
        "a closed: {after:?}"
    );
    assert!(after.iter().any(|e| e["subject"] == "unit:p417-b.service"));
    let esc = cx.escalations_json();
    assert_eq!(esc["closed_total"].as_u64(), Some(a.len() as u64));
    assert_eq!(esc["closed_recent"][0]["closed_why"], "incident-cleared");
    assert_eq!(esc["closed_recent"][0]["incident"], a[0]["incident"]);
    assert_eq!(
        cx.stats_json()["escalations_closed_incident"].as_u64(),
        Some(a.len() as u64)
    );
    assert_eq!(cx.health_json()["escalations_open"], after.len());
    assert_eq!(log_lines(&d, "close"), a.len());
    // total (everything ever escalated) is unchanged
    assert_eq!(esc["total"].as_u64(), Some(before.len() as u64));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn max_age_closes_stuck_items_on_the_next_burst() {
    let (mut cx, d) = cortex(false, 3_600);
    two_failures(&mut cx);
    let n = open(&cx).len();
    // a burst before the age: nothing closes
    cx.process(vec![ev("proc.exit", "info", json!({"comm": "x"}))], 4_000);
    assert_eq!(open(&cx).len(), n);
    // the first burst after 1 h: both close as expired, incidents stay open
    cx.process(vec![ev("proc.exit", "info", json!({"comm": "y"}))], 4_700);
    assert!(open(&cx).is_empty(), "{:?}", open(&cx));
    assert_eq!(
        cx.stats_json()["escalations_closed_expired"].as_u64(),
        Some(n as u64)
    );
    assert_eq!(
        cx.escalations_json()["closed_recent"][0]["closed_why"],
        "expired"
    );
    assert!(
        !cx.incidents_json()["open"].as_array().unwrap().is_empty(),
        "age expiry does not touch incidents"
    );
    assert_eq!(log_lines(&d, "close"), n);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ack_still_closes_and_is_not_counted_as_auto() {
    let (mut cx, d) = cortex(true, 3_600);
    two_failures(&mut cx);
    let id = open(&cx)[0]["id"].as_str().unwrap().to_string();
    assert!(cx.escalations.ack(&id, "T"));
    assert!(open(&cx).iter().all(|e| e["id"] != id.as_str()));
    assert_eq!(cx.escalations_json()["closed_total"], 0);
    assert_eq!(log_lines(&d, "ack"), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn idle_sweep_period_and_expiry_without_a_burst() {
    use gaia_cortex::engine::escalation_sweep_every;
    use std::time::Duration;
    assert_eq!(
        escalation_sweep_every(0),
        None,
        "no max age: no idle wake-up"
    );
    assert_eq!(escalation_sweep_every(8), Some(Duration::from_secs(5)));
    assert_eq!(escalation_sweep_every(90), Some(Duration::from_secs(22)));
    assert_eq!(
        escalation_sweep_every(86_400),
        Some(Duration::from_secs(900))
    );
    let (mut cx, d) = cortex(false, 100);
    two_failures(&mut cx);
    let n = open(&cx).len();
    assert_eq!(cx.expire_escalations(1_050), 0);
    assert_eq!(cx.expire_escalations(1_200), n, "the decider's idle sweep");
    assert!(open(&cx).is_empty());
    let (mut cx0, d0) = cortex(true, 0);
    two_failures(&mut cx0);
    assert_eq!(cx0.expire_escalations(u64::MAX / 2), 0, "0 = never");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&d0);
}
