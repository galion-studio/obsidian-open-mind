// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 175 (Mate's accepted decisions for slot 160, 2026-10-21):
//! D-L2 the nerve's restarts count toward the 3/h cap
//! ([`nerve::restarts_for`], [`heal::cap_check_combined`]) and D-L4 hung
//! detection ([`heal::hung_detail`], [`heal::listener_pids`]). The VPS
//! fixture is a trimmed read-only `GET /state` of the live nerve
//! (127.0.0.1:8670, cycle 6941, 2026-10-21 08:25:05 CEST +02:00).

use gaia_live::heal::{self, cap_check_combined, hung_detail, WINDOW_S};
use gaia_live::nerve::restarts_for;
use serde_json::{json, Value};

fn vps() -> Value {
    serde_json::from_str(include_str!("fixtures/vps-nerve-state-20261021.json")).unwrap()
}

#[test]
fn live_vps_state_parses_and_reports_zero_restarts_for_every_unit() {
    let s = vps();
    assert_eq!(s["cycle"], 6941);
    for svc in [
        "openmind-memory",
        "hermes-rs",
        "galion-one",
        "tuner",
        "console",
        "cloudflared",
        "galion-health",
        "link-watch-rs",
        "galion-desk",
    ] {
        let r = restarts_for(&s, svc).unwrap();
        assert_eq!(r.count, 0, "{svc}");
        assert_eq!(r.component.as_deref(), Some(svc), "{svc}");
        assert_eq!(r.unit.as_deref(), Some(format!("{svc}.service").as_str()));
        assert_eq!(r.restarts_1h, Some(0));
        assert_eq!(r.live_1h, Some(0), "{svc} has a reflex budget row");
        assert_eq!(r.cycle, Some(6941));
    }
    // By unit name too; voice-worker's unit is galion-voice.service.
    let r = restarts_for(&s, "galion-voice").unwrap();
    assert_eq!(r.component.as_deref(), Some("voice-worker"));
    // A box test service is not watched by the nerve: count 0, no match.
    let r = restarts_for(&s, "dummy-flap").unwrap();
    assert_eq!((r.count, r.component, r.unit), (0, None, None));
}

#[test]
fn nerve_restarts_take_the_larger_of_component_and_reflex_budget() {
    let mut s = vps();
    // Component says 1, reflex budget says 2: the same restarts, so 2 (not 3).
    s["components"][1]["restarts_1h"] = json!(1);
    s["reflex"]["budgets"]["hermes-rs.service"]["live_1h"] = json!(2);
    let r = restarts_for(&s, "hermes-rs").unwrap();
    assert_eq!((r.count, r.restarts_1h, r.live_1h), (2, Some(1), Some(2)));
    // Budget only, no component row: still counted.
    let s = json!({"components": [], "reflex": {"budgets": {"x-svc.service": {"live_1h": 3}}}});
    let r = restarts_for(&s, "x-svc").unwrap();
    assert_eq!((r.count, r.unit.as_deref()), (3, Some("x-svc.service")));
    // dry_1h (dry-run reflex actions) is NOT a restart.
    let s = json!({"components": [{"name": "y", "unit": "y.service", "restarts_1h": 0}],
                   "reflex": {"budgets": {"y.service": {"dry_1h": 5, "live_1h": 0}}}});
    assert_eq!(restarts_for(&s, "y").unwrap().count, 0);
}

#[test]
fn nerve_state_that_cannot_be_read_fails_closed() {
    for bad in [
        json!({}),
        json!({"components": "nope"}),
        json!({"components": [{"name": "z", "restarts_1h": "two"}]}),
        json!({"components": [{"name": "z", "restarts_1h": -1}]}),
        json!({"components": [{"name": "z", "unit": "z.service", "restarts_1h": 0}],
               "reflex": {"budgets": {"z.service": {"live_1h": 1.5}}}}),
    ] {
        assert!(restarts_for(&bad, "z").is_err(), "{bad}");
    }
    // A malformed row for ANOTHER service does not matter.
    let s = json!({"components": [{"name": "other", "restarts_1h": "x"}, {"name": "z", "restarts_1h": 1}]});
    assert_eq!(restarts_for(&s, "z").unwrap().count, 1);
}

#[test]
fn combined_cap_keeps_gaia_plus_nerve_at_three_per_hour() {
    let now = 1_800_000_000;
    // Nothing: 0 used. Nerve 2: one slot left for Gaia.
    assert_eq!(cap_check_combined(&[], 0, now), Ok(0));
    assert_eq!(cap_check_combined(&[], 2, now), Ok(2));
    assert_eq!(cap_check_combined(&[now - 100], 1, now), Ok(2));
    // Nerve 2 + Gaia 1 = 3: capped until Gaia's restart leaves the window.
    assert_eq!(
        cap_check_combined(&[now - 100], 2, now),
        Err(WINDOW_S - 100)
    );
    // Nerve 3 alone: capped; no times from the nerve -> worst case window.
    assert_eq!(cap_check_combined(&[], 3, now), Err(WINDOW_S));
    assert_eq!(cap_check_combined(&[], 7, now), Err(WINDOW_S));
    // Gaia 3 alone behaves exactly like the old cap_check.
    let h = [now - 3000, now - 2000, now - 1000];
    assert_eq!(cap_check_combined(&h, 0, now), heal::cap_check(&h, now));
    // Gaia 2 + nerve 1: Gaia waits for the older of its two.
    assert_eq!(
        cap_check_combined(&[now - 3000, now - 1000], 1, now),
        Err(600)
    );
    // Old restarts (outside the hour) never count.
    assert_eq!(
        cap_check_combined(&[now - WINDOW_S, now - 4000], 2, now),
        Ok(2)
    );
    assert_eq!(
        heal::recent_count(&[now - WINDOW_S, now - 5, now + 5], now),
        1
    );
    // Exhaustive: allowed iff gaia + nerve < 3.
    for g in 0..5usize {
        for n in 0..5usize {
            let h: Vec<u64> = (0..g as u64).map(|i| now - 10 - i).collect();
            assert_eq!(
                cap_check_combined(&h, n, now).is_ok(),
                g + n < 3,
                "g {g} n {n}"
            );
        }
    }
}

#[test]
fn hung_means_connected_but_no_answer() {
    // io::ErrorKind Display text, as gaia_live::http_get reports it.
    assert!(hung_detail("read: operation would block"));
    assert!(hung_detail("read: timed out"));
    assert!(hung_detail("write: timed out"));
    // Debug spellings and the probe's own deadline.
    assert!(hung_detail("read: WouldBlock"));
    assert!(hung_detail("read: TimedOut"));
    assert!(hung_detail("read: deadline"));
    // Not hung: nothing listening, no connection at all, or a real answer.
    assert!(!hung_detail("connect: connection refused"));
    assert!(
        !hung_detail("connect: timed out"),
        "no connection is down, not hung"
    );
    assert!(!hung_detail("HTTP 500"));
    assert!(!hung_detail("read: connection reset"));
    assert!(!hung_detail(""));
}

#[test]
fn listener_pids_finds_our_own_listener_and_nothing_on_a_closed_port() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    assert_eq!(heal::listener_pids(port), vec![std::process::id()]);
    drop(l);
    assert!(heal::listener_pids(port).is_empty());
}
