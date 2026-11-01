// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! galion.app account routes, forwarded to galion-accounts (2026-11-01 CET).
//!
//! On the home hosts only (`ONE_HOME_HOSTS`, see `home.rs`), these paths go
//! to the galion-accounts service on loopback (default `127.0.0.1:19970`):
//!
//! | path                          | owner            |
//! |-------------------------------|------------------|
//! | `/account`, `/account/*`      | page, css, js    |
//! | `/api/account/*`              | account API      |
//! | `/api/pay/*`                  | payment starts   |
//! | `/api/checkout`               | price buttons    |
//! | `/webhooks/*`                 | Stripe / crypto  |
//!
//! `ONE_ACCOUNTS_UPSTREAM` moves the upstream (`host:port`, loopback only);
//! `ONE_ACCOUNTS_UPSTREAM=off` switches forwarding off without a new binary.
//! When the upstream is off or down, `/account` shows the friendly built-in
//! placeholder (`home/account.html`) and the API paths answer 503 with a
//! plain "not available" body, so `HEAD /api/checkout` keeps the price
//! buttons on "Coming soon".
//!
//! What is forwarded: method, path and query, the request headers minus the
//! hop-by-hop ones, `Authorization`, any client-sent `X-Forwarded-*` /
//! `X-Real-IP`, and every cookie except `galion_acct` (the account session;
//! the Galion One pairing cookie never leaves this process). One
//! `X-Forwarded-For` is added: Cloudflare's `CF-Connecting-IP` when it is a
//! valid IP, else the TCP peer. galion-accounts uses it only for in-memory
//! rate limiting (`ACCOUNTS_TRUST_PROXY=1`). The body is sent with an exact
//! `Content-Length`; chunked request bodies are refused (411), so there is no
//! framing mismatch to smuggle through. The answer is relayed byte for byte.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

pub const DEFAULT_UPSTREAM: &str = "127.0.0.1:19970";
/// The account session cookie set by galion-accounts.
pub const SESSION_COOKIE: &str = "galion_acct";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// Stripe Checkout creation goes through curl inside galion-accounts.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Accounts answers are small; anything bigger is cut off.
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

/// True for the paths galion-accounts owns.
pub fn routes(path: &str) -> bool {
    path == "/account"
        || path.starts_with("/account/")
        || path.starts_with("/api/account/")
        || path.starts_with("/api/pay/")
        || path == "/api/checkout"
        || path.starts_with("/webhooks/")
}

/// The upstream address, or None when forwarding is switched off. Only
/// loopback addresses are accepted: the accounts port is never meant to be
/// reached over a network.
pub fn upstream(get: &dyn Fn(&str) -> Option<String>) -> Option<SocketAddr> {
    let raw = get("ONE_ACCOUNTS_UPSTREAM").unwrap_or_else(|| DEFAULT_UPSTREAM.to_string());
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("off") {
        return None;
    }
    let addr: SocketAddr = raw.parse().ok()?;
    addr.ip().is_loopback().then_some(addr)
}

const DROP: [&str; 15] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
    "proxy-authenticate",
    "te",
    "trailer",
    "upgrade",
    "content-length",
    "authorization",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip",
    "forwarded",
];

/// The client address to report upstream.
pub fn client_ip(head: &str, peer: Option<IpAddr>) -> String {
    for line in head.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("cf-connecting-ip") {
                if let Ok(ip) = value.trim().parse::<IpAddr>() {
                    return ip.to_string();
                }
            }
        }
    }
    peer.map(|p| p.to_string()).unwrap_or_default()
}

