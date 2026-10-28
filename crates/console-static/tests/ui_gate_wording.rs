// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 410 (console-gate-wording, F-396-3 / D-396-3): the console
//! page gate must tell "the API has no admin route" (404 / 405 / TanStack
//! no-route 500) apart from "the API is not answering" (connect error, 5xx,
//! anything else). Both stay 503 and fail-closed (no room data).
//!
//! In-process server + one fake upstream per status. No network beyond
//! loopback ephemeral ports.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MISSING: &str = "Admin sign-in is not set up on this console";
const DOWN: &str = "Console API is not answering";
const REMOTE: &str = "console.galion.app";

/// Fake upstream: answers every request with `status` and `body`.
fn fake(status: u16, body: &'static str) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            s.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let out = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(out.as_bytes());
        }
    });
    a
}

/// A loopback port with nothing listening.
fn dead() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    drop(l);
    a
}

fn console(upstream: SocketAddr) -> SocketAddr {
    let up = upstream.to_string();
    let cfg = Config::from_args(
        ["--ui", "rust", "--upstream", up.as_str(), "--quiet"]
            .iter()
            .map(|s| s.to_string()),
    )
    .unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /\n").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    a
}

fn send(a: SocketAddr, raw: &str) -> (u16, String) {
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let t = String::from_utf8_lossy(&out).to_string();
    let code = t
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, t)
}

fn page(a: SocketAddr, path: &str) -> (u16, String) {
    send(
        a,
        &format!("GET {path} HTTP/1.1\r\nHost: {REMOTE}\r\nConnection: close\r\n\r\n"),
    )
}

fn pair_post(a: SocketAddr) -> (u16, String) {
    let body = "code=ABCD-EFGH";
    send(
        a,
        &format!(
            "POST /_ui/pair?back=/console HTTP/1.1\r\nHost: {REMOTE}\r\nOrigin: https://{REMOTE}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn gate_splits_no_admin_route_from_api_down() {
    let tanstack = r#"{"error":"Only HTML requests are supported here"}"#;
    let cases: Vec<(&str, SocketAddr, &str)> = vec![
        ("404", fake(404, "Not Found\n"), MISSING),
        ("405", fake(405, "Method Not Allowed\n"), MISSING),
        (
            "503 store not configured (5aa31927 --local-api, no --admin-store)",
            fake(503, r#"{"error":"admin store not configured"}"#),
            MISSING,
        ),
        (
            "503 store unreadable",
            fake(503, r#"{"error":"admin store unreadable"}"#),
            DOWN,
        ),
        ("500 tanstack no-route", fake(500, tanstack), MISSING),
        ("500", fake(500, r#"{"error":"boom"}"#), DOWN),
        ("502", fake(502, "Bad Gateway\n"), DOWN),
        ("503", fake(503, "Service Unavailable\n"), DOWN),
        ("401", fake(401, r#"{"error":"no"}"#), DOWN),
        ("connect refused", dead(), DOWN),
    ];
    for (tag, up, want) in cases {
        let c = console(up);
        for path in ["/console", "/agents", "/services"] {
            let (code, body) = page(c, path);
            assert_eq!(code, 503, "{tag} {path}: fail-closed");
            assert!(body.contains(want), "{tag} {path}: want {want:?}");
            let other = if want == MISSING { DOWN } else { MISSING };
            assert!(!body.contains(other), "{tag} {path}: not {other:?}");
            assert!(!body.contains("/_ui/pair"), "{tag} {path}: no pair form");
        }
    }
}

#[test]
fn pair_post_no_admin_route_is_not_code_refused() {
    // Upstream without POST /api/v1/admin: 503 "not set up", not 401 refused.
    for (tag, up) in [("404", fake(404, "")), ("405", fake(405, ""))] {
        let (code, body) = pair_post(console(up));
        assert_eq!(code, 503, "{tag}");
        assert!(body.contains(MISSING), "{tag}");
        assert!(!body.contains("That code was refused"), "{tag}");
    }
    // Real refusal (401) still says refused; API down still says not answering.
    let (code, body) = pair_post(console(fake(401, r#"{"error":"no"}"#)));
    assert_eq!(code, 401);
    assert!(body.contains("That code was refused"));
    let (code, body) = pair_post(console(dead()));
    assert_eq!(code, 503);
    assert!(body.contains(DOWN));
}

#[test]
fn gate_200_still_shows_pairing_page() {
    let c = console(fake(200, r#"{"ok":true,"paired":true,"you":false}"#));
    let (code, body) = page(c, "/console");
    assert_eq!(code, 200);
    assert!(body.contains("/_ui/pair"));
    assert!(!body.contains(MISSING) && !body.contains(DOWN));
}
