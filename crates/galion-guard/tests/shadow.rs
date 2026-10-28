// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Shadow mode through the full engine: the fail2ban-format shadow log, its
//! agreement with a fail2ban-layout reference written independently here
//! (UTC stamps, `fail2ban.*` loggers, exactly the VPS line shapes), restore
//! lines after a restart, and the never-print guarantee for protected
//! addresses.

mod common;

use std::net::IpAddr;
use std::path::PathBuf;

use common::*;
use galion_guard::compare::{compare, render, Options};
use galion_guard::engine::{Engine, Event};
use galion_guard::f2blog::{parse_log, Record, ShadowLog, Zone};
use galion_guard::ipnet::Net;
use galion_guard::jail::{Jail, JailParams, Protect};
use galion_guard::nft::Shadow;
use galion_guard::source::LogLine;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("guard-shadow-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn engine(state: Option<PathBuf>, log: PathBuf) -> Engine {
    let protect = Protect {
        allow: vec![
            Net::parse("127.0.0.1/8").unwrap(),
            Net::parse("::1").unwrap(),
            Net::parse("198.51.100.0/24").unwrap(),
        ],
        self_addrs: vec![],
    };
    let params = JailParams {
        protect_accepted: 0,
        ..JailParams::default()
    };
    let mut e = Engine::new(
        Jail::new(params, protect),
        Box::new(Shadow::default()),
        state,
    );
    e.record = true;
    e.emit_found = true;
    e.shadow = Some(ShadowLog::new(log, Zone::Zagreb));
    e
}

fn feed(e: &mut Engine, lines: &[String], clock: &mut i64) {
    for l in lines {
        let (t, _) = galion_guard::timefmt::split_stamp(l, *clock, 0).unwrap();
        while *clock < t {
            *clock += 1;
            e.tick(*clock);
        }
        e.handle_line(
            &LogLine {
                time: None,
                text: l.clone(),
                cursor: None,
                file_pos: None,
            },
            *clock,
        );
    }
}

fn advance(e: &mut Engine, clock: &mut i64, secs: i64) {
    for _ in 0..secs {
        *clock += 1;
        e.tick(*clock);
    }
}

/// Attack corpus: one attacker banned three times (1h, 2h, 4h), one slow
/// attacker banned once, an allowlisted admin failing often, noise.
fn corpus() -> Vec<Vec<String>> {
    let mut phases = Vec::new();
    let mut pid = 5000;
    let mut next = || {
        pid += 1;
        pid
    };
    for round in 0..3i64 {
        let base = T0 + round * 5 * 3600;
        let mut v = Vec::new();
        for k in 0..6 {
            v.extend(invalid_user(base + k * 20, next(), "203.0.113.10", "admin"));
            v.extend(invalid_user(
                base + k * 20 + 5,
                next(),
                "198.51.100.7",
                "root",
            ));
            v.extend(noise(base + k * 20 + 7, next(), "192.0.2.50"));
        }
        // Slow-but-steady attacker: one failure every 100 s.
        for k in 0..6 {
            v.extend(invalid_user(
                base + 200 + k * 100,
                next(),
                "192.0.2.99",
                "pi",
            ));
        }
        phases.push(v);
    }
    phases
}

