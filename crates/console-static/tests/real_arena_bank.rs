// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for wave 4 slot 299 (console-arena-bank-rust): `/arena`
//! and `/bank` are Rust-first even when an SPA fallback (`--root` with an
//! HTML shell) is configured. Starts the real release `galion-console-static`
//! binary on the box (ports 21690-21699) twice against fake console APIs:
//!
//! * "full": `/api/arena` + `/api/v1/bank` answer 200 with JSON;
//! * "bare": both answer 404 (what the live Rust `--local-api` on the VPS
//!   answered on 2026-10-27 20:41 CET +01:00).
//!
//! It checks the SPA shell is never served for the two rooms, that no
//! `<script>` or `<form>` reaches the page, that POST is refused without
//! reaching the API, that only GETs hit the upstream, and that the SPA still
//! serves its own unported routes.
//!
//! Optional live leg (`CONSOLE_ARENA_LIVE=1 CONSOLE_ARENA_VPS=user@host`):
//! read-only `ssh -L 21699 -> VPS 127.0.0.1:18080` (the live Rust console
//! primary with `--local-api`), then the box binary renders `/arena` and
//! `/bank` from that real upstream (GETs only, nothing on the VPS changes).
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_arena_bank -- --nocapture --test-threads=1`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const SHELL_MARK: &str = "SPA-SHELL-299";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn send(addr: &str, raw: &str) -> (u16, String, String, u128) {
    let t = Instant::now();
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let ms = t.elapsed().as_millis();
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, head.to_string(), body.to_string(), ms)
}

fn get(addr: &str, path: &str) -> (u16, String, String, u128) {
    send(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    )
}

const ARENA: &str = r#"{"ok":true,"stopped":false,"spend":{"cap_usd":2,"spent_usd":0.37,"generations":2,"max_generations":3},"champion":{"id":"s0","voice":"plain","skill":"triage","score":6},"rows":[{"id":"s0","generation":1,"voice":"plain","skill":"triage","mutation":null,"mode":"dry","score":6},{"id":"s1","generation":2,"voice":"terse","skill":"triage","mutation":"voice","mode":"ran","score":7,"answer":"ok <b>"}]}"#;
const BANK: &str = r#"{"purses":[{"name":"OpenRouter","usd":9.5,"measured":true,"note":"key limit left"}],"totalUsd":9.5,"weekTokensIn":1000000,"weekTokensOut":20000,"weekCostUsd":1.4,"dailyUsd":0.2,"daysLeft":47.5,"riverIfMovedDailyUsd":0.1,"note":"Bearer abc123 hidden"}"#;

/// Fake console API. `full` serves arena + bank; otherwise both are 404.
fn fake_upstream(addr: &'static str, full: bool, log: Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind(addr).expect("bind upstream");
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let (code, body) = if line.starts_with("GET /api/v1/health ") {
                (200, r#"{"status":"healthy","version":"299"}"#)
            } else if full && line.starts_with("GET /api/arena ") {
                (200, ARENA)
            } else if full && line.starts_with("GET /api/v1/bank ") {
                (200, BANK)
            } else {
                (404, r#"{"error":"not found"}"#)
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
}

/// A minimal pre-built SPA: shell + one asset + routes covering the rooms.
fn spa_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-arena-bank-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("assets")).unwrap();
    std::fs::write(
        d.join("_shell.html"),
        format!("<!doctype html><title>{SHELL_MARK}</title><script type=module src=/assets/app.js></script>"),
    )
    .unwrap();
    std::fs::write(d.join("assets/app.js"), "console.log(1)").unwrap();
    std::fs::write(
        d.join("routes.txt"),
        "page /arena\npage /bank\npage /analytics\npage /spa-only\n",
    )
    .unwrap();
    d
}

fn start(bind: &str, upstream: &str, spa: &PathBuf) -> Kill {
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args([
            "--ui",
            "rust",
            "--bind",
            bind,
            "--upstream",
            upstream,
            "--quiet",
            "--root",
        ])
        .arg(spa)
        .arg("--routes")
        .arg(spa.join("routes.txt"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let k = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(bind).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        thread::sleep(Duration::from_millis(50));
    }
    k
}

fn rust_page(path: &str, r: &(u16, String, String, u128)) {
    let (code, head, body, _) = r;
    assert_eq!(*code, 200, "{path}: {head}");
    assert!(
        head.to_ascii_lowercase().contains("text/html"),
        "{path}: {head}"
    );
    assert!(
        !body.contains(SHELL_MARK),
        "{path}: SPA shell served, not Rust"
    );
    let low = body.to_ascii_lowercase();
    assert!(!low.contains("<script"), "{path}: script");
    assert!(!low.contains("<form"), "{path}: form");
    assert!(body.contains("/_ui/"), "{path}: Rust chrome missing");
}

#[test]
fn real_full_api_rust_first_with_spa() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const BIND: &str = "127.0.0.1:21690";
    const UP: &str = "127.0.0.1:21691";
    let log = Arc::new(Mutex::new(Vec::new()));
    fake_upstream(UP, true, Arc::clone(&log));
    let spa = spa_dir("full");
    let _g = start(BIND, UP, &spa);

    let r = get(BIND, "/arena");
    rust_page("/arena", &r);
    let b = &r.2;
    assert!(b.contains("<title>Galion Console · Arena</title>"));
    assert!(b.contains("Read-only board"));
    assert!(b.contains("2/3 generations · $0.37 / $2"));
    assert!(
        b.find(">s1<").unwrap() < b.find(">s0<").unwrap(),
        "newest first"
    );
    assert!(b.contains("ok &lt;b&gt;"), "answer escaped");
    assert!(!b.contains("TypeScript"));
    println!("full GET /arena -> {} {} bytes {} ms", r.0, b.len(), r.3);

    let r = get(BIND, "/bank");
    rust_page("/bank", &r);
    let b = &r.2;
    assert!(b.contains("$9.50</span><span>OpenRouter</span>"));
    assert!(b.contains("47.5</span><span>days left"));
    assert!(!b.contains("abc123"), "bearer scrubbed");
    assert!(!b.contains("route yet"));
    println!("full GET /bank -> {} {} bytes {} ms", r.0, b.len(), r.3);

    // POSTs are refused by Rust and never reach the API.
    let before = log.lock().unwrap().len();
    for (p, body) in [("/arena", "action=generation"), ("/bank", "x=1")] {
        let raw = format!(
            "POST {p} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://127.0.0.1\r\nSec-Fetch-Site: same-origin\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (c, h, _, _) = send(BIND, &raw);
        assert_eq!(c, 405, "POST {p}: {h}");
    }
    assert_eq!(log.lock().unwrap().len(), before, "POST reached upstream");

    // The SPA still serves what Rust does not own.
    let (c, _, b, _) = get(BIND, "/spa-only");
    assert_eq!(c, 200);
    assert!(
        b.contains(SHELL_MARK),
        "SPA fallback still serves its routes"
    );
    let calls = log.lock().unwrap().clone();
    assert!(calls.iter().all(|l| l.starts_with("GET ")), "{calls:?}");
    assert!(calls.iter().any(|l| l.starts_with("GET /api/arena ")));
    assert!(calls.iter().any(|l| l.starts_with("GET /api/v1/bank ")));
    println!("upstream calls: {}", calls.len());
    let _ = std::fs::remove_dir_all(&spa);
}

#[test]
fn real_bare_api_like_live_404() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const BIND: &str = "127.0.0.1:21692";
    const UP: &str = "127.0.0.1:21693";
    let log = Arc::new(Mutex::new(Vec::new()));
    fake_upstream(UP, false, Arc::clone(&log));
    let spa = spa_dir("bare");
    let _g = start(BIND, UP, &spa);

    let r = get(BIND, "/arena");
    rust_page("/arena", &r);
    assert!(
        r.2.contains("no /api/arena route yet"),
        "arena 404 explained"
    );
    assert!(!r.2.contains("<h2>Board</h2>"));
    println!("bare GET /arena -> {} {} bytes {} ms", r.0, r.2.len(), r.3);

    let r = get(BIND, "/bank");
    rust_page("/bank", &r);
    assert!(r.2.contains("no /api/v1/bank route yet"));
    assert!(r.2.contains("Rust bank reader"));
    println!("bare GET /bank -> {} {} bytes {} ms", r.0, r.2.len(), r.3);

    // Latency: 20 rounds of each room against the bare API.
    let mut worst = 0u128;
    for _ in 0..20 {
        for p in ["/arena", "/bank"] {
            let r = get(BIND, p);
            assert_eq!(r.0, 200);
            worst = worst.max(r.3);
        }
    }
    println!("bare 40 renders, worst {worst} ms");
    assert!(worst < 2000, "render too slow: {worst} ms");
    let _ = std::fs::remove_dir_all(&spa);
}

#[test]
fn real_live_vps_upstream_readonly() {
    if !real() || std::env::var("CONSOLE_ARENA_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 CONSOLE_ARENA_LIVE=1 CONSOLE_ARENA_VPS=user@host");
        return;
    }
    const BIND: &str = "127.0.0.1:21698";
    const UP: &str = "127.0.0.1:21699";
    let vps = std::env::var("CONSOLE_ARENA_VPS").expect("CONSOLE_ARENA_VPS=user@host");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let tunnel = Command::new("ssh")
        .args(["-i", &format!("{home}/.ssh/id_ed25519_vps")])
        .args([
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
        ])
        .args(["-N", "-L", &format!("{UP}:127.0.0.1:18080"), &vps])
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh");
    let _t = Kill(tunnel);
    let t0 = Instant::now();
    while TcpStream::connect(UP).is_err() || get(UP, "/api/v1/health").0 != 200 {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "tunnel did not come up"
        );
        thread::sleep(Duration::from_millis(200));
    }
    let (ac, _, _, _) = get(UP, "/api/arena");
    let (bc, _, _, _) = get(UP, "/api/v1/bank");
    println!("live upstream: /api/arena -> {ac}, /api/v1/bank -> {bc}");

    let spa = spa_dir("live");
    let _g = start(BIND, UP, &spa);
    let r = get(BIND, "/arena");
    rust_page("/arena", &r);
    if ac == 404 {
        assert!(r.2.contains("no /api/arena route yet"));
    } else if ac == 200 {
        assert!(r.2.contains("<h2>Board</h2>"));
    }
    println!("live GET /arena -> {} {} bytes {} ms", r.0, r.2.len(), r.3);
    let r = get(BIND, "/bank");
    rust_page("/bank", &r);
    if bc == 404 {
        assert!(r.2.contains("no /api/v1/bank route yet"));
    }
    println!("live GET /bank -> {} {} bytes {} ms", r.0, r.2.len(), r.3);
    let _ = std::fs::remove_dir_all(&spa);
}
