// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1): does `hermes-gateway --mock` leak memory?
//!
//! Reproduces the 2026-10-18 "growth" bench load that the stray VPS
//! gateways on 8652/8653 served (health, /v1/models, chat at c3 and c12),
//! twice, and samples VmRSS / VmHWM from /proc after each round and after
//! idling. A linear leak shows as round 2 growing about as much as round 1;
//! a high-water plateau shows as round 2 adding little or nothing.
//! Port: slot 117 range 19170-19179 (override with GALION_REAL_PORT).
//! Run: `GALION_REAL=1 cargo test -p hermes-gateway --release --test real_mock_rss -- --nocapture`

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "real-test-key-not-secret-0117";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn status_kb(pid: u32, field: &str) -> u64 {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    s.lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn request(port: u16, method: &str, path: &str, body: Option<&str>) -> u16 {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return 0;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let b = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    );
    if s.write_all(req.as_bytes()).is_err() {
        return 0;
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let head = String::from_utf8_lossy(&out[..out.len().min(32)]).to_string();
    head.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

/// Closed-loop load: `conc` threads share `n` requests. Returns (ok, other).
fn load(
    port: u16,
    conc: usize,
    n: usize,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (usize, usize) {
    let next = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut hs = Vec::new();
    for _ in 0..conc {
        let next = next.clone();
        let (m, p, b) = (
            method.to_string(),
            path.to_string(),
            body.map(str::to_string),
        );
        hs.push(std::thread::spawn(move || {
            let (mut ok, mut other) = (0, 0);
            while next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < n {
                match request(port, &m, &p, b.as_deref()) {
                    200 => ok += 1,
                    _ => other += 1,
                }
            }
            (ok, other)
        }));
    }
    hs.into_iter()
        .map(|h| h.join().unwrap())
        .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

fn round(port: u16) -> (usize, usize) {
    let chat = r#"{"model":"hermes-agent","messages":[{"role":"user","content":"ping"}]}"#;
    let r = [
        load(port, 8, 5000, "GET", "/health", None),
        load(port, 8, 3000, "GET", "/v1/models", None),
        load(port, 3, 600, "POST", "/v1/chat/completions", Some(chat)),
        load(port, 12, 1200, "POST", "/v1/chat/completions", Some(chat)),
    ];
    println!(
        "REAL   stages(ok,non200) health={:?} models={:?} chat_c3={:?} chat_c12={:?}",
        r[0], r[1], r[2], r[3]
    );
    r.iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

#[test]
#[cfg(target_os = "linux")]
fn real_mock_gateway_rss_under_bench_load() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let port: u16 = std::env::var("GALION_REAL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(19170);
    let rounds: usize = std::env::var("GALION_REAL_ROUNDS")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6);
    let dir = std::env::temp_dir().join(format!("vps-strays-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_hermes-gateway"))
        .arg("--mock")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HERMES_HOME", &dir)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_STATE_DB", dir.join("s.db"))
        .env("HERMES_RS_SPEND_FILE", dir.join("spend.json"))
        .env("HERMES_RS_CRON", "0")
        // Lift the mock-only call cap so every chat round really runs (the VPS bench
        // strays served ~1.6-1.8k chats per run).
        .env("HERMES_RS_DAILY_CALL_CAP", "100000000")
        .env("API_SERVER_KEY", KEY)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start hermes-gateway");
    let pid = child.id();
    let _guard = Kill(child);
    let t0 = Instant::now();
    while request(port, "GET", "/health", None) != 200 {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "gateway did not come up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let base = status_kb(pid, "VmRSS:");
    println!(
        "REAL start rss_kb={base} hwm_kb={}",
        status_kb(pid, "VmHWM:")
    );
    let mut after = Vec::new();
    for r in 1..=rounds {
        let t = Instant::now();
        let (ok, other) = round(port);
        let rss = status_kb(pid, "VmRSS:");
        std::thread::sleep(Duration::from_secs(5));
        let idle = status_kb(pid, "VmRSS:");
        println!(
            "REAL round={r} ok={ok} non200={other} secs={:.1} rss_kb={rss} idle5s_rss_kb={idle} hwm_kb={}",
            t.elapsed().as_secs_f64(),
            status_kb(pid, "VmHWM:")
        );
        after.push(idle);
    }
    assert_eq!(request(port, "GET", "/health", None), 200, "gateway died");
    let growth: Vec<i64> = after
        .windows(2)
        .map(|w| w[1] as i64 - w[0] as i64)
        .collect();
    println!(
        "REAL verdict round1_growth_kb={} later_round_growth_kb={growth:?}",
        after[0] as i64 - base as i64
    );
    let _ = std::fs::remove_dir_all(&dir);
    // Regression gate for the stray-mock leak (docs/ops/vps-strays.md): before
    // the record_limit fix each round added ~5.5 MB; after it the last three
    // rounds add well under 2 MB together.
    if growth.len() >= 5 {
        let tail: i64 = growth[growth.len() - 3..].iter().sum();
        assert!(
            tail < 2048,
            "mock gateway RSS still grows: {tail} kB over the last 3 rounds"
        );
    }
}
