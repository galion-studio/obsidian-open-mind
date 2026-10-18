// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Load control: slot queue (backpressure), 429 + Retry-After, run timeout,
//! header timeout (408), per-listener max_conns (503), /metrics + /status auth.
//! Mock provider only: no network, no spend.

use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::provider::MockProvider;
use hermes_rs::session::MemStore;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{ClockTool, Registry};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

const KEY: &str = "test-key-limits-0123456789";

fn harness(delay_ms: u64, tweak: impl FnOnce(&mut Config)) -> SocketAddr {
    let home = std::env::temp_dir().join(hermes_rs::new_id("hermes-rs-limits-"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("SOUL.md"), "You are Hermes.").unwrap();
    let mut cfg = Config::defaults(&home);
    tweak(&mut cfg);
    let cfg = Arc::new(cfg);
    let mut mock = MockProvider::echo();
    mock.delay_ms = delay_ms;
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let mut tools = Registry::new();
    tools.add(Arc::new(ClockTool));
    let agent = hermes_rs::agent::boss(cfg.clone(), Arc::new(mock), spend, tools);
    let gw = Arc::new(Gateway::new(
        cfg,
        agent,
        Arc::new(MemStore::default()),
        Some(KEY.into()),
    ));
    hermes_rs::api::spawn(gw, "127.0.0.1:0").unwrap()
}

fn http(addr: SocketAddr, raw: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    (status, head.to_ascii_lowercase(), body.to_string())
}

fn chat(addr: SocketAddr, text: &str) -> (u16, String, String) {
    let b = json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}]})
        .to_string();
    http(
        addr,
        &format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        ),
    )
}

fn get(addr: SocketAddr, path: &str, auth: bool) -> (u16, String, String) {
    let a = if auth {
        format!("Authorization: Bearer {KEY}\r\n")
    } else {
        String::new()
    };
    http(addr, &format!("GET {path} HTTP/1.1\r\nHost: x\r\n{a}\r\n"))
}

fn status_json(addr: SocketAddr) -> Value {
    let (s, _, b) = get(addr, "/status", true);
    assert_eq!(s, 200, "{b}");
    serde_json::from_str(&b).unwrap()
}

#[test]
fn busy_slots_queue_instead_of_429() {
    let addr = harness(250, |c| {
        c.max_concurrent_runs = 1;
        c.limits.queue_wait = Duration::from_secs(5);
        c.limits.queue_max = 8;
    });
    let t0 = Instant::now();
    let hs: Vec<_> = (0..3)
        .map(|i| std::thread::spawn(move || chat(addr, &format!("q{i}")).0))
        .collect();
    let codes: Vec<u16> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(codes, vec![200, 200, 200]);
    // Serialised through one slot: at least 3 x 250 ms.
    assert!(t0.elapsed() >= Duration::from_millis(700));
    let st = status_json(addr);
    assert!(st["runs"]["queued_total"].as_u64().unwrap() >= 2, "{st}");
    assert_eq!(st["runs"]["max"], 1);
}

#[test]
fn no_queue_gives_429_with_retry_after() {
    let addr = harness(400, |c| {
        c.max_concurrent_runs = 1;
        c.limits.queue_wait = Duration::ZERO;
    });
    let h = std::thread::spawn(move || chat(addr, "slow").0);
    std::thread::sleep(Duration::from_millis(100));
    let (s, head, body) = chat(addr, "second");
    assert_eq!(s, 429, "{body}");
    assert!(head.contains("retry-after: 1"), "{head}");
    assert!(body.contains("max_concurrent_runs"));
    assert_eq!(h.join().unwrap(), 200);
}

#[test]
fn run_timeout_cancels_the_turn() {
    let addr = harness(800, |c| {
        c.limits.run_timeout = Duration::from_millis(200);
    });
    let t0 = Instant::now();
    let (s, head, body) = chat(addr, "will time out");
    assert!(t0.elapsed() < Duration::from_secs(5));
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let fin = v["choices"][0]["finish_reason"].as_str().unwrap_or("");
    assert!(
        s != 200 || fin != "stop" || head.contains("x-hermes-completed: false"),
        "{s} {head} {body}"
    );
    let st = status_json(addr);
    assert!(st["runs"]["timeouts"].as_u64().unwrap() >= 1, "{st}");
}

#[test]
fn slow_headers_get_408() {
    let addr = harness(0, |c| c.limits.header_timeout = Duration::from_secs(1));
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"GET /health HTTP/1.1\r\n").unwrap();
    // Keep dribbling bytes: each read is short, but the total deadline still fires.
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(400));
        let _ = s.write_all(b"X-A: b\r\n");
    }
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 408"), "{out}");
}

#[test]
fn max_conns_gives_503_with_retry_after() {
    let addr = harness(0, |c| {
        c.limits.max_conns = 2;
        c.limits.header_timeout = Duration::from_secs(3);
    });
    let idle: Vec<TcpStream> = (0..2).map(|_| TcpStream::connect(addr).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(200));
    let (s, head, _) = get(addr, "/health", false);
    assert_eq!(s, 503);
    assert!(head.contains("retry-after: 1"));
    drop(idle);
    // Freed once the idle sockets close (their reader sees EOF).
    let t0 = Instant::now();
    loop {
        let (s, _, _) = get(addr, "/health", false);
        if s == 200 {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(5), "never recovered");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn metrics_and_status_need_the_key_unless_open() {
    let addr = harness(0, |_| {});
    assert_eq!(get(addr, "/metrics", false).0, 401);
    assert_eq!(get(addr, "/status", false).0, 401);
    let (s, head, body) = get(addr, "/metrics", true);
    assert_eq!(s, 200);
    assert!(head.contains("text/plain"));
    for k in [
        "hermes_up 1",
        "hermes_runs_active",
        "hermes_run_duration_seconds_bucket{le=\"+Inf\"}",
        "hermes_requests_total{route=\"health\"}",
    ] {
        assert!(body.contains(k), "missing {k}");
    }
    let st = status_json(addr);
    assert_eq!(st["status"], "ok");
    assert!(st["spend"].is_object());
    // /health unchanged and open.
    let (s, _, b) = get(addr, "/health", false);
    assert_eq!(s, 200);
    let h: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(
        h,
        json!({"status": "ok", "platform": "hermes-agent", "version": hermes_rs::UPSTREAM_COMPAT, "implementation": "hermes-rs", "implementation_version": hermes_rs::VERSION})
    );
    let open = harness(0, |c| c.limits.metrics_open = true);
    assert_eq!(get(open, "/metrics", false).0, 200);
}
