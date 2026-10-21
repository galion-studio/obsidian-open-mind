// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 237: incident lifetime (T5.1 F3) and nerve incident backfill,
//! through the real engine (no nerve link: dry-run actions stay local).

use gaia_cortex::config::Config;
use gaia_cortex::engine::Cortex;
use nerve_cortex_contract::Event;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);

fn cortex() -> Cortex {
    let d: PathBuf = std::env::temp_dir().join(format!(
        "gaia-cortex-237-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    gaia_cortex::build(Config {
        data_dir: d,
        incident_quiet_s: 600,
        ..Config::default()
    })
}

static TS: AtomicU64 = AtomicU64::new(0);

/// Distinct `ts` per event: the engine drops exact repeats (fingerprint).
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

fn open_of(cx: &Cortex, cause: &str) -> Option<Value> {
    cx.incidents_json()["open"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["cause"] == cause)
        .cloned()
}

fn resolved_of(cx: &Cortex, id: &Value) -> Option<Value> {
    cx.incidents_json()["resolved"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == *id)
        .cloned()
}

#[test]
fn unrelated_bursts_do_not_resolve_an_open_unit_incident() {
    let mut cx = cortex();
    cx.process(vec![failed("tuner.service")], 1_000);
    let inc = open_of(&cx, "unit:tuner.service").expect("tuner incident");
    let id = inc["id"].clone();
    // six bursts of other subjects, one per burst (the T5.1 S3 shape)
    for (i, u) in [
        "ssh.service",
        "cloudflared.service",
        "postfix.service",
        "galion-backup.service",
        "hades-gate.service",
        "galion-one.service",
    ]
    .iter()
    .enumerate()
    {
        cx.process(vec![failed(u)], 1_010 + i as u64 * 10);
    }
    let still = open_of(&cx, "unit:tuner.service").expect("still open after 6 unrelated bursts");
    assert_eq!(still["id"], id);
    assert_eq!(
        still["last_seen"], inc["last_seen"],
        "carried, not re-seen: {still}"
    );
    assert_eq!(still["cycles"], 1, "{still}");
    // and nothing folded into stack-wide although 7 units are failing
    assert!(open_of(&cx, "stack-wide").is_none());
    // its own recovery resolves it in that burst
    cx.process(vec![started("tuner.service")], 2_000);
    assert!(open_of(&cx, "unit:tuner.service").is_none());
    let r = resolved_of(&cx, &id).expect("resolved");
    assert_eq!(r["resolved_by"], "cleared", "{r}");
    assert_eq!(cx.stats_json()["incidents_resolved_cleared"], 1);
    // the other six are still open
    assert_eq!(cx.incidents_json()["open"].as_array().unwrap().len(), 6);
}

#[test]
fn info_burst_that_is_not_a_recovery_keeps_the_incident() {
    let mut cx = cortex();
    cx.process(vec![failed("tuner.service")], 1_000);
    let id = open_of(&cx, "unit:tuner.service").unwrap()["id"].clone();
    for t in [1_010, 1_020, 1_030] {
        cx.process(
            vec![ev(
                "unit.state",
                "info",
                json!({"unit": "tuner.service", "to": "activating"}),
            )],
            t,
        );
    }
    assert_eq!(open_of(&cx, "unit:tuner.service").unwrap()["id"], id);
    // a clean stop clears it too
    cx.process(
        vec![ev(
            "unit.state",
            "info",
            json!({"unit": "tuner.service", "to": "inactive"}),
        )],
        1_040,
    );
    assert!(open_of(&cx, "unit:tuner.service").is_none());
}

#[test]
fn one_shot_subjects_expire_by_time_not_by_batches() {
    let mut cx = cortex(); // incident_quiet_s = 600
    let exit = ev(
        "proc.exit",
        "medium",
        json!({"comm": "worker", "exit_code": 3}),
    );
    cx.process(vec![exit], 1_000);
    let id = open_of(&cx, "proc:worker").expect("proc incident")["id"].clone();
    for t in [1_010, 1_020, 1_030, 1_400] {
        cx.process(vec![failed(&format!("u{t}.service"))], t);
    }
    assert!(
        open_of(&cx, "proc:worker").is_some(),
        "4 unrelated bursts in 400 s"
    );
    cx.process(vec![failed("late.service")], 1_600);
    assert!(open_of(&cx, "proc:worker").is_none());
    let r = resolved_of(&cx, &id).unwrap();
    assert_eq!(r["resolved_by"], "expired", "{r}");
    assert_eq!(cx.stats_json()["incidents_resolved_expired"], 1);
    // units never expire
    assert!(open_of(&cx, "unit:u1010.service").is_some());
}

