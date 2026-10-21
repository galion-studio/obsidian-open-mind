// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end tests for the console-port-2 rooms (`/context`, `/gateway`,
//! `/guardian`, `/hermes`): the real server (`--ui rust`) in-process, a
//! fake console API + fake galion-desk on an ephemeral loopback port, raw
//! HTTP/1.1. Covers rendering, the gate, CSRF refusals, the fixed POST
//! list of `/gateway`, the Guardian PIN header (never echoed) and that
//! `/context` and `/hermes` only GET.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// One upstream request: method, path, raw head, body.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    head: String,
    body: String,
}

type Log = Arc<Mutex<Vec<Seen>>>;

const DESK: &str = r#"{"line":true,"model":"galion","vram":8200000000,"ctx":4096,
"mindpack":{"context":{"cached_sessions":2,"cached_turns":5,"model_window":65536},
"cache":{"input":2500,"output":10,"read":3000000,"reasoning":0},"roles":{"user":1,"assistant":2,"tool":3},
"mind":{"line":"memory <line>","sessions":7,"messages":70},
"road":[{"id":"openmind-boss-20261018","source":"api_server","model":"m1","messages":3,"at":1792340805}],
"tools":[{"name":"terminal","n":10},{"name":"read_file","n":5}],
"models":[{"name":"deepseek","calls":4,"input":1000,"output":20,"read":0}],
"history":[{"at":1792340805,"kind":"ask","text":"why"}],"tasks":[]}}"#;
const GUARDIAN: &str = r#"{"status":"watching","score":97,"activeIncidents":2,"secrets":[{"name":"keys.env","mode":"400","ok":true},{"name":"loose.env","mode":"644","ok":false}],"library":{"count":1,"files":["s1.md"]},"errors":[{"service":"tuner","error":"timeout"}]}"#;

fn hermes(p: &str) -> Option<&'static str> {
    Some(match p {
        "/api/hermes/health" => {
            r#"{"ok":true,"ms":2,"bind":"127.0.0.1:8642","loopback":true,"mode":"gateway","skills":[{"id":"a"},{"id":"b"}],"raw":"{\"implementation\":\"hermes-rs\",\"implementation_version\":\"0.1.0\"}"}"#
        }
        "/api/hermes/guard" => r#"{"ok":true,"on":true,"autoAccept":false}"#,
        "/api/hermes/modes" => {
            r#"{"ok":true,"modes":{"plan":"zai-org/GLM-5.3-Flash","action":"zai-org/GLM-5.3-Flash"},"saved":[{"id":"river","name":"River","model":"deepseek","note":"default"}]}"#
        }
        "/api/hermes/models" => {
            r#"{"ok":true,"models":[{"id":"huihui/qwen","name":"Laptop Qwen"}]}"#
        }
        "/api/hermes/history" => {
            r#"{"ok":true,"items":[{"id":"t-1","title":"First <thread>","count":2,"at":1792340805,"source":"one","pinned":false,"tokens":{"total":1500}}],"files":[]}"#
        }
        "/api/hermes/history?id=t-1" => {
            r#"{"ok":true,"tokens":{"input":1,"output":2,"total":3},"items":[{"kind":"user","text":"hello hermes"},{"kind":"tool","name":"terminal","status":"done","command":"uptime","output":"TOOL-OUTPUT"},{"kind":"assistant","text":"hi there"}]}"#
        }
        _ => return None,
    })
}

fn fake(log: Log) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || answer(s, log));
        }
    });
    addr
}

