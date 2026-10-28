// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for wave 4 slot 336 (console-dist-arena-bank, D-321-1):
//! `--dist` (the live :8080 shape, self-upstream local API) now has a source
//! for `/api/arena` and `/api/v1/bank`:
//!
//! * `--arena-root DIR`: in-process read-only port of `arena.py status`;
//! * `--data-upstream ADDR`: exact-path GET/HEAD proxy for the data routes
//!   the local API does not answer (behind the same API gate).
//!
//! Starts the real release `galion-console-static` on the box, ports
//! 22060-22069, against temp dirs and a fake bank reader; checks the V2
//! `/arena` and `/bank` pages render data (not the empty state), that
//! non-GET never reaches a reader, that the gate runs before the proxy and
//! that unrelated `/api/*` paths are not proxied.
//!
//! Optional live leg (`CONSOLE_ARENA_LIVE=1`, `CONSOLE_ARENA_VPS=host`,
//! default `slavik-vps`): READ-ONLY. `ssh cat` of the live arena board
//! (`/workspace/arena/board.jsonl`, no secrets) into a temp root, and an
//! `ssh -L 22069 -> VPS 127.0.0.1:18080` tunnel (live Rust console primary)
//! used as `--data-upstream`; GETs only, nothing on the VPS changes.
//!
//! `GALION_REAL=1 cargo test --release -p console-static --test real_dist_arena_bank -- --nocapture --test-threads=1`

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
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

fn req(addr: &str, method: &str, path: &str, host: &str) -> (u16, String, String, u128) {
    let body = if method == "POST" {
        r#"{"action":"generation"}"#
    } else {
        ""
    };
    let len = if method == "POST" {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )
    } else {
        String::new()
    };
    send(
        addr,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{len}Connection: close\r\n\r\n{body}"
        ),
    )
}

fn get(addr: &str, path: &str) -> (u16, String, String, u128) {
    req(addr, "GET", path, "127.0.0.1")
}

const BANK: &str = r#"{"purses":[{"name":"OpenRouter","usd":9.5,"measured":true,"note":"key limit left"},{"name":"River","usd":null,"measured":false,"note":"balance not published"}],"totalUsd":9.5,"weekTokensIn":1000000,"weekTokensOut":20000,"weekCostUsd":1.4,"dailyUsd":0.2,"daysLeft":47.5,"riverIfMovedDailyUsd":0.1,"note":"slot336 fake reader"}"#;

/// Fake data reader: `/api/v1/bank` + `/api/arena` 200, everything else 404.
/// Logs every request line it sees.
fn fake_reader(addr: &'static str, log: Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind(addr).expect("bind fake reader");
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let target = line.split(' ').nth(1).unwrap_or("");
            let path = target.split('?').next().unwrap_or("");
            let (code, body) = if line.starts_with("GET ") && path == "/api/v1/bank" {
                (200, BANK)
            } else if line.starts_with("GET ") && path == "/api/arena" {
                (
                    200,
                    r#"{"ok":true,"stopped":false,"spend":{"cap_usd":2,"spent_usd":0},"champion":null,"rows":[{"id":"up-7","generation":1,"score":5}]}"#,
                )
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

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-336-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A console-dist tree (live layout: DIR/static with one asset).
fn dist_dir(tag: &str) -> PathBuf {
    let d = tmp(&format!("dist-{tag}"));
    std::fs::create_dir_all(d.join("static/assets")).unwrap();
    std::fs::write(d.join("static/assets/app.css"), "body{}").unwrap();
    d
}

fn start(bind: &str, dist: &Path, extra: &[&str]) -> Kill {
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args(["--ui", "rust", "--bind", bind, "--quiet", "--dist"])
        .arg(dist)
        .args(extra)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn console");
    let g = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(bind).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "console did not start"
        );
        thread::sleep(Duration::from_millis(50));
    }
    g
}

