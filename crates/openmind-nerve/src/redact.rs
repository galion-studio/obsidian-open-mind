// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Address and host redaction for text that leaves the nerve in `/state`
//! (slot 435, hub-incident-redact, D-422-2, 2026-10-28 CET +01:00).
//!
//! Pushed text (`POST /event` data, `POST /signal` msg) is untrusted and may
//! name the machine it talks about (`edge flood on 10.0.0.5:443`). The hub
//! keeps that text in `/state` (hub `recent`) and turns it into incident
//! messages and self-eval deductions, so it is redacted once at the hub's
//! ingest and again on the event-feed titles and details.
//!
//! Rules, per token (a run of `[A-Za-z0-9._:%\[\]-]`, so quotes, spaces,
//! `=`, `,`, `/` and `@` split tokens):
//!
//! - IPv4 / IPv6 (incl. a `%zone`), `ip:port`, `[ipv6]:port` -> `<addr>`;
//! - an address glued to a word by `_` (`src_10.0.0.5`) -> that part;
//! - `name:port` and dotted host names (`edge.example.org`,
//!   `nerve.lan:8670`) -> `<host>`. A dotted name needs an alphabetic last
//!   label of 2+ letters that is not a file or unit suffix (`server.rs`,
//!   `galion-nerve.service` stay); a single label needs `:port`
//!   (`localhost:8670`, `nerve:8670`) or is `localhost`.
//!
//! Redaction errs on the side of masking: an odd `word:42` becomes `<host>`.
//! Trailing `.` / `:` (end of a sentence) stay outside the token.

use std::net::{IpAddr, SocketAddr};

/// Replacement for an IP address or socket address.
pub const ADDR: &str = "<addr>";
/// Replacement for a host name (with or without a port).
pub const HOST: &str = "<host>";

/// Suffixes that make `a.b` a file or unit name, not a host.
const NOT_TLD: &[&str] = &[
    "rs", "toml", "json", "jsonl", "md", "txt", "log", "conf", "service", "socket", "timer",
    "target", "mount", "slice", "yml", "yaml", "sh", "db", "sqlite", "sock", "lock", "pid", "so",
    "html", "js", "css", "png", "svg", "gz", "tar", "zst", "bak", "tmp", "old", "new", "d", "c",
    "h", "py", "wasm", "bin", "crt", "pem", "key", "csv", "env", "ini", "cfg", "xml", "rules",
];

fn tok_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._:%[]-".contains(c)
}

fn is_ip(t: &str) -> bool {
    let t = t.split('%').next().unwrap_or(t);
    // A bare decimal like "1.5" is not an address; std accepts only dotted
    // quads for v4, and v6 needs at least two colons.
    t.parse::<IpAddr>().is_ok()
}

fn is_sock(t: &str) -> bool {
    if t.parse::<SocketAddr>().is_ok() {
        return true;
    }
    // `[fe80::1%eth0]:443`
    if let Some(rest) = t.strip_prefix('[') {
        if let Some((ip, port)) = rest.split_once("]:") {
            return is_ip(ip) && port_ok(port);
        }
        if let Some(ip) = rest.strip_suffix(']') {
            return is_ip(ip);
        }
    }
    false
}

fn port_ok(p: &str) -> bool {
    !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) && {
        let n: u32 = p.parse().unwrap_or(0);
        (1..=65535).contains(&n)
    }
}

fn label_ok(l: &str) -> bool {
    !l.is_empty()
        && l.len() <= 63
        && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !l.starts_with('-')
        && !l.ends_with('-')
}

fn is_dotted_host(t: &str) -> bool {
    let labels: Vec<&str> = t.split('.').collect();
    if labels.len() < 2 || !labels.iter().all(|l| label_ok(l)) {
        return false;
    }
    let last = labels[labels.len() - 1];
    last.len() >= 2
        && last.bytes().all(|b| b.is_ascii_alphabetic())
        && !NOT_TLD.contains(&last.to_ascii_lowercase().as_str())
}

fn is_host(t: &str) -> bool {
    if t.eq_ignore_ascii_case("localhost") || is_dotted_host(t) {
        return true;
    }
    match t.rsplit_once(':') {
        Some((h, p)) if port_ok(p) => {
            h.eq_ignore_ascii_case("localhost")
                || is_dotted_host(h)
                || (label_ok(h) && h.bytes().any(|b| b.is_ascii_alphabetic()))
        }
        _ => false,
    }
}

