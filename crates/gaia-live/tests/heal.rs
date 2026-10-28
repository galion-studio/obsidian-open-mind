// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Guards of the M2 apply path: scope, restart cap, ledger. (Approval is
//! hades-gate's signed tokens now; see crates/gaia/tests/heal_tick.rs.)

use gaia_live::heal::*;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gaia-live-heal-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Some absolute program that exists on any unix box and is not forbidden.
fn prog() -> String {
    for p in ["/bin/true", "/usr/bin/true", "/bin/echo", "/usr/bin/echo"] {
        if std::path::Path::new(p).is_file() {
            return p.into();
        }
    }
    panic!("no /bin/true or /bin/echo")
}

fn target(port: u16, argv: Vec<String>) -> TestTarget {
    TestTarget {
        name: "gaia-dummy".into(),
        port,
        health_path: "/health".into(),
        argv,
    }
}

fn ok_argv(port: u16) -> Vec<String> {
    vec![prog(), "--port".into(), port.to_string()]
}

#[test]
fn scope_accepts_a_box_test_service() {
    check_target(&target(19032, ok_argv(19032)), &(19030..=19039)).unwrap();
}

#[test]
fn scope_refuses_live_stack_ports_and_wide_ranges() {
    for live in [8080u16, 8091, 8002, 8420, 8642, 8646, 8650, 8670] {
        let e = check_target(&target(live, ok_argv(live)), &(19030..=19039)).unwrap_err();
        assert!(e.contains("outside the allowed"), "{live}: {e}");
        // Even an operator range that includes the live port is refused.
        let e = check_target(&target(live, ok_argv(live)), &(1..=65535)).unwrap_err();
        assert!(e.contains("outside the test range"), "{live}: {e}");
    }
    let e = check_target(&target(19040, ok_argv(19040)), &(19030..=19039)).unwrap_err();
    assert!(e.contains("outside the allowed"), "{e}");
}

#[test]
fn scope_refuses_service_managers_shells_and_strangers() {
    for bad in [
        "/usr/bin/systemctl",
        "/bin/systemctl",
        "/usr/sbin/service",
        "/bin/sh",
        "/bin/bash",
        "/usr/bin/sudo",
        "/usr/bin/ssh",
        "/bin/kill",
        "/usr/bin/pkill",
        "/usr/bin/python3",
        "/usr/bin/env",
    ] {
        let t = target(19032, vec![bad.into(), "restart".into(), "19032".into()]);
        let e = check_target(&t, &(19030..=19039)).unwrap_err();
        assert!(e.contains("forbidden"), "{bad}: {e}");
    }
    let rel = target(
        19032,
        vec!["gaia-dummy".into(), "--port".into(), "19032".into()],
    );
    assert!(check_target(&rel, &(19030..=19039))
        .unwrap_err()
        .contains("absolute"));
    let missing = target(
        19032,
        vec!["/nonexistent/gaia-dummy".into(), "19032".into()],
    );
    assert!(check_target(&missing, &(19030..=19039))
        .unwrap_err()
        .contains("does not exist"));
    let other_port = target(19032, vec![prog(), "--port".into(), "8420".into()]);
    assert!(check_target(&other_port, &(19030..=19039))
        .unwrap_err()
        .contains("does not name the target port"));
    let empty = target(19032, vec![]);
    assert!(check_target(&empty, &(19030..=19039)).is_err());
    let mut bad_name = target(19032, ok_argv(19032));
    bad_name.name = "x; rm -rf /".into();
    assert!(check_target(&bad_name, &(19030..=19039)).is_err());
}

#[test]
fn range_parse() {
    assert_eq!(parse_range("19030-19039").unwrap(), 19030..=19039);
    assert_eq!(parse_range("19032").unwrap(), 19032..=19032);
    assert!(parse_range("a-b").is_err());
}

#[test]
fn cap_is_three_per_rolling_hour() {
    let now = 1_000_000u64;
    assert_eq!(cap_check(&[], now), Ok(0));
    assert_eq!(cap_check(&[now - 10], now), Ok(1));
    assert_eq!(cap_check(&[now - 10, now - 5], now), Ok(2));
    let full = [now - 600, now - 10, now - 5];
    assert_eq!(cap_check(&full, now), Err(WINDOW_S - 600));
    // Restarts older than an hour no longer count.
    assert_eq!(cap_check(&[now - 3600, now - 10, now - 5], now), Ok(2));
    // Future stamps (clock skew) do not count either.
    assert_eq!(cap_check(&[now + 50, now - 10, now - 5], now), Ok(2));
    assert_eq!(MAX_RESTARTS_PER_HOUR, 3);
}

