// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 431 (D-417-3): open escalations survive a cortex restart
//! (rebuilt from `escalations.jsonl` minus ack/close lines) and escalation
//! ids are unique across restarts (`esc-<epoch>-<seq>`), through the real
//! engine. Autoclose stays OFF by default. Also replays the real live file
//! of 2026-10-28 (legacy `esc-<N>` ids) through a fresh start.

use gaia_cortex::config::Config;
use gaia_cortex::engine::Cortex;
use gaia_cortex::sink::{parse_esc_id, EscId};
use nerve_cortex_contract::Event;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const LIVE: &str = include_str!("fixtures/escalations-live-20261028.jsonl");

static N: AtomicU64 = AtomicU64::new(0);
static TS: AtomicU64 = AtomicU64::new(0);

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "gaia-cortex-431-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cortex(d: &Path, autoclose: bool, max_age_s: u64) -> Cortex {
    gaia_cortex::build(Config {
        data_dir: d.to_path_buf(),
        incident_quiet_s: 600,
        escalation_autoclose: autoclose,
        escalation_max_age_s: max_age_s,
        ..Config::default()
    })
}

fn ev(kind: &str, sev: &str, unit: &str) -> Event {
    let mut e = Event::new("journal", kind, sev, json!({"unit": unit}));
    e.ts = format!("t{}", TS.fetch_add(1, Ordering::SeqCst));
    e
}

