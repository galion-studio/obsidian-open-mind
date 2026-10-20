// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Replay synthesized VPS-shaped auth logs through the full engine with a
//! simulated clock and the dry-run firewall: bans, timing, increments,
//! unbans, restart persistence, never-ban guarantees.

mod common;

use std::net::IpAddr;
use std::path::PathBuf;

use common::*;
use galion_guard::engine::{Engine, Event};
use galion_guard::ipnet::Net;
use galion_guard::jail::{Jail, JailParams, Protect};
use galion_guard::nft::DryRun;
use galion_guard::source::LogLine;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn engine(state: Option<PathBuf>) -> Engine {
    let protect = Protect {
        allow: vec![
            Net::parse("127.0.0.1/8").unwrap(),
            Net::parse("::1").unwrap(),
            Net::parse("198.51.100.0/24").unwrap(),
        ],
        self_addrs: vec![],
    };
    let mut e = Engine::new(
        Jail::new(JailParams::default(), protect),
        Box::new(DryRun::new(None, None)),
        state,
    );
    e.record = true;
    e
}

/// Feed lines with the clock following their timestamps (1 s ticks).
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

fn bans(e: &Engine) -> Vec<(IpAddr, i64, i64, u32)> {
    e.events
        .iter()
        .filter_map(|ev| match ev {
            Event::Ban {
                ip,
                at,
                until,
                count,
                ..
            } => Some((*ip, *at, *until, *count)),
            _ => None,
        })
        .collect()
}