#[test]
fn stack_wide_decision_unchanged_and_incident_lives_until_all_recover() {
    let mut cx = cortex();
    let units = [
        "hermes-rs.service",
        "console.service",
        "link-watch-rs.service",
        "openmind-memory.service",
    ];
    let d = cx.process(units.iter().map(|u| failed(u)).collect(), 1_000);
    let sw: Vec<&Value> = d.iter().filter(|d| d["rule"] == "R0-stack-wide").collect();
    assert_eq!(sw.len(), 1, "{d:?}");
    let id = open_of(&cx, "stack-wide").unwrap()["id"].clone();
    // a later single failure elsewhere decides on its own (not R0)
    let d = cx.process(vec![failed("tuner.service")], 1_010);
    assert_eq!(d[0]["rule"], "R3-known-crash", "{d:?}");
    // three recover: stack-wide stays (one member still failing)
    for (i, u) in units[..3].iter().enumerate() {
        cx.process(vec![started(u)], 1_020 + i as u64);
        assert_eq!(open_of(&cx, "stack-wide").unwrap()["id"], id);
    }
    cx.process(vec![started(units[3])], 1_030);
    assert!(open_of(&cx, "stack-wide").is_none());
    assert_eq!(resolved_of(&cx, &id).unwrap()["resolved_by"], "cleared");
    assert!(open_of(&cx, "unit:tuner.service").is_some());
}

fn snapshot(items: &[(&str, &str, &str)]) -> Value {
    json!({"open": items.iter().map(|(id, cause, sev)| json!({
        "id": id, "cause": cause, "severity": sev, "components": [cause],
        "title": format!("{cause}: test"), "opened": "2026-10-21T22:56:56+02:00"
    })).collect::<Vec<_>>(), "resolved": []})
}

fn nerve_ev(kind: &str, sev: &str, seq: u64, data: Value) -> Event {
    let mut e = Event::new("engine", kind, sev, data);
    e.seq = Some(seq);
    e
}

#[test]
fn backfill_opens_missed_nerve_incidents_once() {
    let mut cx = cortex();
    let snap = snapshot(&[
        ("inc-d7320b38", "galion-one", "error"),
        ("inc-a763c076", "edge", "error"),
        ("inc-c0ffee00", "tiny", "critical"),
    ]);
    let evs = cx.backfill(&snap, 1_000);
    assert_eq!(evs.len(), 3);
    assert!(evs.iter().all(|e| e.data["backfill"] == true));
    let d = cx.process(evs, 1_000);
    assert_eq!(d.len(), 3, "{d:?}");
    for x in &d {
        assert_eq!(x["rule"], "R11-nerve-incident", "{x}");
        assert!(x["action"].is_null(), "never an action: {x}");
        assert!(x["reason"].as_str().unwrap().contains("backfill"), "{x}");
    }
    let crit = d
        .iter()
        .find(|x| x["subject"] == "nerve-incident:tiny")
        .unwrap();
    assert_eq!(crit["escalate"], true);
    assert!(open_of(&cx, "nerve-incident:galion-one").is_some());
    let st = cx.stats_json();
    assert_eq!(st["nerve_incidents_backfilled"], 3);
    assert_eq!(st["nerve_incidents_known"], 3);
    // second reconnect, same snapshot: nothing new
    assert!(cx.backfill(&snap, 1_100).is_empty());
    // the stream replays the nerve's own opened event for one of them: dropped
    let d = cx.process(
        vec![nerve_ev(
            "incident.opened",
            "high",
            7,
            json!({"id": "inc-a763c076", "cause": "edge", "components": ["cloudflared"]}),
        )],
        1_200,
    );
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(cx.stats_json()["nerve_incidents_duplicate"], 1);
}

