// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Admin plan and account screens: display only, no billing call.
//!
//! A loopback stub answers `GET /api/v1/health` (the header dot) and
//! `GET /api/v1/admin` (the existing phone gate). Any other path fails
//! the test.

use console_ui::admin_catalog::UPSTREAM_READS;
use console_ui::admin_views::{self, ROUTES};
use console_ui::{Reply, Request, Ui, UiConfig};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn stub() -> (std::net::SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&log);
    thread::spawn(move || {
        for _ in 0..32 {
            let Ok((mut sock, _)) = listener.accept() else {
                break;
            };
            sock.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let head = String::from_utf8_lossy(&buf);
            let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
            recorded.lock().unwrap().push(path.clone());
            let body: &[u8] = if path.starts_with("/api/v1/admin") {
                br#"{"you":false}"#
            } else if path == "/api/v1/health" {
                br#"{"status":"healthy"}"#
            } else {
                br#"{"error":"not a display read"}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.write_all(body);
            let _ = sock.flush();
            let _ = sock.shutdown(Shutdown::Both);
        }
    });
    (addr, log)
}

fn ui(upstream: std::net::SocketAddr) -> Ui {
    Ui::new(UiConfig {
        upstream,
        pulse: None,
        extra_hosts: Vec::new(),
        desk: None,
        files: Default::default(),
    })
}

fn get(path: &'static str) -> Request<'static> {
    Request {
        method: "GET",
        path,
        host: "127.0.0.1",
        peer_loopback: true,
        ..Request::default()
    }
}

fn text(reply: &Reply) -> String {
    String::from_utf8(reply.body.clone()).unwrap()
}

fn assert_display_only(paths: &[String]) {
    for path in paths {
        let ok = path == "/api/v1/health" || path.starts_with("/api/v1/admin") || path.is_empty();
        assert!(
            ok,
            "admin screens called {path}, allowed reads are {UPSTREAM_READS:?} plus the phone gate"
        );
        let lower = path.to_ascii_lowercase();
        for banned in ["bill", "stripe", "checkout", "paypal", "charge", "pay"] {
            assert!(!lower.contains(banned), "{path} looks like a payment call");
        }
    }
}

fn unstyled(html: &str) -> Vec<String> {
    let defined = console_ui::css::defined_classes();
    let allow = [
        "primary", "ghost", "online", "offline", "held", "neutral", "fork", "chill", "i-open",
        "i-close",
    ];
    let mut missing = Vec::new();
    for cap in html.split("class=\"").skip(1) {
        let list = cap.split('"').next().unwrap_or("");
        for class in list.split_whitespace() {
            if !defined.iter().any(|d| d == class) && !allow.contains(&class) {
                missing.push(class.to_string());
            }
        }
    }
    missing.sort();
    missing.dedup();
    missing
}

#[test]
fn plans_and_accounts_render_without_a_billing_call() {
    let (addr, log) = stub();
    let ui = ui(addr);
    for path in ROUTES {
        assert!(
            ui.owns(path, true),
            "{path} stays Rust-owned with a SPA fallback"
        );
        assert!(ui.owns(path, false), "{path}");
    }
    assert!(!ui.owns("/admin", true));
    assert!(!admin_views::owns("/admin/plans/checkout"));

    let plans = ui.handle(&get("/admin/plans"));
    assert_eq!(plans.status, 200);
    let plans_html = text(&plans);
    assert!(plans_html.contains("<title>Galion Console · Plans</title>"));
    assert!(plans_html.contains("$0 · EUR 0 · forever"));
    assert!(plans_html.contains("$20 or EUR 20 per month"));
    assert!(plans_html.contains("$220 or EUR 220 one-time"));
    assert!(plans_html.contains("Open source. Free forever."));
    assert!(plans_html.contains("Galion One"));
    assert!(plans_html.contains("Galion Research"));
    assert!(plans_html.contains(">up<") || plans_html.contains(">up</span>"));
    assert!(!plans_html.contains("<form"));
    assert!(!plans_html.contains("<script"));
    assert!(
        unstyled(&plans_html).is_empty(),
        "{:?}",
        unstyled(&plans_html)
    );

    let accounts = ui.handle(&get("/admin/accounts"));
    assert_eq!(accounts.status, 200);
    let accounts_html = text(&accounts);
    assert!(accounts_html.contains("No users yet"));
    assert!(accounts_html.contains("This account list is empty"));
    assert!(accounts_html.contains("does not create an account"));
    assert!(accounts_html.contains("href=\"/admin/plans\""));
    assert!(!accounts_html.contains("<form"));
    assert!(!accounts_html.contains("$220"));
    assert!(
        unstyled(&accounts_html).is_empty(),
        "{:?}",
        unstyled(&accounts_html)
    );

    let head = ui.handle(&Request {
        method: "HEAD",
        ..get("/admin/plans")
    });
    assert_eq!(head.status, 200);

    let post = ui.handle(&Request {
        method: "POST",
        body: b"plan=ultra",
        content_type: Some("application/x-www-form-urlencoded"),
        ..get("/admin/plans")
    });
    assert_eq!(post.status, 405);
    assert_eq!(post.header("allow"), Some("GET, HEAD"));

    let missing = ui.handle(&get("/admin"));
    assert_eq!(missing.status, 404);

    assert_display_only(&log.lock().unwrap());
    assert!(
        log.lock().unwrap().iter().any(|p| p == "/api/v1/health"),
        "the header dot should read health, not a billing API"
    );
}

#[test]
fn unpaired_console_host_does_not_see_prices() {
    let (addr, log) = stub();
    let ui = ui(addr);
    let reply = ui.handle(&Request {
        method: "GET",
        path: "/admin/plans",
        host: "console.galion.app",
        peer_loopback: true,
        ..Request::default()
    });
    let html = text(&reply);
    assert!(html.contains("back=/admin/plans"), "{html}");
    assert!(!html.contains("$220"));
    assert!(!html.contains("$20 or EUR 20"));
    assert_display_only(&log.lock().unwrap());
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .all(|p| p.starts_with("/api/v1/admin")));
}

#[test]
fn public_host_stays_on_the_quiet_page() {
    let (addr, log) = stub();
    let ui = ui(addr);
    let reply = ui.handle(&Request {
        method: "GET",
        path: "/admin/accounts",
        host: "galion.app",
        peer_loopback: true,
        ..Request::default()
    });
    let html = text(&reply);
    assert!(html.contains("Soon online."));
    assert!(!html.contains("No users yet"));
    assert!(!html.contains("$20"));
    assert!(log.lock().unwrap().is_empty());
}
