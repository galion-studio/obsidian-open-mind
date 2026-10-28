// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for slot 178 cortex-connrate-fix (corrected B41 conn-rate
//! formula, `NERVE_CONNRATE_FORMULA=b41flow`, off by default). Gated
//! `GALION_REAL=1`; run with `--release`. Ports 20480-20489.
//!
//! Two release `openmind-nerve serve` processes watch the same live service
//! on :20481 under a *steady* ~20 connections/s load held past the 60 s
//! TIME-WAIT window:
//! - :20480 default formula (identity count, unchanged cortex-s2-b output);
//! - :20482 `NERVE_CONNRATE_FORMULA=b41flow`.
//!
//! Both must report `conn-rate-20481` high with rates within 25 % of each
//! other and of the client's own count, while the old B41 delta (evidence
//! line 2) stays ~0. Load stopped -> both clear.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): SVC +1, ID_PORT +0, FLOW_PORT +2; `NERVE_CONNRATE_REAL_PORT_BASE`
/// moves the block, unset keeps the old
/// 20480-20489.
static PORTS: Block = Block::new("NERVE_CONNRATE_REAL_PORT_BASE", 20480, 10);

#[allow(non_snake_case)]
fn SVC() -> u16 {
    PORTS.port(1)
}
#[allow(non_snake_case)]
fn ID_PORT() -> u16 {
    PORTS.port(0)
}
#[allow(non_snake_case)]
fn FLOW_PORT() -> u16 {
    PORTS.port(2)
}
const INTERVAL_S: u64 = 10;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

fn state(port: u16) -> Value {
    get(port, "/api/v1/state").unwrap_or_default()
}

fn cycle(st: &Value) -> u64 {
    st["cycle"].as_u64().unwrap_or(0)
}

