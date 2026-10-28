// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 417: the 11 live open escalations of 2026-10-28 15:37 +01:00,
//! taken from the real VPS `escalations.jsonl` (read-only copy in
//! `fixtures/escalations-live-20261028.jsonl`, slot 387): every line written
//! by cortex process 186171 (started 2026-10-27 18:17:08 +01:00, replaced at
//! the 2026-10-28 17:37:56 +01:00 cutover). The file has no ack line at all.
//! Replayed through the real `Escalations` queue: open count at 05:47 and
//! 15:37 as measured live, and what each staged max age would have left.

use gaia_cortex::sink::Escalations;
use serde_json::Value;

const LIVE: &str = include_str!("fixtures/escalations-live-20261028.jsonl");

/// Unix seconds of "2026-10-28T06:25:10+01:00" (civil-from-days, Hinnant).
fn t_of(s: &str) -> u64 {
    let n = |a: usize, b: usize| -> i64 { s[a..b].parse().unwrap() };
    let (y, m, d) = (n(0, 4), n(5, 7), n(8, 10));
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

fn process_186171() -> Vec<(u64, Value)> {
    let (from, to) = (
        t_of("2026-10-27T18:17:08+01:00"),
        t_of("2026-10-28T17:37:56+01:00"),
    );
    let all: Vec<Value> = LIVE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(all.len(), 37, "fixture: 37 lines");
    assert!(
        all.iter().all(|v| v.get("ack").is_none()),
        "nobody ever acked"
    );
    all.into_iter()
        .map(|v| (t_of(v["ts"].as_str().unwrap()), v))
        .filter(|(t, _)| (from..to).contains(t))
        .collect()
}

fn open_at(items: &[(u64, Value)], at: &str, max_age_s: u64) -> (usize, usize) {
    let now = t_of(at);
    let mut q = Escalations::new(None);
    for (t, v) in items.iter().filter(|(t, _)| *t <= now) {
        q.push_at(v.clone(), *t);
    }
    let closed = q.expire(now, max_age_s, at);
    (q.open.len(), closed)
}

#[test]
fn live_open_count_is_reproduced_and_classified() {
    let items = process_186171();
    assert_eq!(items.len(), 11);
    // live readings: 1 at 05:47 (slot 3xx live watch), 11 at 15:37 (slot 392)
    assert_eq!(open_at(&items, "2026-10-28T05:47:00+01:00", 0).0, 1);
    assert_eq!(open_at(&items, "2026-10-28T15:37:00+01:00", 0).0, 11);
    let subj = |v: &Value| v["subject"].as_str().unwrap().to_string();
    let test_units = items
        .iter()
        .filter(|(_, v)| subj(v).starts_with("unit:noloop370-"))
        .count();
    let real: Vec<String> = items
        .iter()
        .filter(|(_, v)| !subj(v).starts_with("unit:noloop370-"))
        .map(|(_, v)| format!("{} {}", subj(v), v["rule"].as_str().unwrap()))
        .collect();
    assert_eq!(test_units, 10, "10 = slot-370 throwaway test units");
    assert_eq!(real, vec!["unit:hermes-rs.service R2-circuit-open"]);
    // 387's exclude list only skips the Hermes inbox file: the escalation
    // record (and so escalations_open) is unchanged by it
    let handoff: Vec<&str> = items
        .iter()
        .map(|(_, v)| v["handoff"].as_str().unwrap_or(""))
        .collect();
    assert!(!handoff.contains(&"excluded"), "exclude list was not live");
}

#[test]
fn staged_max_age_options_at_the_live_reading() {
    let items = process_186171();
    let at = "2026-10-28T15:37:00+01:00";
    // 0 = live (never); 24 h leaves all 11 (oldest 19 h 51 min); 6 h leaves 0
    assert_eq!(open_at(&items, at, 0), (11, 0));
    assert_eq!(open_at(&items, at, 86_400), (11, 0));
    assert_eq!(open_at(&items, at, 21_600), (0, 11));
    // with 24 h they would all be gone by 06:30 the next morning
    assert_eq!(open_at(&items, "2026-10-29T06:30:00+01:00", 86_400).0, 0);
}