fn unbans(e: &Engine) -> Vec<(IpAddr, i64, String)> {
    e.events
        .iter()
        .filter_map(|ev| match ev {
            Event::Unban { ip, at, reason } => Some((*ip, *at, reason.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn fixture_file_bans_exactly_one_attacker() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/auth-vps-shapes.log"
    ))
    .unwrap();
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let mut e = engine(None);
    let mut clock = T0;
    feed(&mut e, &lines, &mut clock);
    let b = bans(&e);
    // 203.0.113.10: four invalid users + maximum attempts = 5 failures.
    assert_eq!(b.len(), 1, "{:?}", e.events);
    assert_eq!(b[0].0, ip("203.0.113.10"));
    assert_eq!(b[0].1, T0 + 150);
    assert_eq!(b[0].2 - b[0].1, 3600);
    // Loopback hammering was ignored, without leaking the address.
    let prot = e
        .events
        .iter()
        .filter(|ev| matches!(ev, Event::Protected { .. }))
        .count();
    assert_eq!(prot, 6);
    assert_eq!(e.jail.totals.failed, 6); // 5 counted + the IPv6 empty-user line
    assert!(e.jail.ban_of(ip("127.0.0.1")).is_none());
    assert!(e.jail.ban_of(ip("198.51.100.7")).is_none());
    // The sudo line with an sshd-shaped message did not count.
    assert!(e.jail.ban_of(ip("203.0.113.99")).is_none());
}

#[test]
fn ban_timing_and_expiry() {
    let mut e = engine(None);
    let mut clock = T0;
    let mut lines = vec![];
    for i in 0..4 {
        lines.extend(invalid_user(
            T0 + 10 + i * 60,
            2000 + i as u32,
            "203.0.113.20",
            "x",
        ));
    }
    feed(&mut e, &lines, &mut clock);
    assert!(bans(&e).is_empty(), "4 failures must not ban");
    feed(
        &mut e,
        &invalid_user(T0 + 300, 2010, "203.0.113.20", "x"),
        &mut clock,
    );
    let b = bans(&e);
    assert_eq!(b, vec![(ip("203.0.113.20"), T0 + 300, T0 + 300 + 3600, 1)]);
    advance(&mut e, &mut clock, 3599);
    assert!(unbans(&e).is_empty());
    advance(&mut e, &mut clock, 1);
    assert_eq!(
        unbans(&e),
        vec![(ip("203.0.113.20"), T0 + 3900, "expired".to_string())]
    );
}

#[test]
fn findtime_window_slides() {
    let mut e = engine(None);
    let mut clock = T0;
    let mut lines = vec![];
    // 5 failures 225 s apart: the rate-rescaled count stays at 4 (fail2ban).
    for i in 0..5 {
        lines.extend(invalid_user(
            T0 + i * 225,
            3000 + i as u32,
            "203.0.113.21",
            "x",
        ));
    }
    feed(&mut e, &lines, &mut clock);
    assert!(bans(&e).is_empty());
    // One more 30 s later: round(4 / 630 * 600) + 1 = 5: ban.
    feed(
        &mut e,
        &invalid_user(T0 + 4 * 225 + 30, 3009, "203.0.113.21", "x"),
        &mut clock,
    );
    assert_eq!(bans(&e).len(), 1);
}

#[test]
fn increments_like_vps_and_purge() {
    let mut e = engine(None);
    let mut clock = T0;
    let a = "203.0.113.22";
    let mut lens = vec![];
    for round in 0..6u32 {
        let start = clock + 1;
        let mut lines = vec![];
        for k in 0..5 {
            lines.extend(invalid_user(
                start + k,
                4000 + round * 10 + k as u32,
                a,
                "x",
            ));
        }
        feed(&mut e, &lines, &mut clock);
        let b = e.jail.ban_of(ip(a)).unwrap().clone();
        lens.push((b.count, b.end - b.start));
        let left = b.end - clock;
        advance(&mut e, &mut clock, left);
        assert!(e.jail.ban_of(ip(a)).is_none());
    }
    assert_eq!(
        lens,
        vec![
            (1, 3600),
            (2, 7200),
            (3, 14_400),
            (4, 28_800),
            (5, 57_600),
            (6, 115_200)
        ]
    );
}

#[test]
fn restart_keeps_bans_history_and_cursor() {
    let dir = std::env::temp_dir().join(format!("galion-guard-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = dir.join("state.json");
    let a = "203.0.113.23";
    let mut clock = T0;
    {
        let mut e = engine(Some(state.clone()));
        e.startup(clock).unwrap();
        let mut lines = vec![];
        for k in 0..5 {
            lines.extend(invalid_user(T0 + 1 + k, 5000 + k as u32, a, "x"));
        }
        feed(&mut e, &lines, &mut clock);
        e.handle_line(
            &LogLine {
                time: Some(clock),
                text: "ip50 sshd[1]: x".into(),
                cursor: Some("s=cur;i=9".into()),
                file_pos: None,
            },
            clock,
        );
        e.save(clock);
        assert!(e.jail.ban_of(ip(a)).is_some());
    } // "crash": engine dropped
    clock += 1800;
    let mut e = engine(Some(state.clone()));
    e.startup(clock).unwrap();
    let b = e.jail.ban_of(ip(a)).expect("ban restored").clone();
    assert_eq!(b.end, T0 + 5 + 3600);
    assert_eq!(e.cursor.as_deref(), Some("s=cur;i=9"));
    assert_eq!(e.jail.totals.banned, 1);
    // The firewall got the ban back with the remaining time only.
    assert!(e
        .backend
        .list()
        .unwrap()
        .iter()
        .any(|(i, left)| *i == ip(a) && left.unwrap() <= 1805));
    // Expires on schedule after the restart.
    let left = b.end - clock;
    advance(&mut e, &mut clock, left);
    assert!(e.jail.ban_of(ip(a)).is_none());
    // History survived: the next ban is #2 (2h).
    let mut lines = vec![];
    for k in 0..5 {
        lines.extend(invalid_user(clock + 1 + k, 5100 + k as u32, a, "x"));
    }
    feed(&mut e, &lines, &mut clock);
    let b2 = e.jail.ban_of(ip(a)).unwrap();
    assert_eq!((b2.count, b2.end - b2.start), (2, 7200));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn admin_login_protects_and_lifts_ban() {
    let mut e = engine(None);
    let mut clock = T0;
    let admin = "203.0.113.24"; // not on the allowlist (roaming admin)
    let mut lines = vec![];
    for k in 0..5 {
        lines.extend(max_auth(T0 + 1 + k, 6000 + k as u32, admin));
    }
    feed(&mut e, &lines, &mut clock);
    assert!(e.jail.ban_of(ip(admin)).is_some());
    // The admin gets in another way (other port/VPN) and logs in: lifted.
    feed(&mut e, &accepted(clock + 10, 6100, admin), &mut clock);
    assert!(e.jail.ban_of(ip(admin)).is_none());
    assert_eq!(unbans(&e).last().unwrap().2, "recent-login");
    // Typos for the next 24 h never ban it again.
    let mut lines = vec![];
    for k in 0..20 {
        lines.extend(max_auth(clock + 20 + k, 6200 + k as u32, admin));
    }
    feed(&mut e, &lines, &mut clock);
    assert!(e.jail.ban_of(ip(admin)).is_none());
}

#[test]
fn noise_never_bans() {
    let mut e = engine(None);
    let mut clock = T0;
    let mut lines = vec![];
    for k in 0..200 {
        lines.extend(noise(T0 + k, 7000 + k as u32, "203.0.113.25"));
    }
    feed(&mut e, &lines, &mut clock);
    assert!(bans(&e).is_empty());
    assert_eq!(e.jail.totals.failed, 0);
}

#[test]
fn many_attackers_ipv4_and_ipv6() {
    let mut e = engine(None);
    let mut clock = T0;
    let mut lines = vec![];
    for k in 0..5i64 {
        for n in 0..50 {
            lines.extend(invalid_user(
                T0 + k * 10 + 1,
                (8000 + n * 10 + k) as u32,
                &format!("203.0.113.{}", 100 + n),
                "u",
            ));
            lines.extend(invalid_user(
                T0 + k * 10 + 2,
                (9000 + n * 10 + k) as u32,
                &format!("2001:db8::{:x}", 0x100 + n),
                "u",
            ));
        }
    }
    feed(&mut e, &lines, &mut clock);
    assert_eq!(bans(&e).len(), 100);
    let fw = e.backend.list().unwrap();
    assert_eq!(fw.len(), 100);
    assert_eq!(fw.iter().filter(|(i, _)| i.is_ipv6()).count(), 50);
}

#[test]
fn journal_lines_with_their_own_time() {
    let mut e = engine(None);
    for k in 0..5 {
        e.handle_line(
            &LogLine {
                time: Some(T0 + k),
                text: format!(
                    "ip50 sshd[{}]: Invalid user z from 203.0.113.26 port 1",
                    100 + k
                ),
                cursor: Some(format!("s=x;i={k}")),
                file_pos: None,
            },
            T0 + 10,
        );
    }
    assert_eq!(bans(&e).len(), 1);
    assert_eq!(e.cursor.as_deref(), Some("s=x;i=4"));
    // Lines older than findtime (journal backlog on start) are not counted.
    let mut e2 = engine(None);
    for k in 0..5 {
        e2.handle_line(
            &LogLine {
                time: Some(T0 + k),
                text: format!(
                    "ip50 sshd[{}]: Invalid user z from 203.0.113.27 port 1",
                    200 + k
                ),
                cursor: None,
                file_pos: None,
            },
            T0 + 3600,
        );
    }
    assert!(bans(&e2).is_empty());
}