fn answer(mut s: TcpStream, log: Log) {
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
                .find_map(|l| l.strip_prefix("Content-Length: "))
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
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("Cookie: "))
        .unwrap_or("")
        .to_string();
    log.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        head: head.clone(),
        body: body.clone(),
    });
    let paired = cookie.contains("galion_admin=good");
    let (code, out): (u16, String) = match (method.as_str(), path.as_str()) {
        ("GET", "/api/v1/admin") => (200, format!("{{\"ok\":true,\"you\":{paired}}}")),
        ("GET", "/api/v1/health") => (
            200,
            r#"{"status":"healthy","version":"3.8.3-board"}"#.into(),
        ),
        ("GET", "/desk") => (200, DESK.into()),
        ("GET", "/api/v3/guardian/status") if cookie.contains("galion_admin=bad") => {
            (401, r#"{"error":"phone required"}"#.into())
        }
        ("GET", "/api/v3/guardian/status") => (200, GUARDIAN.into()),
        ("POST", "/api/v3/guardian/scan") => (200, r#"{"ok":true}"#.into()),
        ("POST", "/api/v3/guardian/heal") if head.contains("X-Galion-Pin: 4242") => {
            (200, r#"{"ok":true}"#.into())
        }
        ("POST", "/api/v3/guardian/heal") => (403, r#"{"error":"pin"}"#.into()),
        ("POST", "/api/v1/command") => (200, r#"{"ok":true,"data":{"pong":true}}"#.into()),
        ("GET", "/api/v1/ping") => (200, "plain <text>".into()),
        ("GET", p) => match hermes(p) {
            Some(b) => (200, b.into()),
            None => (404, "{}".into()),
        },
        _ => (404, "{}".into()),
    };
    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = s.write_all(resp.as_bytes());
}

fn start(desk: bool) -> (SocketAddr, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let api = fake(Arc::clone(&log));
    let mut args = vec![
        "--ui".to_string(),
        "rust".into(),
        "--upstream".into(),
        api.to_string(),
        "--quiet".into(),
    ];
    if desk {
        args.extend(["--desk".to_string(), api.to_string()]);
    }
    let cfg = Config::from_args(args).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /\n").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    (addr, log)
}

struct Resp {
    code: u16,
    body: String,
}

fn send(addr: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
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
        body: body.to_string(),
    }
}

fn get(a: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        a,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn post(a: SocketAddr, path: &str, extra: &str, body: &str) -> Resp {
    send(
        a,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

const SAME: &str = "Sec-Fetch-Site: same-origin\r\n";

fn posts(log: &Log) -> Vec<Seen> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|s| s.method != "GET")
        .cloned()
        .collect()
}

fn clean(r: &Resp, p: &str) {
    assert!(!r.body.to_ascii_lowercase().contains("<script"), "{p}");
    assert!(
        !r.body.contains("style=\""),
        "{p}: inline style breaks the CSP"
    );
    assert!(!r.body.contains("Not in Rust yet"), "{p}");
}

#[test]
fn four_rooms_are_rust_and_get_only() {
    let (a, log) = start(true);
    for p in [
        "/context",
        "/gateway",
        "/guardian",
        "/hermes",
        "/hermes?id=t-1",
    ] {
        let r = get(a, p, "127.0.0.1", "");
        assert_eq!(r.code, 200, "{p}");
        clean(&r, p);
        assert!(!r.body.contains("did not load"), "{p}: {}", r.body);
    }
    assert!(posts(&log).is_empty(), "plain GETs must not POST upstream");
    for p in ["/context", "/gateway", "/guardian", "/hermes"] {
        assert!(console_ui::PORTED.contains(&p));
    }
}

#[test]
fn context_room() {
    let (a, log) = start(true);
    let r = get(a, "/context", "127.0.0.1", "");
    for n in [
        "4096 live · 65536 window · galion · 8.2 GB",
        "Line up · 4096 context · 8.2 GB",
        "openmind…1018",
        "10-18 18:26 · 3 msgs · api_server",
        "memory &lt;line&gt;",
        "2.5k in · 10 out · 3.0M cache read · 0 reasoning",
        "value=\"100\">100%</progress><span class=\"ell\">terminal",
        "value=\"50\">50%</progress><span class=\"ell\">read_file",
        "7 sessions · 70 messages",
    ] {
        assert!(r.body.contains(n), "missing {n}");
    }
    assert!(log.lock().unwrap().iter().any(|s| s.path == "/desk"));
    // No --desk: the room says the desk is not answering.
    let (b, _) = start(false);
    let r = get(b, "/context", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("The desk foundation is not answering"));
}

#[test]
fn gateway_get_and_fixed_posts() {
    let (a, log) = start(true);
    let r = get(a, "/gateway", "127.0.0.1", "");
    assert!(r.body.contains("Select an endpoint and send."));
    assert_eq!(r.body.matches("href=\"/gateway?ep=").count(), 11);
    // GET send: the server makes the GET and pretty-prints JSON.
    let r = get(a, "/gateway?ep=0&send=1", "127.0.0.1", "");
    assert!(
        r.body
            .contains("{\n  &quot;status&quot;: &quot;healthy&quot;,"),
        "{}",
        r.body
    );
    // Non-JSON answers are shown as text, escaped.
    let r = get(a, "/gateway?ep=5&send=1", "127.0.0.1", "");
    assert!(r.body.contains("plain &lt;text&gt;"));
    // A POST endpoint is never sent by a GET.
    let r = get(a, "/gateway?ep=7&send=1", "127.0.0.1", "");
    assert!(r
        .body
        .contains("POST endpoints are sent with the Send button"));
    assert!(posts(&log).is_empty());
    // Out-of-range picks fall back to the first endpoint.
    assert!(get(a, "/gateway?ep=99", "127.0.0.1", "")
        .body
        .contains("<span class=\"muted mono\">/api/v1/health</span>"));
    // POST without same-origin proof: refused, nothing upstream.
    assert_eq!(post(a, "/gateway", "", "ep=7").code, 403);
    assert_eq!(
        post(a, "/gateway", "Sec-Fetch-Site: cross-site\r\n", "ep=7").code,
        403
    );
    assert_eq!(
        post(a, "/gateway", "Origin: https://evil.example\r\n", "ep=7").code,
        403
    );
    assert!(posts(&log).is_empty());
    // POST of a GET endpoint or a made-up one: 400, nothing upstream.
    assert_eq!(post(a, "/gateway", SAME, "ep=0").code, 400);
    assert_eq!(post(a, "/gateway", SAME, "ep=11").code, 400);
    assert_eq!(
        post(a, "/gateway", SAME, "ep=7&path=/api/v1/admin&body=x").code,
        200
    );
    let p = posts(&log);
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(p[0].path, "/api/v1/command");
    assert_eq!(
        p[0].body, "{\"cmd\":\"ping\"}",
        "body comes from the binary"
    );
    // Same-origin by Origin header works too.
    let r = post(a, "/gateway", "Origin: http://127.0.0.1\r\n", "ep=7");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("&quot;pong&quot;: true"));
    // Other methods are refused.
    let r = send(
        a,
        "PUT /gateway HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.code, 405);
}

#[test]
fn guardian_scan_heal_and_pin() {
    let (a, log) = start(true);
    let r = get(a, "/guardian", "127.0.0.1", "");
    for n in [
        "Watch · 24/7 · watching · score 97 · incidents 2",
        "<span>keys.env</span><span class=\"muted\">400</span>",
        "<span>loose.env</span><span class=\"t-bad\">644</span>",
        "Session library · 1",
        "<span class=\"muted\">timeout</span>",
    ] {
        assert!(r.body.contains(n), "missing {n}");
    }
    // Status refused upstream: said, not hidden.
    let r = get(a, "/guardian", "127.0.0.1", "Cookie: galion_admin=bad\r\n");
    assert!(r.body.contains("HTTP 401 (phone required)"));
    // CSRF.
    assert_eq!(post(a, "/guardian", "", "act=scan").code, 403);
    assert!(posts(&log).is_empty());
    // Scan.
    let r = post(a, "/guardian", SAME, "act=scan");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("scan: HTTP 200"));
    // Heal without a PIN, with a header-injection PIN, unknown act: nothing sent.
    assert_eq!(post(a, "/guardian", SAME, "act=heal").code, 400);
    assert_eq!(
        post(a, "/guardian", SAME, "act=heal&pin=1%0D%0AX-Evil%3A+1").code,
        400
    );
    assert_eq!(post(a, "/guardian", SAME, "act=reboot").code, 400);
    assert_eq!(posts(&log).len(), 1);
    // Heal with the PIN: sent as a header, never echoed.
    let r = post(a, "/guardian", SAME, "act=heal&pin=4242");
    assert!(r.body.contains("heal: HTTP 200"));
    assert!(!r.body.contains("4242"), "PIN must not be rendered");
    let p = posts(&log);
    assert_eq!(p.len(), 2);
    assert_eq!(p[1].path, "/api/v3/guardian/heal");
    assert!(p[1].head.contains("X-Galion-Pin: 4242"));
    assert!(!p[0].head.contains("X-Galion-Pin"), "scan carries no PIN");
    assert_eq!(p[1].body, "{}");
    let r = post(a, "/guardian", SAME, "act=heal&pin=1111");
    assert!(r.body.contains("heal: HTTP 403"));
}

#[test]
fn hermes_read_only() {
    let (a, log) = start(true);
    let r = get(a, "/hermes", "127.0.0.1", "");
    for n in [
        "hermes-rs 0.1.0 · gateway · loopback · 2 ms",
        "zai-org/GLM-5.3-Flash",
        "Laptop Qwen",
        "href=\"/hermes?id=t-1\">First &lt;thread&gt;</a>",
        "2 msgs · one · 1.5k tokens · 10-18 18:26",
        "paid-model decision (H1)",
    ] {
        assert!(r.body.contains(n), "missing {n}");
    }
    assert!(!r.body.contains("127.0.0.1:8642"));
    assert!(!r.body.contains("<form"), "no form on the Hermes room");
    let r = get(a, "/hermes?id=t-1", "127.0.0.1", "");
    assert!(r.body.contains("<pre class=\"say\">hello hermes</pre>"));
    assert!(r.body.contains("<span class=\"subtle mono\">uptime</span>"));
    assert!(!r.body.contains("TOOL-OUTPUT"));
    let before = log.lock().unwrap().len();
    let r = get(a, "/hermes?id=..%2Fadmin", "127.0.0.1", "");
    assert_eq!(r.code, 400);
    assert!(r.body.contains("That thread id is not valid."));
    let after: Vec<Seen> = log.lock().unwrap()[before..].to_vec();
    assert!(after.iter().all(|s| !s.path.contains("id=")), "{after:?}");
    assert!(posts(&log).is_empty(), "Hermes room never POSTs");
}

#[test]
fn rooms_are_gated() {
    let (a, log) = start(true);
    for p in ["/context", "/gateway", "/guardian", "/hermes"] {
        let r = get(a, p, "console.galion.app", "");
        assert!(r.body.contains("This phone"), "{p}: unpaired gets pairing");
        assert!(!r.body.contains("watching") && !r.body.contains("memory &lt;line"));
        let r = get(a, p, "sentialink.com", "");
        assert!(r.body.contains("Soon online."), "{p}");
    }
    // Unpaired POSTs on a console host never reach the action.
    let r = send(
        a,
        "POST /guardian HTTP/1.1\r\nHost: console.galion.app\r\nSec-Fetch-Site: same-origin\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 8\r\nConnection: close\r\n\r\nact=scan",
    );
    assert!(r.body.contains("This phone"));
    assert!(posts(&log).is_empty());
    let seen: Vec<String> = log.lock().unwrap().iter().map(|s| s.path.clone()).collect();
    assert!(seen.iter().all(|p| p == "/api/v1/admin"), "{seen:?}");
    // Paired: the page renders and the cookie is forwarded upstream.
    let r = get(
        a,
        "/guardian",
        "console.galion.app",
        "Cookie: galion_admin=good\r\n",
    );
    assert!(r.body.contains("watching"));
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.path == "/api/v3/guardian/status" && s.head.contains("galion_admin=good")));
}

#[test]
fn spa_fallback_keeps_hermes() {
    let ui = console_ui::Ui::new(console_ui::UiConfig {
        upstream: "127.0.0.1:9".parse().unwrap(),
        pulse: None,
        extra_hosts: vec![],
        desk: None,
    });
    assert!(!ui.owns("/hermes", true), "SPA keeps the full Hermes room");
    assert!(ui.owns("/hermes", false));
    for p in ["/context", "/gateway", "/guardian"] {
        assert!(ui.owns(p, true), "{p}");
    }
}

#[test]
fn every_class_is_styled() {
    let (a, _) = start(true);
    let defined = console_ui::css::defined_classes();
    let mut missing = Vec::new();
    for p in [
        "/context",
        "/gateway?ep=7",
        "/guardian",
        "/hermes?id=t-1",
        "/hermes?id=bad%20id",
    ] {
        let r = get(a, p, "127.0.0.1", "Cookie: galion_admin=bad\r\n");
        let main = r.body.split("<main").nth(1).unwrap_or("").to_string();
        for cap in main.split("class=\"").skip(1) {
            for c in cap.split('"').next().unwrap_or("").split_whitespace() {
                if !defined.iter().any(|d| d == c)
                    && !["primary", "ghost", "online", "offline", "held"].contains(&c)
                {
                    missing.push(format!("{p}: {c}"));
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "unstyled: {missing:?}");
}
