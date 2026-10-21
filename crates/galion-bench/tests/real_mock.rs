// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test: start the `galion-bench-mock` BINARY on a slot-50 port
//! (19500-19509), hit it over real TCP, tear it down, check the port is free.
//! Gated: runs only with `GALION_REAL=1`. Run with `--release` so the binary
//! under test is the release build:
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-bench --test real_mock -- --nocapture
//! ```

use galion_bench::client::{get, post_json, request};
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener};
use std::process::{Child, Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_galion-bench-mock");
const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

/// First free port in this slot's range.
fn free_port() -> u16 {
    (19500..=19509)
        .find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("no free port in 19500-19509")
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn(port: u16, files: &[&str]) -> (Kill, SocketAddr, String) {
    let mut cmd = Command::new(BIN);
    cmd.args(["--bind", &format!("127.0.0.1:{port}")]);
    for f in files {
        cmd.args(["--fixtures", &format!("{FIX}/{f}")]);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(
        line.starts_with("listening http://127.0.0.1:"),
        "banner: {line}"
    );
    (
        Kill(child),
        SocketAddr::from(([127, 0, 0, 1], port)),
        line.trim().to_string(),
    )
}

#[test]
fn real_binary_check_mode() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let out = Command::new(BIN)
        .args([
            "--check",
            &format!("{FIX}/example-bank.jsonl"),
            &format!("{FIX}/example-vending-supplier.jsonl"),
            &format!("{FIX}/example-paper-quotes.jsonl"),
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    println!("check: {}", text.trim());
    assert!(out.status.success());
    assert!(text.starts_with("ok fixtures=13 at="));

    let bad = std::env::temp_dir().join(format!("galion-bench-bad-{}.jsonl", std::process::id()));
    std::fs::write(
        &bad,
        "{\"id\":\"x\",\"path\":\"/a\",\"body\":{\"t\":\"2026-10-20T21:00:00Z\"}}\n",
    )
    .unwrap();
    let out = Command::new(BIN)
        .args(["--check", bad.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&bad);
    let err = String::from_utf8_lossy(&out.stderr);
    println!("bad check: {}", err.trim());
    assert!(!out.status.success());
    assert!(err.contains("ends in Z"));
}

#[test]
fn real_binary_serves_fixtures() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let port = free_port();
    let (child, a, banner) = spawn(
        port,
        &[
            "example-bank.jsonl",
            "example-vending-supplier.jsonl",
            "example-paper-quotes.jsonl",
        ],
    );
    println!("{banner}");

    let r = request(
        a,
        "GET",
        "/api/1.0/accounts",
        &[("Authorization", "Bearer mock-token")],
        "",
    )
    .unwrap();
    println!("GET /api/1.0/accounts -> {} {}", r.status, r.body);
    assert_eq!(r.status, 200);
    assert!(
        zagreb_time::require_rfc3339(r.json().unwrap()[0]["updated_at"].as_str().unwrap()).is_ok()
    );

    let s1 = get(a, "/api/1.0/rate?from=EUR&to=USD").unwrap().status;
    let s2 = get(a, "/api/1.0/rate?from=EUR&to=USD").unwrap().status;
    println!("GET /api/1.0/rate x2 -> {s1}, {s2}");
    assert_eq!((s1, s2), (503, 200));

    let r = post_json(a, "/api/1.0/pay", &json!({"amount": 1, "currency": "EUR"})).unwrap();
    println!("POST /api/1.0/pay -> {} {}", r.status, r.body);
    assert_eq!(r.status, 403);

    let o1 = post_json(
        a,
        "/supplier/orders",
        &json!({"sku": "cola-330", "qty": 24}),
    )
    .unwrap();
    let o2 = post_json(
        a,
        "/supplier/orders",
        &json!({"sku": "chips-50", "qty": 12}),
    )
    .unwrap();
    println!(
        "POST /supplier/orders x2 -> {} {} / {}",
        o1.status, o1.body, o2.body
    );
    assert_eq!(o2.json().unwrap()["order_id"], "po-2");

    let q = get(a, "/quotes/ACME").unwrap();
    println!("GET /quotes/ACME -> {} {}", q.status, q.body);
    assert_eq!(
        post_json(a, "/orders/new", &json!({"symbol": "ACME"}))
            .unwrap()
            .status,
        403
    );

    // concurrent hits: every one answered, journal counts them all
    let hs: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(move || get(a, "/supplier/catalog").unwrap().status))
        .collect();
    assert!(hs.into_iter().all(|h| h.join().unwrap() == 200));

    let j = get(a, "/__bench/journal").unwrap().json().unwrap();
    let hits = j["hits"].as_array().unwrap();
    println!("journal: {} hits, journal at {}", hits.len(), j["at"]);
    assert_eq!(hits.len(), 16);
    assert!(hits
        .iter()
        .all(|h| zagreb_time::require_rfc3339(h["at"].as_str().unwrap()).is_ok()));

    drop(child);
    assert!(
        get(a, "/__bench/health").is_err(),
        "mock still answering after kill"
    );
    assert!(TcpListener::bind(a).is_ok(), "port {port} not freed");
    println!("torn down; port {port} free");
}
