// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! sandbox-real-problems: real-life problem scenarios for openmind-sandbox,
//! run ONLY on the VPS (standing rule sandbox_vps_only) as an ordinary
//! non-root temp user, in a throwaway dir, on copies of real VPS data.
//!
//! One static binary, two roles:
//! * host side: `run <base>` (as the temp user, inside a Delegate=yes
//!   transient unit in its own slice) runs every scenario through
//!   `openmind_sandbox::run` and judges each one twice: by the tool's own
//!   `RESULT` lines and by a host-side check (reference counts computed by
//!   root from the live data, /proc scans for survivors, decoy listeners).
//!   `mask` (stdin -> stdout) masks IP addresses in a journal copy.
//! * sandbox side: the same binary, copied into each workdir as `./rp`,
//!   runs the tool subcommands in `payload.rs`.
mod payload;
mod runner;
mod sqlite;

use std::io::{Read, Write};

/// Mask every IPv4 address except 127.0.0.0/8 and every IPv6 address except ::1.
pub fn mask(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        // a URL escape such as %2F is a separator (SSRF probes: url=http%3A%2F%2F<ip>%2F)
        if c == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            out.push_str(&s[i..i + 3]);
            i += 3;
            continue;
        }
        let after_escape = i >= 3 && b[i - 3] == b'%';
        let prev_ok = i == 0
            || after_escape
            || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'.' || b[i - 1] == b':');
        if prev_ok && (c.is_ascii_hexdigit() || c == b':') {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_hexdigit() || b[j] == b'.' || b[j] == b':') {
                j += 1;
            }
            let tok = &s[i..j];
            let next_alpha = j < b.len() && b[j].is_ascii_alphanumeric();
            let colons = tok.matches(':').count();
            // IPv4 may carry a :port
            let (head, tail) = match tok.find(':') {
                Some(k) if colons == 1 => (&tok[..k], &tok[k..]),
                _ => (tok, ""),
            };
            let hv = head.trim_end_matches('.');
            let parts: Vec<&str> = hv.split('.').collect();
            let v4 = colons <= 1
                && parts.len() == 4
                && parts.iter().all(|p| {
                    !p.is_empty()
                        && p.len() <= 3
                        && p.bytes().all(|x| x.is_ascii_digit())
                        && p.parse::<u16>().map(|v| v <= 255).unwrap_or(false)
                });
            let v6 =
                (tok.contains("::") || colons >= 7) && tok.bytes().any(|x| x.is_ascii_hexdigit());
            if !next_alpha && v4 && parts[0] != "127" {
                out.push_str("X.X.X.X");
                out.push_str(&head[hv.len()..]);
                out.push_str(tail);
                i = j;
                continue;
            }
            if !next_alpha && v6 && tok != "::1" {
                out.push_str("IPV6");
                i = j;
                continue;
            }
            out.push_str(tok);
            i = j;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    let num = |i: usize| arg(i).parse::<usize>().unwrap_or(0);
    let rc = match a.first().map(String::as_str) {
        Some("mask") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).ok();
            let m = s.lines().map(mask).collect::<Vec<_>>().join("\n");
            std::io::stdout().write_all(m.as_bytes()).ok();
            std::io::stdout().write_all(b"\n").ok();
            0
        }
        Some("run") if a.len() == 2 => runner::run_all(&a[1]),
        // ---- sandbox side ----
        Some("diag502") => payload::diag_main(&arg(1)),
        Some("dbsuite") => payload::dbsuite(&arg(1), &arg(2)),
        Some("crashjob") => payload::crashjob(&arg(1)),
        Some("crashloop") => payload::crashloop(&arg(1)),
        Some("memhog") => payload::memhog(&arg(1)),
        Some("chunk") => payload::chunk(&arg(1), num(2), num(3)),
        Some("forkjob") => payload::forkjob(&arg(1), num(2)),
        Some("sleeper") => payload::sleeper(),
        Some("detach") => payload::detach(&arg(1)),
        Some("hang") => payload::hang(&arg(1)),
        Some("net") => payload::net(num(1) as u16, num(2) as u16),
        Some("secrets") => payload::secrets(&a[1..]),
        Some("fill") => payload::fill(num(1), num(2)),
        Some("procview") => payload::procview(),
        _ => {
            eprintln!("usage: sandbox-real-problems run <base> | mask   (sandbox side: diag502 dbsuite crashloop memhog forkjob hang net secrets fill procview)");
            2
        }
    };
    std::process::exit(rc);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_keeps_loopback_and_times() {
        let l = "2026-10-21T02:51:52+00:00 ip50 cloudflared[1]: dial tcp [::1]:4001 ip=198.51.100.7 to 203.0.113.9:443 v6=2001:db8::5 ok 127.0.0.1:8670 v2024.8.2 /x?url=http%3A%2F%2F192.0.2.44%2Flatest";
        let m = mask(l);
        assert!(m.contains("2026-10-21T02:51:52+00:00"));
        assert!(m.contains("[::1]:4001"));
        assert!(m.contains("ip=X.X.X.X"));
        assert!(m.contains("v6=IPV6"));
        assert!(m.contains("127.0.0.1:8670"));
        assert!(m.contains("v2024.8.2"));
        assert!(!m.contains("198.51"));
        assert!(m.contains("to X.X.X.X:443"));
        assert!(m.contains("%2F%2FX.X.X.X%2Flatest"));
    }
    #[test]
    fn diag_counts() {
        let t = "2026-10-21T02:51:52+00:00 h c[1]: 2026Z ERR  error=\"Unable to reach the origin service. x\" connIndex=1 originService=http://localhost:4001\n\
                 2026-10-21T02:51:53+00:00 h c[1]: 2026Z ERR Request failed error=\"Unable to reach the origin service. x\" dest=https://galion.studio/ type=http\n\
                 2026-10-21T02:51:54+00:00 h c[1]: 2026Z INF ok";
        let d = payload::diag(t);
        assert_eq!((d.lines, d.err_lines, d.requests, d.events), (3, 2, 1, 1));
        assert_eq!(d.by_dest.get("galion.studio"), Some(&1));
        assert_eq!(d.by_origin.get("http://localhost:4001"), Some(&1));
        assert_eq!(d.last, "2026-10-21T02:51:54+00:00");
    }
    #[test]
    fn sqlite_rejects_garbage() {
        assert!(sqlite::check(b"not a db", None).is_err());
    }
}