/// The reference "fail2ban.log": written from the engine's events with
/// fail2ban's own layout and UTC stamps, independent of `f2blog`'s writer.
fn reference_f2b(events: &[Event]) -> String {
    fn utc(t: i64) -> String {
        // stamp() gives "YYYY-MM-DDTHH:MM:SS.000001+00:00"
        let s = stamp(t);
        format!("{} {}", &s[..10], &s[11..19])
    }
    let mut out = String::new();
    for e in events {
        match e {
            Event::Found { at, ip, logged } => {
                out += &format!(
                    "{},719 fail2ban.filter         [1268562]: INFO    [sshd] Found {ip} - {}\n",
                    utc(*at),
                    utc(*logged)
                )
            }
            Event::Ban {
                ip,
                at,
                until,
                count,
                ..
            } => {
                out += &format!(
                    "{},883 fail2ban.actions        [1268562]: NOTICE  [sshd] Ban {ip}\n",
                    utc(*at)
                );
                if *count >= 2 {
                    out += &format!(
                        "{},450 fail2ban.observer       [1268562]: INFO    [sshd] IP {ip} is bad: 1 # last x - incr\n\
                         {},450 fail2ban.observer       [1268562]: NOTICE  [sshd] Increase Ban {ip} ({count} # {} -> {})\n",
                        utc(*at),
                        utc(*at),
                        galion_guard::duration::format((until - at) as u64),
                        utc(*until)
                    );
                }
            }
            Event::Unban { ip, at, .. } => {
                out += &format!(
                    "{},150 fail2ban.actions        [1268562]: NOTICE  [sshd] Unban {ip}\n",
                    utc(*at)
                )
            }
            _ => {}
        }
    }
    out
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn shadow_log_matches_fail2ban_reference() {
    let d = tmp("ref");
    let log = d.join("shadow.log");
    let mut e = engine(None, log.clone());
    e.startup(T0 - 1).unwrap();
    let mut clock = T0 - 1;
    for (i, p) in corpus().into_iter().enumerate() {
        feed(&mut e, &p, &mut clock);
        let next = T0 + (i as i64 + 1) * 5 * 3600 - 1;
        let n = next - clock;
        advance(&mut e, &mut clock, n);
    }
    let text = std::fs::read_to_string(&log).unwrap();
    let ours = parse_log(&text, Zone::Zagreb, "sshd");
    let bans: Vec<&Record> = ours
        .iter()
        .filter(|r| matches!(r, Record::Ban { .. }))
        .collect();
    assert_eq!(bans.len() as u64, e.jail.totals.banned);
    assert_eq!(
        ours.iter()
            .filter(|r| matches!(r, Record::Found { .. }))
            .count() as u64,
        e.jail.totals.failed
    );
    // 203.0.113.10: 1h, 2h, 4h.
    let incs: Vec<(u32, u64)> = ours
        .iter()
        .filter_map(|r| match r {
            Record::Increase {
                ip: i, count, secs, ..
            } if *i == ip("203.0.113.10") => Some((*count, *secs)),
            _ => None,
        })
        .collect();
    assert_eq!(incs, vec![(2, 7200), (3, 14_400)], "{text}");
    // The allowlisted admin never appears; its hits are logged withheld.
    assert!(!text.contains("198.51.100.7"));
    assert!(text.contains("Ignore <protected address withheld> by Allowlist"));
    // Same columns as fail2ban.
    assert!(text
        .lines()
        .all(|l| l.as_bytes().get(23) == Some(&b' ') && l[24..].starts_with("galion-guard.")));

    let reference = reference_f2b(&e.events);
    let f2b = parse_log(&reference, Zone::Utc, "sshd");
    let r = compare(&f2b, &ours, &Options::default());
    let report = render(&r, true);
    assert!(r.parity, "{report}");
    assert_eq!(r.bans_same_second, e.jail.totals.banned);
    assert_eq!(r.fail2ban.increased_bans, r.guard.increased_bans);
    assert!(r.guard.increased_bans >= 2);
    assert!(report.contains("verdict: PARITY"));
    // Drop one fail2ban ban: reported, with the address, as ONLY GUARD.
    let cut = reference
        .lines()
        .filter(|l| !(l.contains("NOTICE  [sshd] Ban 192.0.2.99")))
        .collect::<Vec<_>>()
        .join("\n");
    let r2 = compare(
        &parse_log(&cut, Zone::Utc, "sshd"),
        &ours,
        &Options::default(),
    );
    assert!(!r2.parity);
    assert!(r2.bans_only_guard >= 1);
    assert!(render(&r2, true).contains("ONLY GUARD"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn shadow_restart_logs_restore_and_never_touches_firewall() {
    let d = tmp("restart");
    let log = d.join("shadow.log");
    let state = d.join("state.json");
    let mut clock = T0;
    {
        let mut e = engine(Some(state.clone()), log.clone());
        e.startup(clock).unwrap();
        let mut v = Vec::new();
        for k in 0..5 {
            v.extend(invalid_user(
                T0 + 1 + k,
                700 + k as u32,
                "203.0.113.33",
                "x",
            ));
        }
        feed(&mut e, &v, &mut clock);
        assert_eq!(e.jail.bans().count(), 1);
        assert_eq!(e.backend.name(), "shadow");
        assert!(!e
            .events
            .iter()
            .any(|ev| matches!(ev, Event::BackendError { .. })));
        e.save(clock);
    }
    let mut e = engine(Some(state), log.clone());
    advance(&mut e, &mut clock, 10);
    e.startup(clock).unwrap();
    assert_eq!(e.jail.bans().count(), 1);
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.contains("NOTICE  [sshd] Restore Ban 203.0.113.33"),
        "{text}"
    );
    let recs = parse_log(&text, Zone::Zagreb, "sshd");
    assert!(recs
        .iter()
        .any(|r| matches!(r, Record::Ban { restored: true, .. })));
    // Restores are not counted as bans in a comparison.
    let r = compare(&recs, &recs, &Options::default());
    assert_eq!(r.fail2ban.bans, 1);
    assert_eq!(r.fail2ban.restored_bans, 1);
    let _ = std::fs::remove_dir_all(&d);
}
