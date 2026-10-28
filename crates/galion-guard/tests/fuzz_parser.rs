// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Mutation fuzzing of the log parser (std-only PRNG, no fuzz framework).
//! `cargo test` runs a short fixed-seed pass; `GALION_FUZZ_SECS=60` runs
//! longer with a time-based seed (printed, so failures are reproducible
//! with `GALION_FUZZ_SEED`).

mod common;

use std::net::IpAddr;
use std::time::{Duration, Instant};

use galion_guard::engine::Engine;
use galion_guard::filter::{Host, SshdFilter};
use galion_guard::ipnet::{is_hard_protected, Net};
use galion_guard::jail::{Jail, JailParams, Protect};
use galion_guard::nft::DryRun;
use galion_guard::session::{Outcome, Sessions};
use galion_guard::source::{parse_journal_json, LogLine};
use galion_guard::timefmt;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const TOKENS: &[&str] = &[
    " from ",
    " port ",
    " ssh2",
    ": ",
    " [preauth]",
    "Invalid user ",
    "Failed password for ",
    "Failed publickey for invalid user ",
    "Accepted publickey for ",
    "invalid user ",
    "::ffff:",
    "::",
    "[",
    "]",
    "203.0.113.5",
    "127.0.0.1",
    "::1",
    "2001:db8::9",
    "999.999.1.1",
    "error: ",
    "fatal: ",
    "sshd[",
    "sshd-session[",
    "\u{0}",
    "\u{fffd}",
    "é",
    "\t",
    "  ",
    " on eth0",
    "1:2:3:4:5:6:7:8",
    "pam_unix(sshd:auth): authentication failure; ruser= rhost=",
    " user=root",
    "Connection closed by ",
    "Disconnecting: Too many authentication failures",
    "maximum authentication attempts exceeded for ",
];

fn seeds() -> Vec<String> {
    let t = common::T0;
    let mut v = vec![];
    v.extend(common::invalid_user(t, 1, "203.0.113.1", "admin"));
    v.extend(common::max_auth(t, 2, "203.0.113.2"));
    v.extend(common::accepted(t, 3, "203.0.113.3"));
    v.extend(common::noise(t, 4, "203.0.113.4"));
    let fx = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/auth-vps-shapes.log"
    ))
    .unwrap();
    v.extend(fx.lines().map(String::from));
    v
}

fn mutate(r: &mut Rng, s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    for _ in 0..(1 + r.below(6)) {
        match r.below(6) {
            0 if !chars.is_empty() => {
                let i = r.below(chars.len());
                chars.remove(i);
            }
            1 => {
                let i = r.below(chars.len() + 1);
                let c = char::from_u32((r.next() % 0x2FF) as u32).unwrap_or('x');
                chars.insert(i, c);
            }
            2 => {
                let i = r.below(chars.len() + 1);
                let tok: Vec<char> = TOKENS[r.below(TOKENS.len())].chars().collect();
                for (k, c) in tok.into_iter().enumerate() {
                    chars.insert(i + k, c);
                }
            }
            3 if chars.len() > 2 => {
                let a = r.below(chars.len());
                let b = r.below(chars.len());
                chars.swap(a, b);
            }
            4 if !chars.is_empty() => {
                let a = r.below(chars.len());
                let b = (a + r.below(40)).min(chars.len());
                let dup: Vec<char> = chars[a..b].to_vec();
                for (k, c) in dup.into_iter().enumerate() {
                    chars.insert(b + k, c);
                }
            }
            _ => {
                let n = r.below(30);
                for _ in 0..n {
                    chars.push(char::from_u32(32 + (r.next() % 95) as u32).unwrap());
                }
            }
        }
    }
    chars.into_iter().collect()
}

/// Every address a failure names must literally appear in the line (or its
/// IPv4-mapped / bracketed form): the parser can never invent a victim.
fn ip_in_line(ip: IpAddr, line: &str) -> bool {
    let s = ip.to_string();
    if line.contains(&s) {
        return true;
    }
    // IPv6 text forms differ (zero compression, case): parse every token.
    line.split(|c: char| {
        c.is_whitespace() || c == '[' || c == ']' || c == '(' || c == ')' || c == '='
    })
    .filter_map(|t| t.trim_end_matches([':', ',']).parse::<IpAddr>().ok())
    .any(|t| galion_guard::ipnet::canonical(t) == ip)
}

fn run(seed: u64, budget: Duration, max_iters: u64) -> u64 {
    let f = SshdFilter::new();
    let mut sessions = Sessions::new();
    let mut r = Rng(seed | 1);
    let base = seeds();
    let mut eng = Engine::new(
        Jail::new(
            JailParams {
                maxretry: 2,
                ..JailParams::default()
            },
            Protect {
                allow: vec![Net::parse("127.0.0.0/8").unwrap()],
                self_addrs: vec![],
            },
        ),
        Box::new(DryRun::new(None, None)),
        None,
    );
    let start = Instant::now();
    let mut n = 0;
    while n < max_iters && start.elapsed() < budget {
        n += 1;
        let pick = r.below(base.len());
        let line = mutate(&mut r, &base[pick]);
        let now = common::T0 + (n as i64 % 600);
        let _ = timefmt::split_stamp(&line, now, (r.below(28) as i64 - 14) * 3600);
        let _ = timefmt::parse_rfc3339(&line);
        let text = timefmt::split_stamp(&line, now, 0)
            .map(|(_, x)| x.to_string())
            .unwrap_or(line.clone());
        if let Some(m) = f.matches(&text) {
            assert!(m.rule < f.rule_count());
            if let Some(Host::Ip(ip)) = &m.host {
                assert!(
                    ip_in_line(*ip, &text),
                    "invented address {ip} from {text:?}"
                );
            }
            if let Outcome::Failure { ip, .. } = sessions.feed(&m, now) {
                // The address may come from an earlier line of the same connection.
                let _ = ip;
            }
        }
        eng.handle_line(
            &LogLine {
                time: None,
                text: line.clone(),
                cursor: None,
                file_pos: None,
            },
            now,
        );
        let _ = parse_journal_json(&line);
        let _ = parse_journal_json(&format!(
            "{{\"MESSAGE\":{:?},\"_HOSTNAME\":\"h\",\"_COMM\":\"sshd\"}}",
            line
        ));
        if n % 64 == 0 {
            eng.tick(now);
        }
    }
    for b in eng.jail.bans() {
        assert!(!is_hard_protected(b.ip), "banned loopback {}", b.ip);
    }
    n
}

#[test]
fn fuzz_parser_short() {
    let n = run(0x9e37_79b9_7f4a_7c15, Duration::from_secs(20), 30_000);
    assert!(n > 1000);
}

#[test]
fn fuzz_parser_timed() {
    let Ok(secs) = std::env::var("GALION_FUZZ_SECS") else {
        return;
    };
    let secs: u64 = secs.parse().unwrap_or(30);
    let seed = std::env::var("GALION_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        });
    eprintln!("fuzz seed {seed}, {secs} s");
    let n = run(seed, Duration::from_secs(secs), u64::MAX);
    eprintln!("fuzz: {n} inputs, no panic, no invented address, no loopback ban");
}
