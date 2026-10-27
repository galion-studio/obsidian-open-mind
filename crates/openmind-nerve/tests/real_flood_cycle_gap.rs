// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 278 / P-233-1 rest: cycle-gap bucket spread for counter-only floods.
//!
//! Event-mode nerve with a long heartbeat must spread cloudflared
//! `total_requests` delta across one 10 s bucket per 10 s of the gap, so a
//! quiet ~0.6 req/s edge cannot CRITICAL. Slot 252's zero-baseline guard
//! remains the safety net when the gap is still ≤ 60 s (test heartbeat).
//!
//! Gated: `GALION_REAL=1`. Ports 21480-21489.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-nerve \
//!   --test real_flood_cycle_gap -- --nocapture --test-threads=1
//! ```

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const NERVE_PORT: u16 = 21480;
const METRICS_PORT: u16 = 21481;
const API_PORT: u16 = 21482;
const LIVE: u16 = 21484;
const DEAD: u16 = 21485;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn http(port: u16, path: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_secs(2),
    ) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    );
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, body)
}

fn get_json(port: u16, path: &str) -> Value {
    let (c, b) = http(port, path);
    assert_eq!(c, 200, "{path}: {b}");
    serde_json::from_str(&b).unwrap_or_else(|e| panic!("{path}: {e}: {b}"))
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-flood-cycle-gap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

fn serve_metrics(port: u16, reqs: Arc<AtomicU64>, errs: Arc<AtomicU64>) {
    let lst = TcpListener::bind(("127.0.0.1", port)).expect("metrics port free");
    std::thread::spawn(move || {
        for mut c in lst.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let body = format!(
                "cloudflared_tunnel_ha_connections 4\n\
                 cloudflared_tunnel_total_requests {}\n\
                 cloudflared_tunnel_request_errors {}\n",
                reqs.load(Ordering::Relaxed),
                errs.load(Ordering::Relaxed)
            );
            let _ = write!(
                c,
                "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
        }
    });
}

fn start_nerve(dir: &Path) -> Kill {
    std::fs::write(
        dir.join("components.json"),
        format!(
            r#"[{{"name":"cloudflared","kind":"tunnel","metrics_port":{METRICS_PORT},"group":"edge","unit":"cloudflared.service","gaia":"AETHER","system":"tunnels","critical":true}}]"#
        ),
    )
    .unwrap();
    let child = Command::new(BIN)
        .args([
            "serve",
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{NERVE_PORT}"),
            "--log",
            &dir.join("events.jsonl").display().to_string(),
        ])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env(
            "HOME",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
        )
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .env("NERVE_EVT_HEARTBEAT", "60")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .env("NERVE_DDOS_MIN_RPS", "10")
        .env("NERVE_DDOS_MIN_REQUESTS", "300")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nerve");
    let n = Kill(child);
    let t = Instant::now();
    while http(NERVE_PORT, "/health").0 != 200 {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "nerve failed to become healthy"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    n
}

#[test]
fn real_cycle_gap_counter_dump_is_not_critical() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = tmp();
    let reqs = Arc::new(AtomicU64::new(10_000));
    let errs = Arc::new(AtomicU64::new(100));
    serve_metrics(METRICS_PORT, Arc::clone(&reqs), Arc::clone(&errs));

    let _nerve = start_nerve(&dir);
    std::thread::sleep(Duration::from_secs(2));

    // Slot 233 numbers: +612 requests since previous cycle.
    reqs.fetch_add(612, Ordering::Relaxed);
    errs.fetch_add(400, Ordering::Relaxed);

    let deadline = Instant::now() + Duration::from_secs(75);
    let mut saw = None;
    while Instant::now() < deadline {
        let d = get_json(NERVE_PORT, "/api/v1/ddos");
        if let Some(arr) = d["streams"].as_array() {
            if let Some(s) = arr.iter().find(|s| s["stream"] == "edge-cloudflared") {
                if s["requests_60s"].as_u64().unwrap_or(0) >= 300 {
                    saw = Some(s.clone());
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    let s = saw.expect("edge-cloudflared never saw the 612-request dump");
    // Heartbeat gap here is ~60 s, so spread fills the minute; slot 252
    // zero-baseline / first-bucket skip still caps at elevated. Full 900 s
    // spread → normal is covered by lib/engine unit tests.
    assert_ne!(
        s["level"], "flood",
        "cycle-gap counter dump must not CRITICAL: {s}"
    );
    assert!(
        s["level"] == "elevated" || s["level"] == "normal",
        "expected warn-only or normal, got {}: {s}",
        s["level"]
    );

    let inc = get_json(NERVE_PORT, "/api/v1/incidents");
    let critical_ddos = inc["open"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|i| i["cause"] == "ddos:edge-cloudflared" && i["severity"] == "critical");
    assert!(
        !critical_ddos,
        "no CRITICAL ddos:edge-cloudflared incident: {inc}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_ip_bearing_api_flood_still_critical() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = tmp();
    let svc = TcpListener::bind(("127.0.0.1", LIVE)).expect("live port free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    std::fs::write(
        dir.join("components.json"),
        format!(
            r#"[{{"name":"live-svc","port":{LIVE}}},{{"name":"dead-svc","port":{DEAD},"gaia":"APOLLO"}}]"#
        ),
    )
    .unwrap();
    let _nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env(
                "HOME",
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
            )
            .env("NERVE_ADDR", format!("127.0.0.1:{API_PORT}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", dir.join("components.json"))
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", format!("{API_PORT}-{DEAD}"))
            .env("NERVE_DDOS_MIN_RPS", "5")
            .env("NERVE_DDOS_MIN_REQUESTS", "200")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    while http(API_PORT, "/healthz").0 != 200 {
        assert!(t.elapsed() < Duration::from_secs(15), "nerve health");
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_secs(6));
    let hs: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(move || {
                (0..100)
                    .filter(|_| http(API_PORT, "/healthz").0 == 200)
                    .count()
            })
        })
        .collect();
    let sent: usize = hs.into_iter().map(|h| h.join().unwrap()).sum();
    assert!(sent >= 700, "sent {sent}");
    std::thread::sleep(Duration::from_secs(6));
    let d = get_json(API_PORT, "/api/v1/ddos");
    let api = d["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["stream"] == "nerve-api")
        .cloned()
        .unwrap_or_else(|| panic!("no nerve-api stream: {d}"));
    assert_eq!(
        api["level"], "flood",
        "IP-bearing nerve-api flood must still CRITICAL: {api}"
    );
    assert_eq!(d["firing"], true, "{d}");
    let _ = std::fs::remove_dir_all(&dir);
}
