// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 387: replay every live cortex escalation (VPS
//! /var/lib/gaia-cortex/escalations.jsonl, read-only copy 2026-10-28 13:28
//! +01:00, 37 lines from 2026-10-21 19:08 +02:00 to 2026-10-28 06:29 +01:00)
//! through the real `Handoff` with and without the staged exclude list.
//! No network, no model.

use gaia_cortex::handoff::{Handoff, Offer};
use gaia_cortex::handoff_exclude;
use serde_json::{json, Value};
use std::path::PathBuf;

const LIVE: &str = include_str!("fixtures/escalations-live-20261028.jsonl");
/// The staged list (deploy/staged/cortex/88-hermes-exclude.conf).
const STAGED: &[&str] = &["noloop*", "sbxtest*", "swarmtest-*"];

fn live() -> Vec<Value> {
    LIVE.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// The ticket fields `Handoff::offer` reads, built as the engine does
/// (`unit` = the subject's unit, from the lead event).
fn ticket(e: &Value) -> Value {
    json!({"incident": e["incident"], "subject": e["subject"], "rule": e["rule"],
           "severity": e["severity"], "unit": e["lead_event"]["unit"]})
}

/// Unix seconds of the escalation `ts` ("2026-10-28T06:25:10+01:00").
fn t_of(e: &Value) -> u64 {
    let s = e["ts"].as_str().expect("ts");
    let n = |a: usize, b: usize| -> i64 { s[a..b].parse().unwrap() };
    let (y, m, d) = (n(0, 4), n(5, 7), n(8, 10));
    // days from civil (Howard Hinnant), proleptic Gregorian
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let off = (n(20, 22) * 3600 + n(23, 25) * 60) * if &s[19..20] == "-" { -1 } else { 1 };
    (days * 86_400 + n(11, 13) * 3600 + n(14, 16) * 60 + n(17, 19) - off) as u64
}

#[test]
fn ts_parse_matches_the_nerve_clock() {
    // esc-103 lead event t=1793165108 at 06:25:08+01:00
    assert_eq!(
        t_of(&json!({"ts": "2026-10-28T06:25:08+01:00"})),
        1_793_165_108
    );
    assert_eq!(
        t_of(&json!({"ts": "2026-10-28T07:25:08+02:00"})),
        1_793_165_108
    );
}

#[derive(Default, Debug, PartialEq)]
struct Tally {
    written: Vec<String>,
    excluded: Vec<String>,
    duplicate: usize,
    rate_limited: usize,
}

fn replay(tag: &str, exclude: Vec<String>) -> Tally {
    let d: PathBuf = std::env::temp_dir().join(format!("hx387-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let mut h =
        Handoff::new(true, d.join("inbox"), Some(d.join("st.json")), 6, 3600).with_exclude(exclude);
    let mut t = Tally::default();
    for e in live() {
        let s = e["subject"].as_str().unwrap().to_string();
        match h.offer(&ticket(&e), t_of(&e)) {
            Offer::Written(_) => t.written.push(s),
            Offer::Excluded(_) => t.excluded.push(s),
            Offer::Duplicate => t.duplicate += 1,
            Offer::RateLimited => t.rate_limited += 1,
            o => panic!("unexpected {o:?}"),
        }
    }
    let files = std::fs::read_dir(d.join("inbox"))
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().ends_with(".json"))
        .count();
    assert_eq!(files, t.written.len());
    let _ = std::fs::remove_dir_all(&d);
    t
}

#[test]
fn live_corpus_shape() {
    let v = live();
    assert_eq!(v.len(), 37);
    let live_handed = v
        .iter()
        .filter(|e| e["handoff"] == "handed-to-hermes")
        .count();
    assert_eq!(live_handed, 30);
}

#[test]
fn without_exclude_the_replay_matches_the_live_outcome() {
    let t = replay("off", vec![]);
    // live: 30 handed-to-hermes, 4 duplicate, 2 rate-limited (+1 pre-handoff line)
    assert_eq!(t.excluded.len(), 0);
    let test_units = t
        .written
        .iter()
        .filter(|s| s.contains("noloop370") || s.contains("sbxtest"))
        .count();
    eprintln!(
        "off: written {} (test units {test_units}), duplicate {}, rate-limited {}",
        t.written.len(),
        t.duplicate,
        t.rate_limited
    );
    assert_eq!(test_units, 9, "9 test-unit handoffs went to Hermes live");
    // exactly the live counts; +1 written = the 2026-10-21 19:08 line from
    // before the handoff existed (no `handoff` field)
    assert_eq!((t.written.len(), t.duplicate, t.rate_limited), (31, 4, 2));
}

#[test]
fn staged_list_skips_every_test_unit_and_nothing_else() {
    let mut pats: Vec<String> = STAGED.iter().map(|s| s.to_string()).collect();
    handoff_exclude::validate(&mut pats).expect("staged list passes the guard");
    let off = replay("off2", vec![]);
    let on = replay("on", pats);
    eprintln!(
        "on: written {}, excluded {}, duplicate {}, rate-limited {}",
        on.written.len(),
        on.excluded.len(),
        on.duplicate,
        on.rate_limited
    );
    // every noloop370/sbxtest escalation is excluded (13 lines), no other one
    assert_eq!(on.excluded.len(), 13, "{:?}", on.excluded);
    assert!(on
        .excluded
        .iter()
        .all(|s| s.contains("noloop370") || s.contains("sbxtest")));
    // no test unit reaches the inbox, every real handoff still does
    assert!(on
        .written
        .iter()
        .all(|s| !s.contains("noloop370") && !s.contains("sbxtest")));
    let real_off: Vec<&String> = off
        .written
        .iter()
        .filter(|s| !s.contains("noloop370") && !s.contains("sbxtest"))
        .collect();
    let real_on: Vec<&String> = on.written.iter().collect();
    assert_eq!(real_on, real_off, "real handoffs unchanged");
    for must in [
        "unit:hermes-rs.service",
        "unit:gaia-hermes-handoff.path.service",
        "nerve-incident:console",
        "nerve-incident:ddos:edge-cloudflared",
        "threshold:verify_probe_synthetic",
    ] {
        assert!(on.written.iter().any(|s| s == must), "{must} still handed");
    }
    assert_eq!(
        on.rate_limited, 0,
        "test units no longer eat the 6/h budget"
    );
}
