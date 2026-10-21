// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! console-route-31 (wave 4 slot 170): `/hermes` is Rust-first even when a
//! pre-built SPA fallback (`--root`) is configured. The real server runs
//! in-process with an SPA root (shell + an asset), the 31 live-source page
//! routes, the `/api/` proxy and a fake console API on loopback; raw
//! HTTP/1.1. Checks: `/hermes` is the Rust page (no script, CSP unchanged),
//! writes are held (POST/PUT 405, nothing upstream), every other live route
//! goes where `Ui::spa_fallbacks` says, and the old SPA's `/api/hermes/*`
//! proxy is untouched.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use console_ui::port2::LIVE_SRC_ROUTES;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const CSP: &str = "default-src 'none'; style-src 'self'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";
const SHELL: &str =
    "<!doctype html><title>old-spa-shell</title><script src=\"/assets/index-x.js\"></script>";

type Log = Arc<Mutex<Vec<(String, String, String)>>>;

fn fake_api(log: Log) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head, body) = loop {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_string();
                        let len: usize = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_string)
                            })
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        while buf.len() < p + 4 + len {
                            let n = s.read(&mut chunk).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                        }
                        break (head, String::from_utf8_lossy(&buf[p + 4..]).to_string());
                    }
                };
                let first = head.lines().next().unwrap_or("").to_string();
                let method = first.split(' ').next().unwrap_or("").to_string();
                let path = first.split(' ').nth(1).unwrap_or("").to_string();
                log.lock()
                    .unwrap()
                    .push((method.clone(), path.clone(), body.clone()));
                let out: String = match (method.as_str(), path.as_str()) {
                    ("GET", "/api/v1/health") => r#"{"status":"healthy","version":"t"}"#.into(),
                    ("GET", "/api/hermes/health") => r#"{"ok":true,"ms":1,"loopback":true,"bind":"127.0.0.1:8642","mode":"gateway","skills":[{"id":"a"}],"raw":"{\"implementation\":\"hermes-rs\",\"implementation_version\":\"0.1.0\"}"}"#.into(),
                    ("GET", "/api/hermes/guard") => r#"{"ok":true,"on":true,"autoAccept":true}"#.into(),
                    ("GET", "/api/hermes/modes") => r#"{"ok":true,"modes":{"plan":"p-model","action":"a-model"},"saved":[]}"#.into(),
                    ("GET", "/api/hermes/models") => r#"{"ok":true,"models":[{"id":"x/y","name":"Laptop Y"}]}"#.into(),
                    ("GET", "/api/hermes/history") => r#"{"ok":true,"items":[{"id":"t-1","title":"First","count":2,"at":1792340805,"source":"one","tokens":{"total":10}}]}"#.into(),
                    ("GET", "/api/hermes/history?id=t-1") => r#"{"ok":true,"tokens":{"input":1,"output":1,"total":2},"items":[{"kind":"user","text":"hello hermes"}]}"#.into(),
                    (_, p) => format!("{{\"echo\":\"{method} {p}\",\"body\":{}}}", if body.is_empty() { "null".into() } else { body.clone() }),
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                    out.len()
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    addr
}

fn spa_root() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "console-route31-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("assets")).unwrap();
    fs::write(d.join("_shell.html"), SHELL).unwrap();
    fs::write(d.join("assets/index-x.js"), "console.log(1)").unwrap();
    d
}

fn routes_text() -> String {
    LIVE_SRC_ROUTES
        .iter()
        .map(|p| {
            if *p == "/md/user" {
                "page /md/$name\n".to_string()
            } else {
                format!("page {p}\n")
            }
        })
        .collect()
}

fn start() -> (SocketAddr, Log, console_ui::Ui) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let api = fake_api(Arc::clone(&log));
    let root = spa_root();
    let args = [
        "--ui",
        "rust",
        "--upstream",
        &api.to_string(),
        "--root",
        root.to_str().unwrap(),
        "--proxy-prefix",
        "/api/",
        "--quiet",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();
    let cfg = Config::from_args(args).unwrap();
    let site = Site::load(&root, &cfg.shell).unwrap();
    assert!(site.shell().is_some(), "SPA fallback configured");
    let state = Arc::new(State::new(
        site,
        Routes::parse(&routes_text()).unwrap(),
        cfg,
    ));
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || server::serve(l, state));
    let ui = console_ui::Ui::new(console_ui::UiConfig {
        upstream: api,
        pulse: None,
        extra_hosts: vec![],
        desk: None,
    });
    (addr, log, ui)
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

impl Resp {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    }
}