/// The replacement for one token, if it must be masked.
fn mask(t: &str) -> Option<&'static str> {
    if is_ip(t) || is_sock(t) {
        Some(ADDR)
    } else if is_host(t) {
        Some(HOST)
    } else {
        None
    }
}

/// One token: whole, else its `_`-separated parts (address parts only).
fn redact_token(t: &str, out: &mut String) {
    // Keep sentence punctuation outside the token.
    let core = t.trim_end_matches(['.', ':']);
    let tail = &t[core.len()..];
    if let Some(m) = mask(core) {
        out.push_str(m);
    } else if core.contains('_') {
        for (i, part) in core.split('_').enumerate() {
            if i > 0 {
                out.push('_');
            }
            let p = part.trim_end_matches(['.', ':']);
            if is_ip(p) || is_sock(p) {
                out.push_str(ADDR);
                out.push_str(&part[p.len()..]);
            } else {
                out.push_str(part);
            }
        }
    } else {
        out.push_str(core);
    }
    out.push_str(tail);
}

/// `s` with every IP address, socket address and host name masked.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tok = String::new();
    for c in s.chars() {
        if tok_char(c) || c == '_' {
            tok.push(c);
        } else {
            if !tok.is_empty() {
                redact_token(&tok, &mut out);
                tok.clear();
            }
            out.push(c);
        }
    }
    if !tok.is_empty() {
        redact_token(&tok, &mut out);
    }
    out
}

/// True when `s` still holds something [`redact`] would mask.
pub fn has_address(s: &str) -> bool {
    redact(s) != s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_plain_port_and_json() {
        assert_eq!(redact("ban 10.0.0.5"), "ban <addr>");
        assert_eq!(redact("edge flood on 10.0.0.5:443"), "edge flood on <addr>");
        assert_eq!(
            redact(r#"{"title":"edge flood on 10.0.0.5:443","fix":"ban 10.0.0.5"}"#),
            r#"{"title":"edge flood on <addr>","fix":"ban <addr>"}"#
        );
        assert_eq!(
            redact("from 192.0.2.8, then 198.51.100.4."),
            "from <addr>, then <addr>."
        );
        assert_eq!(redact("addr=127.0.0.1:8670/x"), "addr=<addr>/x");
        assert_eq!(redact("net 10.0.0.0/8"), "net <addr>/8");
        assert_eq!(redact("src_10.0.0.5_dst"), "src_<addr>_dst");
        assert_eq!(redact("http://10.0.0.5:8080/p"), "http://<addr>/p");
    }

    #[test]
    fn ipv6_forms() {
        assert_eq!(redact("dial ::1 refused"), "dial <addr> refused");
        assert_eq!(redact("dial [::1]:8670 refused"), "dial <addr> refused");
        assert_eq!(redact("peer 2001:db8::7 up"), "peer <addr> up");
        assert_eq!(redact("[2001:db8::7]:443,"), "<addr>,");
        assert_eq!(redact("fe80::1%eth0 link"), "<addr> link");
        assert_eq!(redact("[fe80::1%eth0]:22"), "<addr>");
        assert_eq!(redact("::ffff:10.0.0.5"), "<addr>");
    }

    #[test]
    fn hosts_and_host_port() {
        assert_eq!(redact("edge.example.org down"), "<host> down");
        assert_eq!(redact("nerve.lan:8670"), "<host>");
        assert_eq!(redact("localhost:8670 and localhost"), "<host> and <host>");
        assert_eq!(redact("origin nerve:8670 dead"), "origin <host> dead");
        assert_eq!(redact("mail grok@galion.studio"), "mail grok@<host>");
        assert_eq!(
            redact("https://console.galion.studio/x"),
            "https://<host>/x"
        );
    }

    #[test]
    fn leaves_ordinary_text() {
        for s in [
            "pushed 1 error(s) in the last 2 min; last: POST /x -> 502",
            "server.rs:190 galion-nerve.service config.toml",
            "hermes_rs.memory.cross_session",
            "v2.0 took 1.5 s at 21:34:51 on 2026-10-28T21:34:51+01:00",
            "e.g. i.e. 3 of 3",
            "http:// edge-flood ddos t158 gaia:t158",
            "",
        ] {
            assert_eq!(redact(s), s, "{s}");
            assert!(!has_address(s));
        }
        // Multibyte text passes through.
        assert_eq!(redact("čćž 10.0.0.5 ü"), "čćž <addr> ü");
    }

    #[test]
    fn idempotent() {
        let once = redact("a 10.0.0.5:443 b [::1]:1 c x.example.org");
        assert_eq!(redact(&once), once);
        assert!(!has_address(&once));
    }
}