/// Only the account session cookie, or None when there is none.
fn session_cookie(value: &str) -> Option<String> {
    let kept: Vec<&str> = value
        .split(';')
        .map(str::trim)
        .filter(|c| {
            c.split_once('=')
                .is_some_and(|(n, _)| n.trim() == SESSION_COOKIE)
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

/// True when the request carries `Transfer-Encoding` (refused with 411).
pub fn chunked(head: &str) -> bool {
    head.lines().skip(1).any(|line| {
        line.split_once(':')
            .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case("transfer-encoding"))
    })
}

/// The request as sent to galion-accounts.
pub fn build_request(method: &str, target: &str, head: &str, body: &[u8], client: &str) -> Vec<u8> {
    let mut out = format!("{method} {target} HTTP/1.1\r\n");
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let lname = name.trim().to_ascii_lowercase();
        if lname.is_empty() || DROP.contains(&lname.as_str()) || lname.starts_with("x-forwarded-") {
            continue;
        }
        if lname == "cookie" {
            if let Some(c) = session_cookie(value) {
                out.push_str(&format!("Cookie: {c}\r\n"));
            }
            continue;
        }
        out.push_str(name.trim());
        out.push_str(": ");
        out.push_str(value.trim());
        out.push_str("\r\n");
    }
    if !client.is_empty() {
        out.push_str(&format!("X-Forwarded-For: {client}\r\n"));
    }
    out.push_str("X-Forwarded-Proto: https\r\n");
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// What happened to a forwarded request.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The upstream answered; this many bytes were relayed.
    Relayed(usize),
    /// Nothing was relayed: the upstream is down (refused, timed out or
    /// closed without a byte). The caller shows the fallback.
    Down,
}

/// Send `request` to `addr` and relay the answer to `client`. Once the first
/// byte has gone to the client a later error only ends the relay.
pub fn forward(addr: SocketAddr, request: &[u8], client: &mut dyn Write) -> Outcome {
    let Ok(mut up) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else {
        return Outcome::Down;
    };
    let _ = up.set_read_timeout(Some(READ_TIMEOUT));
    let _ = up.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = up.set_nodelay(true);
    if up.write_all(request).and_then(|()| up.flush()).is_err() {
        return Outcome::Down;
    }
    let mut buf = [0u8; 16 * 1024];
    let mut sent = 0usize;
    loop {
        let n = match up.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        let n = n.min(MAX_RESPONSE.saturating_sub(sent));
        if n == 0 || client.write_all(&buf[..n]).is_err() {
            break;
        }
        sent += n;
    }
    let _ = client.flush();
    if sent == 0 {
        Outcome::Down
    } else {
        Outcome::Relayed(sent)
    }
}

/// The answer when galion-accounts is off or down, or None for the paths
/// that fall back to the built-in placeholder page (`/account`).
pub fn unavailable(method: &str, path: &str) -> Option<Vec<u8>> {
    if matches!(method, "GET" | "HEAD") && (path == "/account" || path == "/account/") {
        return None;
    }
    let body: &[u8] = if path == "/api/checkout" {
        b"not connected\n"
    } else if path.starts_with("/api/") {
        br#"{"ok":false,"error":"unavailable","message":"Accounts are not available right now. Try again later."}"#
    } else {
        b"not available\n"
    };
    let kind = if body.starts_with(b"{") {
        "application/json"
    } else {
        "text/plain; charset=utf-8"
    };
    let mut out = format!(
        "HTTP/1.1 503 Service Unavailable\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\nRetry-After: 60\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    if method != "HEAD" {
        out.extend_from_slice(body);
    }
    Some(out)
}

/// 411 for a chunked request body.
pub fn length_required() -> Vec<u8> {
    b"HTTP/1.1 411 Length Required\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 24\r\nConnection: close\r\n\r\nsend a Content-Length.\r\n".to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn route_table() {
        for p in [
            "/account",
            "/account/",
            "/account/account.css",
            "/account/account.js",
            "/api/account/create",
            "/api/account/me",
            "/api/pay/stripe/checkout",
            "/api/pay/cash",
            "/api/checkout",
            "/webhooks/stripe",
            "/webhooks/crypto",
        ] {
            assert!(routes(p), "{p}");
        }
        for p in [
            "/",
            "/accounts",
            "/account.html",
            "/api/checkout/x",
            "/api/accounts",
            "/api/approvals",
            "/internal/v1/health",
            "/webhooks",
            "/home.js",
        ] {
            assert!(!routes(p), "{p}");
        }
    }

    #[test]
    fn upstream_is_loopback_only() {
        let none = |_: &str| None;
        assert_eq!(upstream(&none), Some(DEFAULT_UPSTREAM.parse().unwrap()));
        let set = |v: &'static str| move |_: &str| Some(v.to_string());
        assert_eq!(upstream(&set("off")), None);
        assert_eq!(upstream(&set("")), None);
        assert_eq!(upstream(&set("10.0.0.5:19970")), None);
        assert_eq!(upstream(&set("not an address")), None);
        assert_eq!(
            upstream(&set("[::1]:19975")),
            Some("[::1]:19975".parse().unwrap())
        );
    }

    const HEAD: &str = "POST /api/account/login HTTP/1.1\r\nHost: galion.app\r\nOrigin: https://galion.app\r\nContent-Type: application/json\r\nContent-Length: 999\r\nConnection: keep-alive\r\nAuthorization: Bearer nope\r\nX-Forwarded-For: 198.51.100.66\r\nX-Forwarded-Host: evil\r\nX-Real-IP: 198.51.100.66\r\nCookie: galion_one=pairing-token; galion_acct=sess123; other=1\r\nCF-Connecting-IP: 203.0.113.9\r\nStripe-Signature: t=1,v1=ab";

    #[test]
    fn request_is_rewritten() {
        let ip = client_ip(HEAD, Some("127.0.0.1".parse().unwrap()));
        assert_eq!(ip, "203.0.113.9");
        let req = build_request("POST", "/api/account/login?x=1", HEAD, b"{\"a\":1}", &ip);
        let s = String::from_utf8(req).unwrap();
        assert!(s.starts_with("POST /api/account/login?x=1 HTTP/1.1\r\n"));
        assert!(s.contains("Host: galion.app\r\n"));
        assert!(s.contains("Origin: https://galion.app\r\n"));
        assert!(s.contains("Content-Type: application/json\r\n"));
        assert!(s.contains("Stripe-Signature: t=1,v1=ab\r\n"));
        assert!(s.contains("Cookie: galion_acct=sess123\r\n"));
        assert!(!s.contains("pairing-token") && !s.contains("other=1"));
        assert!(!s.contains("Bearer") && !s.contains("198.51.100.66") && !s.contains("evil"));
        assert!(!s.contains("keep-alive") && !s.contains("999"));
        assert_eq!(s.matches("X-Forwarded-For:").count(), 1);
        assert!(s.contains("X-Forwarded-For: 203.0.113.9\r\n"));
        assert!(s.ends_with("Content-Length: 7\r\nConnection: close\r\n\r\n{\"a\":1}"));
    }

    #[test]
    fn client_ip_falls_back_to_peer() {
        let h = "GET / HTTP/1.1\r\nCF-Connecting-IP: not-an-ip";
        assert_eq!(
            client_ip(h, Some("127.0.0.1".parse().unwrap())),
            "127.0.0.1"
        );
        assert_eq!(client_ip(h, None), "");
        let r = String::from_utf8(build_request("GET", "/account", h, b"", "")).unwrap();
        assert!(!r.contains("X-Forwarded-For"));
        assert!(!r.contains("Cookie"));
    }

    #[test]
    fn chunked_is_detected() {
        assert!(chunked(
            "POST /webhooks/stripe HTTP/1.1\r\nTransfer-Encoding: chunked"
        ));
        assert!(!chunked(
            "POST /webhooks/stripe HTTP/1.1\r\nContent-Length: 3"
        ));
        assert!(String::from_utf8_lossy(&length_required()).starts_with("HTTP/1.1 411"));
    }

    #[test]
    fn fallbacks() {
        assert!(unavailable("GET", "/account").is_none(), "placeholder page");
        assert!(unavailable("HEAD", "/account/").is_none());
        let head = String::from_utf8(unavailable("HEAD", "/api/checkout").unwrap()).unwrap();
        assert!(head.starts_with("HTTP/1.1 503 "));
        assert!(head.ends_with("\r\n\r\n"), "HEAD has no body");
        let get = String::from_utf8(unavailable("GET", "/api/checkout").unwrap()).unwrap();
        assert!(get.ends_with("not connected\n"));
        let api = String::from_utf8(unavailable("POST", "/api/account/create").unwrap()).unwrap();
        assert!(api.contains("application/json") && api.contains("\"unavailable\""));
        let css = String::from_utf8(unavailable("GET", "/account/account.css").unwrap()).unwrap();
        assert!(css.starts_with("HTTP/1.1 503 ") && css.contains("no-store"));
        let hook = String::from_utf8(unavailable("POST", "/webhooks/stripe").unwrap()).unwrap();
        assert!(hook.starts_with("HTTP/1.1 503 "), "Stripe retries later");
    }

    /// A one-shot fake galion-accounts: records the request, sends `answer`.
    fn fake(answer: &'static [u8]) -> (SocketAddr, thread::JoinHandle<Vec<u8>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut got = Vec::new();
            let mut tmp = [0u8; 4096];
            // Read the head and the declared body.
            loop {
                let n = s.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&tmp[..n]);
                let text = String::from_utf8_lossy(&got).into_owned();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len: usize = text[..end]
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            s.write_all(answer).unwrap();
            got
        });
        (addr, h)
    }

    #[test]
    fn relays_the_answer_byte_for_byte() {
        let answer: &[u8] =
            b"HTTP/1.1 303 See Other\r\nlocation: /account?plan=plus\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        let (addr, h) = fake(answer);
        let req = build_request("GET", "/api/checkout?plan=plus", HEAD, b"", "203.0.113.9");
        let mut out = Vec::new();
        assert_eq!(
            forward(addr, &req, &mut out),
            Outcome::Relayed(answer.len())
        );
        assert_eq!(out, answer);
        assert_eq!(
            h.join().unwrap(),
            req,
            "upstream got exactly the rewritten request"
        );
    }

    #[test]
    fn down_upstream_is_reported() {
        // Bind then drop: the port is closed.
        let addr = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let mut out = Vec::new();
        assert_eq!(
            forward(addr, b"GET / HTTP/1.1\r\n\r\n", &mut out),
            Outcome::Down
        );
        assert!(out.is_empty());
        // Accepts, then closes without answering: also down.
        let (addr, h) = fake(b"");
        let req = build_request(
            "GET",
            "/account",
            "GET /account HTTP/1.1\r\nHost: galion.app",
            b"",
            "",
        );
        assert_eq!(forward(addr, &req, &mut out), Outcome::Down);
        h.join().unwrap();
        assert!(out.is_empty());
    }
}