fn send(a: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Resp {
        code: head
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn get(a: SocketAddr, p: &str) -> Resp {
    send(
        a,
        &format!("GET {p} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    )
}

fn non_get(log: &Log) -> Vec<(String, String, String)> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|(m, _, _)| m != "GET")
        .cloned()
        .collect()
}

#[test]
fn hermes_is_rust_first_under_spa_fallback() {
    let (a, log, _) = start();
    for p in ["/hermes", "/hermes?id=t-1"] {
        let r = get(a, p);
        assert_eq!(r.code, 200, "{p}");
        assert!(!r.body.contains("old-spa-shell"), "{p}: SPA took the room");
        assert!(r.body.contains("Hermes Research"), "{p}");
        assert!(!r.body.to_ascii_lowercase().contains("<script"), "{p}");
        assert!(!r.body.contains("style=\""), "{p}: inline style");
        assert!(!r.body.contains("<form"), "{p}: no form on /hermes");
        assert!(!r.body.contains("127.0.0.1:8642"), "{p}: bind hidden");
        assert_eq!(r.header("content-security-policy").as_deref(), Some(CSP));
        assert_eq!(r.header("x-frame-options").as_deref(), Some("DENY"));
        assert_eq!(r.header("cache-control").as_deref(), Some("no-store"));
    }
    let r = get(a, "/hermes");
    assert!(r.body.contains("Threads · 1 "));
    assert!(r
        .body
        .contains("served from Rust even when the old console is installed"));
    assert!(get(a, "/hermes?id=t-1")
        .body
        .contains("<pre class=\"say\">hello hermes</pre>"));
    let h = send(
        a,
        "HEAD /hermes HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(h.code, 200);
    assert!(h.body.is_empty(), "HEAD has no body");
    assert!(non_get(&log).is_empty());
}

#[test]
fn hermes_writes_are_held() {
    let (a, log, _) = start();
    let before = log.lock().unwrap().len();
    for (m, extra) in [
        ("POST", "Sec-Fetch-Site: same-origin\r\n"),
        ("POST", "Origin: http://127.0.0.1\r\n"),
        ("POST", "Sec-Fetch-Site: cross-site\r\n"),
        ("PUT", "Sec-Fetch-Site: same-origin\r\n"),
        ("DELETE", ""),
        ("PATCH", ""),
    ] {
        let body = "input=hello&act=guard&on=0";
        let r = send(
            a,
            &format!(
                "{m} /hermes HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert_eq!(r.code, 405, "{m} {extra}");
        assert_eq!(r.header("allow").as_deref(), Some("GET, HEAD"));
        assert!(!r.body.contains("old-spa-shell"));
    }
    let after = log.lock().unwrap().len();
    assert_eq!(after, before, "a held write reached the API");
    assert!(non_get(&log).is_empty());
}

#[test]
fn every_live_route_goes_where_spa_fallbacks_says() {
    let (a, _, ui) = start();
    let fallbacks = ui.spa_fallbacks(true);
    assert!(!fallbacks.contains(&"/hermes"));
    // Exactly the live routes that are neither ported nor `/` (on 0916684:
    // 13), minus nothing: no ported room is handed to the SPA any more.
    let expect: Vec<&str> = LIVE_SRC_ROUTES
        .iter()
        .copied()
        .filter(|p| *p != "/" && !console_ui::PORTED.contains(p))
        .collect();
    assert_eq!(fallbacks, expect);
    let mut rust = 0;
    for p in LIVE_SRC_ROUTES {
        let r = get(a, p);
        let spa = r.body.contains("old-spa-shell");
        if fallbacks.contains(p) {
            assert_eq!(r.code, 200, "{p}");
            assert!(spa, "{p}: expected the SPA shell");
        } else {
            rust += 1;
            assert!(!spa, "{p}: expected the Rust page");
            assert!(
                !r.body.to_ascii_lowercase().contains("<script"),
                "{p}: Rust page has a script"
            );
            assert!(matches!(r.code, 200 | 302), "{p}: {}", r.code);
        }
    }
    assert_eq!(rust, console_ui::PORTED.len() + 1, "PORTED + /");
    assert_eq!(rust + fallbacks.len(), 31);
}

#[test]
fn old_spa_hermes_api_proxy_untouched() {
    let (a, log, _) = start();
    let body = r#"{"input":"x"}"#;
    let r = send(
        a,
        &format!(
            "POST /api/hermes/chat HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    assert_eq!(r.code, 200);
    assert!(
        r.body.contains("\"echo\":\"POST /api/hermes/chat\""),
        "{}",
        r.body
    );
    let p = non_get(&log);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].2, body);
    let r = get(a, "/assets/index-x.js");
    assert_eq!(r.code, 200, "SPA assets still served");
}
