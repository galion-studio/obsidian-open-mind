// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for console V2 sprite + cached health (wave 4 slot 276):
//! starts the real `galion-console-static` binary with `--ui rust` on the box
//! (ports 21460-21469), hits `/services`, `/_ui/i.*.svg`, and a burst of
//! chrome pages against a counting fake upstream, then tears down.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_sprite_health -- --nocapture`.
//! No VPS contact. No live console.service change.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:21460";
const UPSTREAM: &str = "127.0.0.1:21461";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(path: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(BIND).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, head.to_string(), body.to_string())
}

fn fake_upstream(hits: Arc<Mutex<u32>>) {
    let l = TcpListener::bind(UPSTREAM).expect("bind upstream");
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let req = String::from_utf8_lossy(&buf);
            if req.contains("GET /api/v1/health ") {
                *hits.lock().unwrap() += 1;
                let body =
                    r#"{"status":"healthy","version":"sprite-health","target":{"podId":"box"}}"#;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else {
                let body = "{}";
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        }
    });
}

#[test]
fn real_sprite_and_cached_health() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let hits = Arc::new(Mutex::new(0u32));
    fake_upstream(Arc::clone(&hits));

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args([
            "--ui",
            "rust",
            "--upstream",
            UPSTREAM,
            "--bind",
            BIND,
            "--quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let _guard = Kill(child);
    let t0 = Instant::now();

    while TcpStream::connect(BIND).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // Cached health: a burst of chrome pages → one upstream /api/v1/health.
    let (c, _, _) = get("/console");
    assert_eq!(c, 200);
    let after_first = *hits.lock().unwrap();
    assert_eq!(after_first, 1, "first /console fills the cache");
    for p in ["/data", "/agents", "/console", "/gateway", "/memory"] {
        for _ in 0..3 {
            let (c, _, _) = get(p);
            assert_eq!(c, 200, "{p}");
        }
    }
    let total = *hits.lock().unwrap();
    assert_eq!(
        total, 1,
        "burst of chrome pages must reuse cached health, hits={total}"
    );
    println!("health upstream hits after burst: {total} (want 1)");

    let t = Instant::now();
    let (code, head, body) = get("/services");
    let ms = t.elapsed().as_millis();
    assert_eq!(code, 200, "{head}");
    assert!(!body.to_ascii_lowercase().contains("<script"));
    assert!(body.contains("<use href=\"/_ui/i."), "sprite use missing");
    assert!(
        !body.contains("<path d=\"M6 6h12v4H6z"),
        "path still inlined"
    );
    let href = body
        .split("<use href=\"")
        .nth(1)
        .unwrap()
        .split('#')
        .next()
        .unwrap()
        .to_string();
    println!(
        "GET /services -> {code}, {} bytes, {ms} ms; sprite {href}",
        body.len()
    );

    let (c, h, spr) = get(&href);
    assert_eq!(c, 200, "{h}");
    assert!(h.contains("immutable"), "{h}");
    assert!(h.contains("image/svg+xml"), "{h}");
    let symbols = spr.matches("<symbol ").count();
    assert!(symbols >= 40, "symbols={symbols}");
    println!(
        "GET {href} -> {c}, {} bytes, {symbols} symbols, immutable",
        spr.len()
    );
}
