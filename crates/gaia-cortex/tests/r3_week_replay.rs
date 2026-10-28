// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 309: seven days of REAL VPS systemd lifecycle lines (read-only
//! `journalctl _PID=1 -o json` export 2026-10-27 ~22:05 +01:00, every unit
//! that logged "Failed with result" in the last 7 days, fields MESSAGE_ID /
//! UNIT / EXIT_* / UNIT_RESULT / JOB_* only) through the cortex rules.
//!
//! The journal lines are mapped to nerve events the way the live nerve
//! 9d1a4f3 `evt/journal.rs` does (`Stopping` is NOT mapped live: slot 310
//! F2 adds it), grouped per unit into 1.5 s bursts (the live cortex
//! coalesce), and decided on. Asserts: every planned verdict has a stop /
//! restart job marker or a 143/130 exit that is active again; no burst with
//! a non-stop exit status, a KILL or a core dump is ever planned; the live
//! console 143 restarts are all planned.

use gaia_cortex::guard::Guard;
use gaia_cortex::rules::{Limits, Rules, Subject};
use nerve_cortex_contract::Event;
use openmind_nerve::protected_hosts::ProtectedHosts;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const WEEK: &str = include_str!("fixtures/vps-failing-units-7d-20261027.jsonl");

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|x| !x.is_empty())
}

/// 9d1a4f3 evt/journal.rs parse_entry_with, unit lines only.
fn nerve_event(v: &Value, seq: u64) -> Option<(u64, Event)> {
    let unit = s(v, "UNIT")?;
    let id = s(v, "MESSAGE_ID")?;
    let (kind, sev) = match id {
        "d9b373ed55a64feb8242e02dbe79a49c" => {
            if s(v, "UNIT_RESULT") == Some("start-limit-hit") {
                ("unit.start_limit", "critical")
            } else {
                ("unit.failed", "high")
            }
        }
        "98e322203f7a4ed290d09fe03c09fe15" => ("unit.exit", "high"),
        "5eb03494b6574f1a8b9fe29bc1d4d9b8" => ("unit.restart_scheduled", "high"),
        "39f53479d3a045ac8e11786248231fbf" => ("unit.started", "medium"),
        "9d1aaa27d60140bd96365438aad20286" => ("unit.stopped", "medium"),
        _ => return None, // incl. Stopping de5b426a (not mapped by 9d1a4f3)
    };
    let mut e = Event::new(
        "journal",
        kind,
        sev,
        json!({
            "message_id": id,
            "unit_result": s(v, "UNIT_RESULT"),
            "exit_code": s(v, "EXIT_CODE"),
            "exit_status": s(v, "EXIT_STATUS"),
            "job_type": s(v, "JOB_TYPE"),
            "job_result": s(v, "JOB_RESULT"),
            "n_restarts": s(v, "N_RESTARTS"),
        }),
    );
    e.unit = Some(unit.to_string());
    e.seq = Some(seq);
    e.key = Some(format!("{kind}:{unit}"));
    let t = s(v, "__REALTIME_TIMESTAMP")?.parse::<u64>().ok()? / 1000; // ms
    Some((t, e))
}

#[test]
fn seven_days_of_vps_failures_planned_vs_crash() {
    let mut per_unit: BTreeMap<String, Vec<(u64, Event)>> = BTreeMap::new();
    let mut lines = 0;
    for (i, l) in WEEK.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        lines += 1;
        let v: Value = serde_json::from_str(l).unwrap();
        if let Some((t, e)) = nerve_event(&v, i as u64 + 1) {
            per_unit
                .entry(e.unit.clone().unwrap())
                .or_default()
                .push((t, e));
        }
    }
    assert!(lines > 100, "fixture has {lines} lines");
    let mut r = Rules::new(
        Guard::with_hosts(ProtectedHosts::builtin(), &[]),
        &[],
        Limits {
            restarts_per_unit_hour: 3,
            restarts_total_hour: 6,
            flap_failures: 3,
            flap_window_s: 900,
            circuit_open_s: 3600,
        },
    );
    let mut tally: BTreeMap<(String, &'static str), u32> = BTreeMap::new();
    let (mut planned, mut fail_bursts) = (0, 0);
    for (unit, mut evs) in per_unit {
        evs.sort_by_key(|(t, _)| *t);
        let mut i = 0;
        while i < evs.len() {
            let t0 = evs[i].0;
            let mut burst = vec![];
            while i < evs.len() && evs[i].0 <= t0 + 1500 {
                burst.push(evs[i].1.clone());
                i += 1;
            }
            let sub = Subject {
                key: format!("unit:{unit}"),
                unit: Some(unit.clone()),
                host: None,
                events: burst,
            };
            if sub.failures() == 0 {
                continue;
            }
            fail_bursts += 1;
            let ev = sub.stop_evidence();
            let v = r.decide(&sub, false, t0 / 1000);
            *tally.entry((unit.clone(), v.rule)).or_default() += 1;
            let exits: Vec<String> = sub
                .events
                .iter()
                .filter_map(|e| {
                    e.data["exit_status"]
                        .as_str()
                        .map(|x| format!("{}/{x}", e.data["exit_code"].as_str().unwrap_or("")))
                })
                .collect();
            if v.rule == "R5b-planned-stop" {
                planned += 1;
                assert!(!ev.crash, "{unit} planned with crash evidence {exits:?}");
                assert!(
                    ev.marker.is_some() || (ev.stop_exit && ev.active_again),
                    "{unit}: {exits:?}"
                );
            }
            if exits
                .iter()
                .any(|x| !x.ends_with("/143") && !x.ends_with("/130") && !x.ends_with("/0"))
            {
                assert_ne!(v.rule, "R5b-planned-stop", "{unit} crash {exits:?} planned");
            }
            if unit == "console.service"
                && exits.iter().all(|x| x == "exited/143")
                && !exits.is_empty()
            {
                assert_eq!(v.rule, "R5b-planned-stop", "console 143 restart: {v:?}");
            }
        }
    }
    println!("[309] 7-day VPS replay: {lines} journal lines, {fail_bursts} failure bursts, {planned} planned (R5b)");
    for ((u, rule), n) in &tally {
        println!("[309]   {u:<50} {rule:<22} {n}");
    }
    assert!(planned >= 4, "the 4 console 143 restarts are planned");
}