#[test]
fn ledger_round_trip_per_service() {
    let d = tmp("ledger");
    let l = Ledger::new(&d);
    assert!(l.history("a").unwrap().is_empty());
    l.record("a", 10, 1).unwrap();
    l.record("b", 11, 2).unwrap();
    l.record("a", 12, 3).unwrap();
    assert_eq!(l.history("a").unwrap(), vec![10, 12]);
    assert_eq!(l.history("b").unwrap(), vec![11]);
    let text = std::fs::read_to_string(l.path()).unwrap();
    assert!(
        text.contains("+01:00\"") || text.contains("+02:00\""),
        "Europe/Zagreb stamp with explicit offset: {text}"
    );
}

// ------------------------------------------------ S3.1 global budget

#[test]
fn global_ledger_sits_next_to_the_hades_replay_ledger() {
    assert_eq!(
        global_ledger_path(std::path::Path::new("/var/lib/galion/hades/ledger.jsonl")),
        PathBuf::from("/var/lib/galion/hades/gaia-restarts.jsonl")
    );
    assert_eq!(
        global_ledger_path(std::path::Path::new("ledger.jsonl")),
        PathBuf::from(GLOBAL_LEDGER_FILE)
    );
}

#[test]
fn a_new_state_dir_sees_the_global_history() {
    let d = tmp("budget-new-state");
    let global = d.join("gate/gaia-restarts.jsonl");
    let a = Budget::new(&d.join("state-a"), &global);
    std::fs::create_dir_all(d.join("state-a")).unwrap();
    let now = epoch_now();
    for i in 0..3 {
        a.record("svc", now - 10 + i, 100 + i as u32).unwrap();
    }
    // Before S3.1 a fresh --state dir had an empty ledger: fresh budget.
    assert!(Ledger::new(&d.join("state-b"))
        .history("svc")
        .unwrap()
        .is_empty());
    assert_eq!(cap_check(&[], now), Ok(0));
    // Now the budget includes the global ledger: still capped.
    let b = Budget::new(&d.join("state-b"), &global);
    let h = b.history("svc").unwrap();
    assert_eq!(h.len(), 3);
    assert!(cap_check(&h, now).is_err());
    // Other services are not affected.
    assert!(b.history("other").unwrap().is_empty());
}

#[test]
fn a_restart_in_both_ledgers_counts_once() {
    let d = tmp("budget-dedup");
    std::fs::create_dir_all(d.join("s")).unwrap();
    let b = Budget::new(&d.join("s"), &d.join("g.jsonl"));
    b.record("svc", 1000, 7).unwrap();
    b.record("svc", 1001, 8).unwrap();
    assert_eq!(b.history("svc").unwrap(), vec![1000, 1001]);
    // An older state-only record (from before the global ledger) still counts.
    Ledger::new(&d.join("s")).record("svc", 999, 6).unwrap();
    assert_eq!(b.history("svc").unwrap(), vec![999, 1000, 1001]);
    // Global-only (written by another --state) counts as well.
    Ledger::at(d.join("g.jsonl"))
        .record_synced("svc", 1002, 9)
        .unwrap();
    assert_eq!(b.history("svc").unwrap().len(), 4);
}

#[test]
fn corrupt_or_torn_global_ledger_is_an_error() {
    let d = tmp("budget-torn");
    std::fs::create_dir_all(d.join("s")).unwrap();
    let g = d.join("g.jsonl");
    std::fs::write(
        &g,
        "{\"service\":\"svc\",\"epoch_s\":5,\"pid\":1}\n{\"service\":\"sv",
    )
    .unwrap();
    let e = Budget::new(&d.join("s"), &g).history("svc").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
    assert!(e.to_string().contains("g.jsonl"), "{e}");
}

#[test]
fn budget_lock_is_exclusive_between_threads() {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    let d = tmp("budget-lock");
    let b = Budget::new(&d.join("s"), &d.join("gate/g.jsonl"));
    let order: Arc<Mutex<Vec<&str>>> = Arc::default();
    let held = b.lock().unwrap();
    let (b2, o2) = (b.clone(), order.clone());
    let t0 = Instant::now();
    let h = std::thread::spawn(move || {
        let _l = b2.lock().unwrap();
        o2.lock().unwrap().push("second");
        t0.elapsed()
    });
    std::thread::sleep(Duration::from_millis(300));
    order.lock().unwrap().push("first-release");
    drop(held);
    let waited = h.join().unwrap();
    assert_eq!(*order.lock().unwrap(), vec!["first-release", "second"]);
    assert!(waited >= Duration::from_millis(250), "{waited:?}");
}

#[test]
fn concurrent_recorders_never_tear_lines() {
    let d = tmp("budget-concurrent");
    let g = d.join("g.jsonl");
    let hs: Vec<_> = (0..8)
        .map(|t| {
            let (s, g) = (d.join(format!("s{t}")), g.clone());
            std::thread::spawn(move || {
                std::fs::create_dir_all(&s).unwrap();
                let b = Budget::new(&s, &g);
                for i in 0..25u64 {
                    let _l = b.lock().unwrap();
                    b.record("svc", 10_000 + t * 100 + i, (t * 100 + i) as u32)
                        .unwrap();
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let all = Ledger::at(g).entries("svc").unwrap();
    assert_eq!(all.len(), 200);
}
