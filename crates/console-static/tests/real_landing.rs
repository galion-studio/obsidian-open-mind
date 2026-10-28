// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the Desk landing (`/console`, the project overview):
//! starts the real `galion-console-static` binary with `--ui rust` on the
//! box (slot 254 ports 21230-21239), probes it over loopback HTTP and stops
//! it. The API upstream is a closed loopback port on purpose: the overview
//! is static facts and must render with the API down (header dot "down").
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_landing -- --nocapture`.
//! No VPS contact.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:21232";
const DEAD_UPSTREAM: &str = "127.0.0.1:21233";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(path: &str, extra: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(BIND).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}Connection: close\r\n\r\n"
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

#[test]
fn real_landing_overview() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args([
            "--ui",
            "rust",
            "--upstream",
            DEAD_UPSTREAM,
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

    let t = Instant::now();
    let (code, head, body) = get("/console", "");
    let ms = t.elapsed().as_millis();
    assert_eq!(code, 200, "{head}");
    for needle in [
        "Discovering the universe together with humans.",
        "you leave with more than you came in with",
        "Sector map",
        "The automated process",
        "Waiting for Slavik",
        "swarm/hermes-core",
        "45b7980",
        "<td>645</td>",
    ] {
        assert!(body.contains(needle), "missing {needle}");
    }
    assert!(!body.to_ascii_lowercase().contains("<script"));
    assert!(!body.contains("style=\""));
    assert!(head.contains("Content-Security-Policy: default-src 'none'"));
    // API down: header dot says so, page still renders.
    assert!(body.contains("dot offline"));
    println!(
        "GET /console -> {code}, {} bytes, {ms} ms, 0 <script>, {} cards, {} svg",
        body.len(),
        body.matches("class=\"ov-card\"").count(),
        body.matches("<svg class=\"ov-svg\"").count()
    );

    let sheets: Vec<&str> = body
        .split("rel=\"stylesheet\" href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .collect();
    assert_eq!(
        sheets.len(),
        2,
        "base + overview stylesheets on /console: {sheets:?}"
    );
    assert!(sheets[0].contains("/_ui/console."), "{:?}", sheets[0]);
    assert!(sheets[1].contains("/_ui/overview."), "{:?}", sheets[1]);
    let (c, _, css) = get(sheets[0], "");
    assert_eq!(c, 200);
    assert!(css.contains("prefers-color-scheme:light"));
    assert!(
        !css.contains("ov-card"),
        "base sheet must not include overview rules"
    );
    println!("GET {} -> {c}, {} bytes (base)", sheets[0], css.len());
    let (c, _, ocss) = get(sheets[1], "");
    assert_eq!(c, 200);
    assert!(ocss.contains("ov-card"));
    println!("GET {} -> {c}, {} bytes (overview)", sheets[1], ocss.len());

    let (c, h, _) = get("/", "");
    assert_eq!(c, 302);
    assert!(h.contains("Location: /console"));
    println!("GET / -> {c} Location: /console");
    for p in [
        "/status",
        "/services",
        "/agents",
        "/docs",
        "/soon",
        "/hermes",
    ] {
        let (c, _, b) = get(p, "");
        assert_eq!(c, 200, "{p}");
        assert!(!b.contains("<script"), "{p}");
        println!("GET {p} -> {c}");
    }
    let (_, _, b) = get("/console", "Cookie: galion_theme=light\r\n");
    assert!(b.contains("data-theme=\"light\""));
    println!("GET /console (cookie light) -> data-theme=light");
}