fn rust_page(name: &str, r: &(u16, String, String, u128)) {
    assert_eq!(r.0, 200, "{name}: {}", r.1);
    assert!(
        r.1.to_ascii_lowercase().contains("content-type: text/html"),
        "{name}"
    );
    assert!(!r.2.contains("<script"), "{name}: script");
    assert!(!r.2.contains("<form"), "{name}: form");
}

fn arena_root(tag: &str, board: &str) -> PathBuf {
    let d = tmp(&format!("arena-{tag}"));
    std::fs::create_dir_all(d.join("champions")).unwrap();
    std::fs::write(d.join("board.jsonl"), board).unwrap();
    d
}

#[test]
fn real_dist_local_arena_reader() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const BIND: &str = "127.0.0.1:22060";
    let dist = dist_dir("local");
    let mut board = String::new();
    for i in 0..20 {
        board.push_str(&format!(
            "{{\"id\": \"g{}-{}\", \"generation\": {}, \"voice\": \"short\", \"skill\": \"none\", \"tools\": [\"read\"], \"mutation\": null, \"score\": {}, \"mode\": \"dry\"}}\n",
            i / 4 + 1,
            i % 4,
            i / 4 + 1,
            60 + i
        ));
    }
    board.push_str("\nnot json\n");
    let root = arena_root("local", &board);
    std::fs::write(
        root.join("champions/current.json"),
        r#"{"id": "g5-3", "voice": "short", "skill": "none", "score": 79}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("spend.json"),
        r#"{"cap_usd": 2.0, "spent_usd": 0.42, "generations": 2, "max_generations": 3}"#,
    )
    .unwrap();
    let rs = root.to_str().unwrap().to_string();
    let _g = start(BIND, &dist, &["--arena-root", &rs]);

    let (c, h, b, ms) = get(BIND, "/api/arena");
    assert_eq!(c, 200, "{h}");
    assert!(h.contains("application/json") && h.contains("no-store"));
    for k in [
        r#""ok":true"#,
        r#""stopped":false"#,
        r#""spent_usd":0.42"#,
        r#""champion":{"id":"g5-3""#,
        r#""hermes":"intern""#,
    ] {
        assert!(b.contains(k), "{k} in {b}");
    }
    let ids = b.matches(r#"{"id":"g"#).count();
    // 22 lines -> last 16 -> 14 JSON rows (+1 champion id).
    assert_eq!(ids, 15, "{b}");
    println!(
        "local GET /api/arena -> {c} {} bytes {ms} ms, 14 rows",
        b.len()
    );

    let r = get(BIND, "/arena");
    rust_page("/arena", &r);
    assert!(!r.2.contains("no /api/arena route yet"));
    assert!(r.2.contains("<h2>Champion</h2>") && r.2.contains("g5-3"));
    assert!(
        r.2.contains("g2-2") && !r.2.contains("g1-0"),
        "last 16 lines only"
    );
    println!("local GET /arena -> {} {} bytes {} ms", r.0, r.2.len(), r.3);

    // Actions are held: POST/DELETE 405, board untouched.
    for m in ["POST", "DELETE", "PUT"] {
        let (c, _, b, _) = req(BIND, m, "/api/arena", "127.0.0.1");
        assert_eq!(c, 405, "{m}");
        assert!(b.contains("held"), "{m}: {b}");
    }
    assert_eq!(
        std::fs::read_to_string(root.join("board.jsonl")).unwrap(),
        board
    );
    let r = req(BIND, "POST", "/arena", "127.0.0.1");
    assert_eq!(r.0, 405, "room POST");

    // Gate before the reader.
    assert_eq!(req(BIND, "GET", "/api/arena", "evil.example").0, 401);
    assert_eq!(req(BIND, "GET", "/api/arena", "console.galion.app").0, 401);
    // Remote console host page: sign-in note, no board leak.
    let r = req(BIND, "GET", "/arena", "console.galion.app");
    assert!(!r.2.contains("g5-3"), "board leaked to unpaired host");

    // STOP file + broken spend -> stopped, default spend (as the script).
    std::fs::write(root.join("STOP"), "").unwrap();
    std::fs::write(root.join("spend.json"), "{broken").unwrap();
    let (_, _, b, _) = get(BIND, "/api/arena");
    assert!(b.contains(r#""stopped":true"#));
    assert!(
        b.contains(r#""spend":{"cap_usd":2,"spent_usd":0,"generations":0,"max_generations":3}"#)
    );
    // Unreadable (not UTF-8) -> 500 without the path.
    std::fs::write(root.join("spend.json"), [0xffu8, 0xfe]).unwrap();
    let (c, _, b, _) = get(BIND, "/api/arena");
    assert_eq!(c, 500);
    assert!(!b.contains(&rs), "path leaked");
    let r = get(BIND, "/arena");
    rust_page("/arena 500", &r);
    assert!(r.2.contains("HTTP 500"), "page explains the 500");

    // Bank without a reader: unchanged honest empty state.
    let (c, _, _, _) = get(BIND, "/api/v1/bank");
    assert_eq!(c, 404);
    let r = get(BIND, "/bank");
    rust_page("/bank", &r);
    assert!(r.2.contains("no /api/v1/bank route yet"));

    // Load: 200 page renders, worst-case latency.
    let mut worst = 0;
    for i in 0..200 {
        let r = get(BIND, if i % 2 == 0 { "/arena" } else { "/api/arena" });
        assert!(r.0 == 200 || r.0 == 500);
        worst = worst.max(r.3);
    }
    println!("local 200 requests, worst {worst} ms");
    assert!(worst < 2000);
    let _ = std::fs::remove_dir_all(&dist);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn real_dist_data_upstream_proxy() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const BIND: &str = "127.0.0.1:22061";
    const READER: &str = "127.0.0.1:22062";
    let log = Arc::new(Mutex::new(Vec::new()));
    fake_reader(READER, Arc::clone(&log));
    let dist = dist_dir("proxy");
    let _g = start(BIND, &dist, &["--data-upstream", READER]);

    let (c, _, b, ms) = get(BIND, "/api/v1/bank");
    assert_eq!(c, 200);
    assert_eq!(b, BANK);
    println!("proxy GET /api/v1/bank -> {c} {} bytes {ms} ms", b.len());
    let rb = get(BIND, "/bank");
    let r = &rb;
    rust_page("/bank", r);
    assert!(
        r.2.contains("OpenRouter") && r.2.contains("$9.50"),
        "bank rendered"
    );
    assert!(!r.2.contains("no /api/v1/bank route yet"));
    let ra = get(BIND, "/arena");
    rust_page("/arena", &ra);
    assert!(ra.2.contains("up-7"), "arena via reader");
    println!(
        "proxy GET /bank -> {} {} bytes, /arena -> {} {} bytes",
        rb.0,
        rb.2.len(),
        ra.0,
        ra.2.len()
    );

    let before = log.lock().unwrap().len();
    // Not proxied: non-GET, foreign host, unpaired remote host, other paths,
    // prefix tricks.
    for (m, p, host) in [
        ("POST", "/api/v1/bank", "127.0.0.1"),
        ("POST", "/api/arena", "127.0.0.1"),
        ("DELETE", "/api/v1/bank", "127.0.0.1"),
        ("GET", "/api/v1/bank", "evil.example"),
        ("GET", "/api/v1/bank", "console.galion.app"),
        ("GET", "/api/v1/bankx", "127.0.0.1"),
        ("GET", "/api/v1/bank/../keys", "127.0.0.1"),
        ("GET", "/api/v1/keys", "127.0.0.1"),
        ("GET", "/api/arena/x", "127.0.0.1"),
    ] {
        let (c, _, _, _) = req(BIND, m, p, host);
        assert!(matches!(c, 401 | 404 | 405), "{m} {p} {host} -> {c}");
    }
    assert_eq!(
        log.lock().unwrap().len(),
        before,
        "reader saw: {:?}",
        log.lock().unwrap()
    );
    // Query string still routes (path is exact, query passed through).
    assert_eq!(get(BIND, "/api/v1/bank?fresh=1").0, 200);
    assert!(log
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .starts_with("GET /api/v1/bank?fresh=1 "));
    let seen = log.lock().unwrap().clone();
    assert!(seen.iter().all(|l| l.starts_with("GET ")), "{seen:?}");
    println!("reader saw {} requests, all GET", seen.len());
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn real_dist_arena_live_readonly() {
    if !real() || std::env::var("CONSOLE_ARENA_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 CONSOLE_ARENA_LIVE=1 [CONSOLE_ARENA_VPS=host]");
        return;
    }
    const BIND: &str = "127.0.0.1:22068";
    const UP: &str = "127.0.0.1:22069";
    let vps = std::env::var("CONSOLE_ARENA_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let ssh = |args: &[&str]| {
        Command::new("ssh")
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=15"])
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("ssh")
    };
    // 1) Live board copy (read-only cat).
    let out = ssh(&[&vps, "cat /workspace/arena/board.jsonl"]);
    assert!(out.status.success(), "ssh cat failed");
    let board = String::from_utf8(out.stdout).expect("utf8 board");
    let live_lines: Vec<&str> = board.lines().collect();
    let tail = &live_lines[live_lines.len().saturating_sub(16)..];
    let want: Vec<String> = tail
        .iter()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let i = l.find("\"id\": \"")? + 7;
            Some(l[i..].split('"').next()?.to_string())
        })
        .collect();
    println!(
        "live board.jsonl: {} lines, {} ids in tail",
        live_lines.len(),
        want.len()
    );
    let root = arena_root("live", &board);
    let rs = root.to_str().unwrap().to_string();

    // 2) Tunnel to the live Rust console primary for the data upstream.
    let tunnel = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes"])
        .args(["-N", "-L", &format!("{UP}:127.0.0.1:18080"), &vps])
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh -L");
    let _t = Kill(tunnel);
    let t0 = Instant::now();
    while TcpStream::connect(UP).is_err() || get(UP, "/api/v1/health").0 != 200 {
        assert!(
            t0.elapsed() < Duration::from_secs(25),
            "tunnel did not come up"
        );
        thread::sleep(Duration::from_millis(200));
    }
    let (la, _, lab, _) = get(UP, "/api/arena");
    let (lb, _, lbb, _) = get(UP, "/api/v1/bank");
    println!("live :18080 /api/arena -> {la} {:?}", lab.trim());
    println!("live :18080 /api/v1/bank -> {lb} {:?}", lbb.trim());

    let dist = dist_dir("live");
    let _g = start(BIND, &dist, &["--arena-root", &rs, "--data-upstream", UP]);
    // Local reader wins for /api/arena.
    let (c, _, b, ms) = get(BIND, "/api/arena");
    assert_eq!(c, 200);
    for id in &want {
        assert!(b.contains(&format!("\"id\":\"{id}\"")), "{id} missing");
    }
    assert!(
        b.contains(r#""spend":{"cap_usd":2"#),
        "default spend (no live spend.json)"
    );
    println!(
        "box /api/arena from live board -> {c} {} bytes {ms} ms, ids {}/{}",
        b.len(),
        want.len(),
        want.len()
    );
    let r = get(BIND, "/arena");
    rust_page("/arena", &r);
    for id in &want {
        assert!(r.2.contains(id.as_str()), "page missing {id}");
    }
    println!(
        "box /arena from live board -> {} {} bytes {} ms",
        r.0,
        r.2.len(),
        r.3
    );
    // Bank goes to the live primary: same status as live.
    let (c, _, b, _) = get(BIND, "/api/v1/bank");
    assert_eq!(c, lb, "passthrough status");
    assert_eq!(b.trim(), lbb.trim(), "passthrough body");
    let r = get(BIND, "/bank");
    rust_page("/bank", &r);
    if lb == 404 {
        assert!(r.2.contains("no /api/v1/bank route yet"));
    }
    println!(
        "box /api/v1/bank via live :18080 -> {c}; /bank {} bytes {} ms",
        r.2.len(),
        r.3
    );
    let _ = std::fs::remove_dir_all(&dist);
    let _ = std::fs::remove_dir_all(&root);
}