fn open_ids(cx: &Cortex) -> Vec<String> {
    cx.escalations_json()["open"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect()
}

fn epoch_of(id: &str) -> u64 {
    match parse_esc_id(id) {
        Some(EscId::Unique { start, .. }) => start,
        other => panic!("new-shape id expected, got {id} -> {other:?}"),
    }
}

fn two_failures(cx: &mut Cortex, a: &str, b: &str) {
    cx.process(vec![ev("unit.failed", "high", a)], 1_000);
    cx.process(vec![ev("unit.failed", "high", b)], 1_010);
}

#[test]
fn restart_keeps_open_escalations_and_ids_stay_unique() {
    let d = dir();
    let mut cx = cortex(&d, false, 0);
    two_failures(&mut cx, "p431-a.service", "p431-b.service");
    let first = open_ids(&cx);
    assert!(first.len() >= 2, "{:?}", cx.escalations_json());
    let e1 = epoch_of(&first[0]);
    assert!(first.iter().all(|id| epoch_of(id) == e1));
    drop(cx);

    // Restart (same second is likely here): the queue comes back, and the
    // new process picks an epoch after every epoch in the log.
    let mut cx = cortex(&d, false, 0);
    assert_eq!(open_ids(&cx), first, "open survives restart");
    assert_eq!(cx.stats_json()["escalations_reloaded"], first.len());
    assert!(cx.stats_json()["escalation_epoch"].as_u64().unwrap() > e1);
    two_failures(&mut cx, "p431-c.service", "p431-d.service");
    let second = open_ids(&cx);
    assert!(second.len() >= first.len() + 2);
    let mut all = second.clone();
    all.sort();
    all.dedup();
    assert_eq!(
        all.len(),
        second.len(),
        "no duplicate ids across restarts: {second:?}"
    );
    let e2 = epoch_of(second.last().unwrap());
    assert!(e2 > e1);

    // An ack in process 2 for an item from process 1 sticks after restart 3.
    assert!(cx.escalations.ack(&first[0], "2026-10-28T23:30:00+01:00"));
    let left = open_ids(&cx);
    drop(cx);
    let cx = cortex(&d, false, 0);
    assert_eq!(open_ids(&cx), left);
    assert!(!open_ids(&cx).contains(&first[0]));
    assert!(cx.stats_json()["escalation_epoch"].as_u64().unwrap() > e2);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn autoclose_stays_off_by_default_after_restart() {
    let d = dir();
    let mut cx = cortex(&d, false, 0);
    two_failures(&mut cx, "p431-e.service", "p431-f.service");
    let n = open_ids(&cx).len();
    drop(cx);
    let mut cx = cortex(&d, false, 0);
    let c = Config::default();
    assert!(!c.escalation_autoclose && c.escalation_max_age_s == 0);
    cx.process(vec![ev("unit.started", "info", "p431-e.service")], 2_000);
    cx.process(vec![ev("unit.started", "info", "p431-f.service")], 2_010);
    assert_eq!(open_ids(&cx).len(), n, "default: nothing auto-closes");
    assert_eq!(cx.escalations_json()["closed_total"], 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn closed_escalations_stay_closed_after_restart() {
    let d = dir();
    let mut cx = cortex(&d, true, 0);
    two_failures(&mut cx, "p431-g.service", "p431-h.service");
    assert!(!open_ids(&cx).is_empty());
    cx.process(vec![ev("unit.started", "info", "p431-g.service")], 2_000);
    cx.process(vec![ev("unit.started", "info", "p431-h.service")], 2_010);
    assert!(open_ids(&cx).is_empty(), "{:?}", cx.escalations_json());
    let closed = cx.escalations_json()["closed_total"].as_u64().unwrap();
    assert!(closed >= 2);
    drop(cx);
    let cx = cortex(&d, false, 0);
    assert!(open_ids(&cx).is_empty(), "closed stay closed");
    let recent = cx.escalations_json()["closed_recent"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(recent.len() as u64, closed);
    assert!(recent.iter().all(|v| v["closed_why"] == "incident-cleared"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn torn_last_line_after_a_crash_is_tolerated() {
    let d = dir();
    let mut cx = cortex(&d, false, 0);
    two_failures(&mut cx, "p431-i.service", "p431-j.service");
    let ids = open_ids(&cx);
    drop(cx);
    let p = d.join("escalations.jsonl");
    let mut body = std::fs::read(&p).unwrap();
    body.extend_from_slice(br#"{"id":"esc-9-1","incident":"inc-x","sub"#);
    std::fs::write(&p, body).unwrap();
    let mut cx = cortex(&d, false, 0);
    assert_eq!(open_ids(&cx), ids);
    assert_eq!(cx.stats_json()["escalations_reload_skipped"], 1);
    cx.process(vec![ev("unit.failed", "high", "p431-k.service")], 3_000);
    let after = open_ids(&cx);
    drop(cx);
    let cx = cortex(&d, false, 0);
    assert_eq!(
        open_ids(&cx),
        after,
        "record after the torn line is readable"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The real live file (37 lines, legacy ids, 0 ack lines) through a start
/// of this build: every record is open again (the old process dropped them
/// at each restart, unlogged). With the staged 6 h max age (drop-in 93,
/// D-417-2) the first burst closes them all as `expired`, logged.
#[test]
fn live_file_reload_legacy_ids_and_first_deploy_effect() {
    let lines: Vec<&str> = LIVE.lines().filter(|l| !l.trim().is_empty()).collect();
    let records = lines
        .iter()
        .filter(|l| serde_json::from_str::<Value>(l).unwrap()["id"].is_string())
        .count();
    assert_eq!(lines.len(), 37);
    assert_eq!(records, 37, "no ack/close lines in the live file");
    for l in &lines {
        let v: Value = serde_json::from_str(l).unwrap();
        let id = v["id"].as_str().unwrap();
        assert!(matches!(parse_esc_id(id), Some(EscId::Legacy(_))), "{id}");
    }

    let d = dir();
    std::fs::write(d.join("escalations.jsonl"), LIVE).unwrap();
    let cx = cortex(&d, false, 0);
    assert_eq!(
        open_ids(&cx).len(),
        37,
        "default: all 37 legacy items reopen"
    );
    drop(cx);

    let mut cx = cortex(&d, true, 6 * 3600);
    assert_eq!(open_ids(&cx).len(), 37);
    let now = gaia_cortex::engine::now_secs();
    cx.process(vec![], now);
    assert!(
        open_ids(&cx).is_empty(),
        "6 h max age closes the legacy backlog"
    );
    assert_eq!(cx.stats_json()["escalations_closed_expired"], 37);
    drop(cx);
    let text = std::fs::read_to_string(d.join("escalations.jsonl")).unwrap();
    assert_eq!(text.lines().filter(|l| l.contains("\"close\"")).count(), 37);
    let cx = cortex(&d, false, 0);
    assert!(open_ids(&cx).is_empty(), "expired legacy items stay closed");
    let _ = std::fs::remove_dir_all(&d);
}