#[test]
fn nerve_resolved_with_only_an_id_resolves_its_incident() {
    let mut cx = cortex();
    cx.process(
        vec![nerve_ev(
            "incident.opened",
            "high",
            1,
            json!({"id": "inc-1", "cause": "galion-one", "components": ["galion-one-mouth"]}),
        )],
        1_000,
    );
    let id = open_of(&cx, "nerve-incident:galion-one").unwrap()["id"].clone();
    // unrelated bursts: stays open (no batch-count resolution)
    for t in [1_010, 1_020, 1_030] {
        cx.process(vec![failed(&format!("x{t}.service"))], t);
    }
    assert!(open_of(&cx, "nerve-incident:galion-one").is_some());
    // the nerve's incident.resolved carries only {"id"}
    let d = cx.process(
        vec![nerve_ev(
            "incident.resolved",
            "info",
            9,
            json!({"id": "inc-1"}),
        )],
        1_100,
    );
    assert_eq!(d[0]["subject"], "nerve-incident:galion-one", "{d:?}");
    assert!(open_of(&cx, "nerve-incident:galion-one").is_none());
    assert_eq!(resolved_of(&cx, &id).unwrap()["resolved_by"], "cleared");
    assert_eq!(cx.stats_json()["nerve_incidents_known"], 0);
}

#[test]
fn reconnect_reconciles_dropped_and_follows_restarted_ids() {
    let mut cx = cortex();
    let evs = cx.backfill(
        &snapshot(&[("inc-a", "edge", "error"), ("inc-b", "galion-one", "error")]),
        1_000,
    );
    cx.process(evs, 1_000);
    let edge_id = open_of(&cx, "nerve-incident:edge").unwrap()["id"].clone();
    // the nerve restarted: galion-one is open again under a new id, edge is gone
    let evs = cx.backfill(&snapshot(&[("inc-b2", "galion-one", "error")]), 2_000);
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0].kind, "incident.resolved");
    assert_eq!(evs[0].data["cause"], "edge");
    let d = cx.process(evs, 2_000);
    assert!(
        d[0]["reason"].as_str().unwrap().contains("reconciled"),
        "{d:?}"
    );
    assert!(open_of(&cx, "nerve-incident:edge").is_none());
    assert_eq!(
        resolved_of(&cx, &edge_id).unwrap()["resolved_by"],
        "cleared"
    );
    let st = cx.stats_json();
    assert_eq!(st["nerve_incidents_remapped"], 1);
    assert_eq!(st["nerve_incidents_reconciled"], 1);
    assert_eq!(
        cx.incidents_json()["nerve_incidents"]["inc-b2"],
        "galion-one"
    );
    // the new nerve's own replayed opened for inc-b2: dropped, no second alert
    let d = cx.process(
        vec![nerve_ev(
            "incident.opened",
            "high",
            1,
            json!({"id": "inc-b2", "cause": "galion-one"}),
        )],
        2_100,
    );
    assert!(d.is_empty());
    // and its resolve (id only) resolves the still-open galion-one incident
    cx.process(
        vec![nerve_ev(
            "incident.resolved",
            "info",
            2,
            json!({"id": "inc-b2"}),
        )],
        2_200,
    );
    assert!(open_of(&cx, "nerve-incident:galion-one").is_none());
}

#[test]
fn hostile_snapshot_is_bounded() {
    let mut cx = cortex();
    assert!(cx.backfill(&json!({"open": "nope"}), 1).is_empty());
    assert!(cx.backfill(&json!([1, 2]), 1).is_empty());
    let many: Vec<Value> = (0..2000)
        .map(|i| json!({"id": format!("inc-{i}"), "cause": format!("c{i}\u{1}"), "severity": "warn"}))
        .collect();
    let evs = cx.backfill(&json!({"open": many}), 1);
    assert_eq!(evs.len(), 512);
    assert!(!evs[0].data["cause"].as_str().unwrap().contains('\u{1}'));
    let junk = json!({"open": [{"id": 5, "cause": "x"}, {"cause": "y"}, {"id": "", "cause": "z"}]});
    assert!(cx.backfill(&junk, 1).is_empty());
}
