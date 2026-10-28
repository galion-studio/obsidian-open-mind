// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test: builds the RELEASE galion-health, starts it on 127.0.0.1:19490
//! (swarm slot 49 range 19490-19499), with a fake upstream on 19491 (HTTP 200),
//! a closed port 19492 (down) and a raw TCP port 19493, then GETs every endpoint
//! and tears everything down. Gated: run with
//! `GALION_REAL=1 cargo test -p galion-health --test real_health -- --ignored --nocapture`.
//! Optional GALION_HEALTH_BIN=<path> skips the build and tests that binary
//! (e.g. the rebuilt e43d7ab binary that matches the live sha256).
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::ports::Block;

/// Slot 375 (real-ports-batch-2): block movable by `HEALTH_REAL_PORT_BASE`
/// (unset = 19490): health +0, upstream +1, closed +2, raw +3.
static PORTS: Block = Block::new("HEALTH_REAL_PORT_BASE", 19490, 4);
fn addr_at() -> &'static str {
    PORTS.addr(0)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn release_bin() -> PathBuf {
    if let Ok(p) = std::env::var("GALION_HEALTH_BIN") {
        return PathBuf::from(p);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .args(["build", "--release", "-j", "2", "-p", "galion-health"])
        .status()
        .expect("spawn cargo build");
    assert!(st.success(), "release build failed");
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("release/galion-health")
}

fn get(path: &str) -> (String, String) {
    let mut s = TcpStream::connect(addr_at()).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    // One write: the server does a single read() and closes; split writes can get an RST.
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").as_bytes())
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").expect("http head");
    (head.to_string(), body.to_string())
}

fn upstream() -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(PORTS.addr(1)).expect("bind upstream");
    std::thread::spawn(move || {
        for s in l.incoming().take(64) {
            let Ok(mut s) = s else { continue };
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        }
    })
}

#[test]
#[ignore]
fn real_release_binary_endpoints() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let bin = release_bin();
    let _up = upstream();
    let _raw = TcpListener::bind(PORTS.addr(3)).expect("bind raw");
    // 1) all-green spec: fake upstream + raw tcp + no-probe entry.
    let green = format!(
        "up|-|{}|/health;raw|-|{}|-;noprobe|-|0|-",
        PORTS.port(1),
        PORTS.port(3)
    );
    let child = Command::new(&bin)
        .env("GALION_HEALTH_ADDR", addr_at())
        .env("GALION_HEALTH_CHECKS", &green)
        .env("GALION_LOG_FORMAT", "json")
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn galion-health");
    let _k = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(addr_at()).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(5), "no listen");
        std::thread::sleep(Duration::from_millis(50));
    }
    let (h, b) = get("/status.json");
    eprintln!("/status.json -> {}\n{b}", h.lines().next().unwrap());
    assert!(h.starts_with("HTTP/1.1 200 OK"));
    assert!(h.contains("Content-Type: application/json"));
    assert!(h.contains("Cache-Control: no-store"));
    assert!(b.starts_with("{\"ok\":true,\"checked_at\":\""));
    assert!(
        b.contains("+02:00\"") || b.contains("+01:00\""),
        "Zagreb offset"
    );
    assert!(b.contains(&format!(
        "\"name\":\"up\",\"unit\":\"-\",\"unit_state\":\"n/a\",\"port\":{},\"http\":\"200\"",
        PORTS.port(1)
    )));
    assert!(b.contains("\"http\":\"tcp-open\""));
    let (h2, b2) = get("/json");
    assert!(h2.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(b2, b, "5 s cache: /json == /status.json body");
    let (h, b) = get("/healthz");
    assert!(h.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(b, "ok\n");
    let (h, b) = get("/");
    assert!(h.contains("text/html; charset=utf-8"));
    assert!(b.contains("Galion health: 3/3 ok"));
    let (h, b) = get("/status");
    assert!(h.starts_with("HTTP/1.1 404 Not Found"));
    assert_eq!(b, "not found\n");
    drop(_k);

    // 2) failing spec: closed port -> down -> /healthz 503.
    std::thread::sleep(Duration::from_millis(200));
    let child = Command::new(&bin)
        .env("GALION_HEALTH_ADDR", addr_at())
        .env(
            "GALION_HEALTH_CHECKS",
            format!("gone|-|{}|/health;up|-|{}|/", PORTS.port(2), PORTS.port(1)),
        )
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn galion-health");
    let _k = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(addr_at()).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(5), "no listen");
        std::thread::sleep(Duration::from_millis(50));
    }
    let (h, b) = get("/healthz");
    eprintln!("/healthz (failing) -> {} {b}", h.lines().next().unwrap());
    assert!(h.starts_with("HTTP/1.1 503 Service Unavailable"));
    assert_eq!(b, "failing: gone\n");
    let (_, b) = get("/status.json");
    assert!(b.starts_with("{\"ok\":false,"));
    assert!(b.contains(&format!(
        "\"name\":\"gone\",\"unit\":\"-\",\"unit_state\":\"n/a\",\"port\":{},\"http\":\"down\"",
        PORTS.port(2)
    )));
}