/// Wait until the nerve on `port` has run a cycle after `after`.
fn next_cycle(port: u16, after: u64) -> Value {
    let t = Instant::now();
    loop {
        let st = state(port);
        if cycle(&st) > after || t.elapsed() > Duration::from_secs(3 * INTERVAL_S + 10) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn conn_finding(st: &Value) -> Option<Value> {
    st["cortex"]["bleeding"]
        .as_array()?
        .iter()
        .find(|f| f["id"] == format!("conn-rate-{}", SVC()))
        .cloned()
}

/// `port X: R new connections/s over ...` -> R.
fn title_rate(f: &Value) -> f64 {
    let t = f["title"].as_str().unwrap_or_default();
    t.split(": ")
        .nth(1)
        .and_then(|r| r.split(' ').next())
        .and_then(|r| r.parse().ok())
        .unwrap_or(-1.0)
}

/// `... growth G/s)` in evidence line 2 -> G (old B41 delta).
fn old_growth(f: &Value) -> f64 {
    let e = f["evidence"][1].as_str().unwrap_or_default();
    e.split("growth ")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .and_then(|r| r.parse().ok())
        .unwrap_or(-1.0)
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn sensor_us(st: &Value) -> String {
    st["cortex_sensors"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["name"] == "conn-rate"))
        .map(|s| s["us"].to_string())
        .unwrap_or_default()
}

fn spawn(port: u16, dir: &std::path::Path, formula: Option<&str>) -> Kill {
    let comps = dir.join("components.json");
    let mut c = Command::new(BIN);
    c.arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", INTERVAL_S.to_string())
        .env("NERVE_DATA_DIR", dir.join(format!("data-{port}")))
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_LISTEN_SCOPE", PORTS.range())
        .env("NERVE_EDGE_HOSTS", "0")
        .env("NERVE_CONNRATE_PORTS", SVC().to_string())
        .env_remove("NERVE_CONNRATE_FORMULA")
        .env_remove("NERVE_SENSORS_OFF")
        .stderr(Stdio::null());
    if let Some(f) = formula {
        c.env("NERVE_CONNRATE_FORMULA", f);
    }
    Kill(c.spawn().expect("spawn nerve"))
}

#[test]
fn real_connrate_b41flow_under_steady_load() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nerve-real-178-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"s178-svc","port":{}}}]"#, SVC()),
    )
    .unwrap();

    // Live service: answer and close (server closes first -> TIME-WAIT on
    // the server row, like hermes-rs answering HTTP/1.0 probes).
    let svc = TcpListener::bind(("127.0.0.1", SVC())).expect("slot port free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 512];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });

    let id_nerve = spawn(ID_PORT(), &dir, None);
    let flow_nerve = spawn(FLOW_PORT(), &dir, Some("b41flow"));

    // Steady ~20/s load (one connection every 50 ms, paced on a clock).
    let stop = Arc::new(AtomicBool::new(false));
    let made = Arc::new(AtomicU64::new(0));
    let (stop2, made2) = (stop.clone(), made.clone());
    let load = std::thread::spawn(move || {
        let t0 = Instant::now();
        let mut k: u64 = 0;
        while !stop2.load(Ordering::Relaxed) {
            if let Ok(mut s) = TcpStream::connect(("127.0.0.1", SVC())) {
                let _ = s.write_all(b"GET / HTTP/1.0\r\n\r\n");
                let mut x = Vec::new();
                let _ = s.read_to_end(&mut x);
                made2.fetch_add(1, Ordering::Relaxed);
            }
            k += 1;
            let due = t0 + Duration::from_millis(50 * k);
            if let Some(d) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(d);
            }
        }
    });

    // Warm-up past the 60 s TIME-WAIT window so the pool is flat.
    let t_load = Instant::now();
    std::thread::sleep(Duration::from_secs(75));
    let mut c_id = cycle(&state(ID_PORT()));
    let mut c_flow = cycle(&state(FLOW_PORT()));
    let mut rows = vec![];
    for _ in 0..3 {
        let m0 = made.load(Ordering::Relaxed);
        let t0 = Instant::now();
        let a = next_cycle(ID_PORT(), c_id);
        let b = next_cycle(FLOW_PORT(), c_flow);
        c_id = cycle(&a);
        c_flow = cycle(&b);
        let client = (made.load(Ordering::Relaxed) - m0) as f64 / t0.elapsed().as_secs_f64();
        let fa = conn_finding(&a).unwrap_or_else(|| panic!("identity: no finding {}", a["cortex"]));
        let fb = conn_finding(&b).unwrap_or_else(|| panic!("b41flow: no finding {}", b["cortex"]));
        let (ra, rb, g) = (title_rate(&fa), title_rate(&fb), old_growth(&fb));
        eprintln!(
            "t+{:.0}s client {client:.1}/s | :{id_port} cycle {c_id} identity: {} [{}] ev={} us={} | :{flow_port} cycle {c_flow} b41flow: {} [{}] ev3={} old-b41 {g:.2}/s us={}",
            t_load.elapsed().as_secs_f64(),
            fa["title"],
            fa["severity"],
            fa["evidence"].as_array().map(|v| v.len()).unwrap_or(0),
            sensor_us(&a),
            fb["title"],
            fb["severity"],
            fb["evidence"][3],
            sensor_us(&b),
            id_port = ID_PORT(),
            flow_port = FLOW_PORT(),
        );
        assert_eq!(
            fa["evidence"].as_array().unwrap().len(),
            3,
            "default unchanged"
        );
        assert_eq!(fb["evidence"].as_array().unwrap().len(), 4);
        assert!(fb["evidence"][3]
            .as_str()
            .unwrap()
            .starts_with("formula=b41flow "));
        assert_eq!(fa["severity"], "error", "{fa}");
        assert_eq!(fb["severity"], "error", "{fb}");
        assert!((rb - ra).abs() / ra <= 0.25, "flow {rb} vs identity {ra}");
        assert!(
            (rb - client).abs() / client <= 0.25,
            "flow {rb} vs client {client}"
        );
        assert!(
            g.abs() <= 2.0,
            "old B41 delta should be ~0 under steady load: {g}"
        );
        rows.push((ra, rb, g, client));
    }
    stop.store(true, Ordering::Relaxed);
    load.join().unwrap();
    let total = made.load(Ordering::Relaxed);

    // Load stopped: both clear (flow: pool drains, delta cancels aged-out).
    let t = Instant::now();
    let (mut a_clear, mut b_clear) = (None, None);
    while t.elapsed() < Duration::from_secs(4 * INTERVAL_S + 10)
        && (a_clear.is_none() || b_clear.is_none())
    {
        let a = next_cycle(ID_PORT(), c_id);
        let b = next_cycle(FLOW_PORT(), c_flow);
        c_id = cycle(&a);
        c_flow = cycle(&b);
        if a_clear.is_none() && conn_finding(&a).is_none() {
            a_clear = Some(c_id);
        }
        if b_clear.is_none() && conn_finding(&b).is_none() {
            b_clear = Some(c_flow);
        }
    }
    assert!(
        a_clear.is_some() && b_clear.is_some(),
        "cleared: {a_clear:?} {b_clear:?}"
    );
    eprintln!(
        "load stopped after {total} connections ({:.0} s); cleared identity cycle {a_clear:?}, b41flow cycle {b_clear:?}; rss identity {} KiB, b41flow {} KiB",
        t_load.elapsed().as_secs_f64(),
        rss_kib(id_nerve.0.id()),
        rss_kib(flow_nerve.0.id())
    );
    drop(id_nerve);
    drop(flow_nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
