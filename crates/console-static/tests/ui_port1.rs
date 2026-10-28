// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end tests for the console-port-1 rooms (`/analytics`, `/arena`,
//! `/bank`, `/cache`): the real server with a fake console API, a fake
//! galion-desk, raw HTTP/1.1 over loopback (ephemeral ports). Kept apart
//! from `ui.rs` so the other port slots do not collide in one file.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const HEALTH: &str = r#"{"status":"healthy","version":"3.8.3-board","target":{"pod":"ip50"}}"#;
const ANALYTICS: &str = r#"{"at":"2026-10-21T05:00:00+02:00",
 "health":{"score":97,"status":"healthy","anomalies":0,"services":{"online":2,"offline":0}},
 "infrastructure":{"cpu":7,"load":0.3,"cores":8,"memory":44,"disk":{"pct":31,"used":33285996544,"total":107374182400},"gpu":null},
 "services":[{"id":"console","port":8080,"ok":true,"ms":2},{"id":"hermes","port":8642,"ok":true,"ms":9}],
 "agents":{"turns":3,"lastAt":null,"hermes":{"last":[210,190],"avg":200}},
 "users":{"phones":1},"workforce":{"zone":"fridge","live":"Hermes"},
 "edge":[{"host":"galion.app","code":200,"ms":71}],
 "systems":[{"id":"nerve","ok":true,"note":"<fresh>"}],
 "nerve":{"events":["guard: quiet"]}}"#;
const ARENA: &str = r#"{"ok":true,"stopped":false,"note":"Promote does not edit SOUL.md","hermes":"intern",
 "spend":{"cap_usd":2,"spent_usd":0.37,"generations":2,"max_generations":3},"champion":null,
 "rows":[{"id":"s0","generation":1,"voice":"plain","skill":"triage","tools":[],"mutation":null,"mode":"dry","score":3},
         {"id":"s1","generation":2,"voice":"terse","skill":"triage","tools":[],"mutation":"voice","mode":"ran","score":5,"answer":"ok"}]}"#;
const DESK: &str = r#"{"at":1,"mindpack":{"cache":{"read":109107266,"write":1193851,"input":19391995,"output":1830315,"reasoning":789359,"files":1142,"file_bytes":16731597},
 "cached":[{"id":"galion-one","model":"qwen3.5:9b","turns":8}],
 "stores":[{"name":"scratch","n":1019,"bytes":12629743}],
 "models":[{"name":"m1","calls":3,"input":1500,"output":20,"read":2000000,"reasoning":0}],
 "recent":[{"name":"a.txt","bytes":2048,"at":1792551000}]}}"#;

type Log = Arc<Mutex<Vec<String>>>;

fn fake(log: Log, bank: bool) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || answer(s, log, bank));
        }
    });
    addr
}

fn answer(mut s: TcpStream, log: Log, bank: bool) {
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head = loop {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break String::from_utf8_lossy(&buf[..p]).to_string();
        }
    };
    let first = head.lines().next().unwrap_or("").to_string();
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("Cookie: "))
        .unwrap_or("-")
        .to_string();
    log.lock().unwrap().push(format!("{first} {cookie}"));
    let mut it = first.split(' ');
    let method = it.next().unwrap_or("");
    let path = it.next().unwrap_or("");
    let paired = cookie.contains("galion_admin=good");
    let (code, out): (u16, String) = match (method, path) {
        ("GET", "/api/v1/admin") => (200, format!("{{\"ok\":true,\"paired\":true,\"you\":{paired}}}")),
        ("GET", "/api/v1/health") => (200, HEALTH.into()),
        ("GET", "/api/v1/analytics") if paired => (200, ANALYTICS.into()),
        ("GET", "/api/v1/analytics") => (401, "{\"error\":\"phone required\"}".into()),
        ("GET", "/api/arena") => (200, ARENA.into()),
        ("GET", "/api/v1/bank") if bank => (
            200,
            r#"{"purses":[{"name":"OpenRouter","usd":9.5,"measured":true,"note":"key limit left Bearer abc123"}],"totalUsd":9.5,"weekTokensIn":1000000,"weekTokensOut":200000,"weekCostUsd":1.4,"dailyUsd":0.2,"daysLeft":47.5,"riverIfMovedDailyUsd":0.03,"note":"ok"}"#.into(),
        ),
        // What the live TanStack console says for a route it lacks (Accept: JSON).
        ("GET", "/api/v1/bank") => (
            500,
            "{\"error\":\"Only HTML requests are supported here\"}".into(),
        ),
        ("GET", "/desk") => (200, DESK.into()),
        _ => (404, "nope".into()),
    };
    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = s.write_all(resp.as_bytes());
}

fn start_with(bank: bool, desk: bool) -> (SocketAddr, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let api = fake(Arc::clone(&log), bank);
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--upstream".into(),
        api.to_string(),
        "--quiet".into(),
    ];
    if desk {
        args.push("--desk".into());
        args.push(api.to_string());
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
    head: String,
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
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn get(addr: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn clean_page(p: &str, r: &Resp) {
    assert_eq!(r.code, 200, "{p}: {}", r.head);
    assert!(!r.body.to_ascii_lowercase().contains("<script"), "{p}");
    assert!(!r.body.contains("style=\""), "{p}: inline style");
    assert!(!r.body.contains("Not in Rust yet"), "{p}");
    assert!(
        r.head
            .contains("Content-Security-Policy: default-src 'none'"),
        "{p}"
    );
}

const PAIRED: &str = "Cookie: galion_admin=good\r\n";

#[test]
fn analytics_boards_and_refusal() {
    let (a, log) = start_with(false, false);
    // Loopback without the phone cookie: the API says 401, the page says why.
    let r = get(a, "/analytics", "127.0.0.1", "");
    clean_page("/analytics", &r);
    assert!(r.body.contains("<title>Galion Console · Nerve</title>"));
    assert!(r.body.contains("The analytics API did not answer."));
    assert!(r.body.contains("paired phones only"));
    // The phone cookie is forwarded and the boards render.
    let r = get(a, "/analytics", "127.0.0.1", PAIRED);
    clean_page("/analytics", &r);
    assert!(r
        .body
        .contains("<span>Health</span><span class=\"muted\">97 · healthy</span>"));
    assert!(r.body.contains("2 up · 0 down"));
    assert!(r.body.contains("200 ms · mean of the last calls"));
    assert!(r.body.contains("<li><span>guard: quiet</span></li>"));
    assert!(r
        .body
        .contains("galion.app</span><span class=\"muted\">200 · 71 ms"));
    assert!(r.body.contains("no checks yet"));
    for (q, needle) in [
        (
            "systems",
            "nerve</span><span class=\"muted\">up · &lt;fresh&gt;",
        ),
        ("infrastructure", "31% · 31.0 GB of the 100 GB quota"),
        (
            "services",
            "hermes</span><span class=\"muted\">:8642 · up · 9 ms",
        ),
        ("agents", "3 · voice turns kept · none yet"),
        ("users", "Paired phones</span><span class=\"muted\">1"),
        ("workforce", "The live agent on this pod is Hermes."),
        ("bogus", "aria-current=\"page\">Overview</a>"),
    ] {
        let r = get(a, &format!("/analytics?board={q}"), "127.0.0.1", PAIRED);
        clean_page(q, &r);
        assert!(r.body.contains(needle), "{q}: {needle}");
    }
    let calls = log.lock().unwrap().clone();
    assert!(calls.iter().all(|l| l.starts_with("GET ")), "{calls:?}");
    assert!(calls
        .iter()
        .any(|l| l.starts_with("GET /api/v1/analytics ") && l.contains("galion_admin=good")));
    // POST is refused before any API call.
    let before = log.lock().unwrap().len();
    let r = send(
        a,
        "POST /analytics HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.code, 405);
    assert_eq!(log.lock().unwrap().len(), before);
}

#[test]
fn arena_reads_and_never_acts() {
    let (a, log) = start_with(false, false);
    let r = get(a, "/arena", "127.0.0.1", "");
    clean_page("/arena", &r);
    assert!(r.body.contains("Private arena · Hermes stays the intern"));
    assert!(r.body.contains("2/3 generations · $0.37 / $2"));
    assert!(r.body.find(">s1<").unwrap() < r.body.find(">s0<").unwrap());
    assert!(r.body.contains("mutated voice · terse · triage"));
    assert!(r.body.contains("elite · plain · triage"));
    assert!(!r.body.contains("<form method=\"post\""));
    for body in ["action=generation", "{\"action\":\"generation\"}"] {
        let raw = format!(
            "POST /arena HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://127.0.0.1\r\nSec-Fetch-Site: same-origin\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(send(a, &raw).code, 405);
    }
    let calls = log.lock().unwrap().clone();
    assert!(calls.iter().all(|l| l.starts_with("GET ")), "{calls:?}");
    assert!(calls.iter().any(|l| l.starts_with("GET /api/arena ")));
}

#[test]
fn bank_without_and_with_an_api() {
    let (a, _) = start_with(false, false);
    let r = get(a, "/bank", "127.0.0.1", "");
    clean_page("/bank", &r);
    assert!(r.body.contains("The cash the stack can measure"));
    assert!(r.body.contains("—</span><span>total"));
    assert!(r.body.contains("no /api/v1/bank route yet"));
    let (b, _) = start_with(true, false);
    let r = get(b, "/bank", "127.0.0.1", "");
    clean_page("/bank", &r);
    assert!(r.body.contains(
        "$9.50</span><span>OpenRouter</span><span class=\"muted\">key limit left</span>"
    ));
    assert!(!r.body.contains("abc123"), "bearer token scrubbed");
    assert!(r.body.contains("1000000</span><span>7-day tokens in"));
    assert!(r.body.contains("47.5</span><span>days left"));
    assert!(!r.body.contains("route yet"));
}

#[test]
fn cache_reads_the_desk() {
    let (a, log) = start_with(false, true);
    let r = get(a, "/cache", "127.0.0.1", "");
    clean_page("/cache", &r);
    assert!(r
        .body
        .contains("109.1M read · 1.2M write · 1142 files · 16.7 MB"));
    assert!(r.body.contains("8 turns in the context cache"));
    assert!(r.body.contains("1019 files · 12.6 MB"));
    assert!(r.body.contains("10-21 04:50</span> a.txt · 2 KB"));
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.starts_with("GET /desk ")));
    // Without --desk: zeros and a note, like the TSX when the desk is down.
    let (b, _) = start_with(false, false);
    let r = get(b, "/cache", "127.0.0.1", "");
    clean_page("/cache", &r);
    assert!(r.body.contains("0 read · 0 write · 0 files · 0"));
    assert!(r.body.contains("did not answer"));
}

#[test]
fn port1_rooms_are_gated() {
    let (a, log) = start_with(true, true);
    for p in ["/analytics", "/arena", "/bank", "/cache"] {
        let r = get(a, p, "console.galion.app", "");
        assert!(r.body.contains("This phone"), "{p}: unpaired gets pairing");
        assert!(
            !r.body.contains("generations") && !r.body.contains("109.1M"),
            "{p}"
        );
        let r = get(a, p, "sentialink.com", "");
        assert!(r.body.contains("Soon online."), "{p}: public host");
    }
    assert!(!log
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.starts_with("GET /api/arena") || l.starts_with("GET /desk")));
    let r = get(a, "/arena", "console.galion.app", PAIRED);
    assert!(r.body.contains("2/3 generations"));
}

#[test]
fn rust_owns_arena_and_bank_even_with_a_spa() {
    let ui = console_ui::Ui::new(console_ui::UiConfig {
        upstream: "127.0.0.1:9".parse().unwrap(),
        pulse: None,
        extra_hosts: Vec::new(),
        desk: None,
        files: Default::default(),
    });
    // Slot 299: /arena and /bank are Rust-first; the SPA keeps no route.
    assert!(console_ui::SPA_FIRST.is_empty());
    for p in ["/analytics", "/arena", "/bank", "/cache"] {
        assert!(ui.owns(p, true) && ui.owns(p, false), "{p}");
        assert!(console_ui::PORTED.contains(&p));
    }
    for p in console_ui::PORTED {
        assert!(
            ui.owns(p, true),
            "{p} must be Rust-first with a SPA fallback"
        );
    }
}
