// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 236 (cortex-api-load, T5.1 follow-ups F4/F5): API latency of the
//! REAL release `gaia-cortex` under a connection flood and while the
//! decider is stuck inside a call to a hung nerve. Box only, loopback only,
//! slot ports 21270-21279.
//!
//! Gated: `GALION_REAL=1`. Binary: `GALION_CORTEX_BIN` (default: this
//! crate's binary). `GALION_LOAD_BASELINE=1` only records the numbers (to
//! measure an older binary, the "before"); otherwise the fixed targets are
//! asserted: every `/health` and `/stats` answers 200 with p99 < 100 ms in
//! every phase. `GALION_LOAD_OUT=<file>` writes the JSON summary.
//!
//! `GALION_LOAD_TRACE=1` prints every sample.
//!
//! Run: `GALION_REAL=1 cargo test --release -p gaia-cortex --test real_api_load -- --nocapture`
//!
//! Phases (one cortex, one hung stub nerve that accepts and never answers):
//!   P0 quiet           200 sequential reads (/health and /stats alternating)
//!   P1 flood           32 idle connections (= max_connections), each reconnects as soon as it is closed; 200 reads
//!   P2 hung decider    push a unit failure: the decider sits in POST /actions to the hung nerve (3 x 3 s); reads for 6 s
//!   P3 hung + flood    both at once; reads for 6 s
//!   checks             invalid push items counted while busy show up in /stats; decisions land (by = unknown);
//!                      after the hang /stats is fresh again (busy false)
//!
//! Nothing here touches systemd, the VPS or any address beyond 127.0.0.1.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 21270;
const CORTEX_PORT: u16 = 21271;
const FLOOD: usize = 32;

fn now_unix() -> i64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds; printed through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[236 {}] {}",
        zagreb_time::from_unix(now_unix()),
        msg.as_ref()
    );
}

/// One request on a fresh connection: (status, headers+body text, elapsed).
fn http(method: &str, path: &str, body: &str, timeout: Duration) -> (u16, String, Duration) {
    let t = Instant::now();
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], CORTEX_PORT).into(), timeout)
    else {
        return (0, String::new(), t.elapsed());
    };
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new(), t.elapsed());
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let el = t.elapsed();
    let out = String::from_utf8_lossy(&out).to_string();
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, out, el)
}

fn body_json(raw: &str) -> Value {
    raw.split_once("\r\n\r\n")
        .and_then(|(_, b)| serde_json::from_str(b).ok())
        .unwrap_or(Value::Null)
}

fn stats() -> Value {
    let (c, raw, _) = http("GET", "/stats", "", Duration::from_secs(15));
    if c == 200 {
        body_json(&raw)
    } else {
        Value::Null
    }
}

#[derive(Default)]
struct Samples {
    ms: Vec<f64>,
    codes: Vec<u16>,
    busy: u64,
}

impl Samples {
    fn add(&mut self, code: u16, raw: &str, el: Duration) {
        self.ms.push(el.as_secs_f64() * 1000.0);
        self.codes.push(code);
        if raw.contains("x-cortex-busy: 1") || raw.contains("\"busy\":true") {
            self.busy += 1;
        }
    }
    fn pct(&self, p: f64) -> f64 {
        if self.ms.is_empty() {
            return f64::NAN;
        }
        let mut v = self.ms.clone();
        v.sort_by(|a, b| a.total_cmp(b));
        let i = ((v.len() as f64 * p).ceil() as usize).clamp(1, v.len()) - 1;
        v[i]
    }
    fn ok(&self) -> usize {
        self.codes.iter().filter(|c| **c == 200).count()
    }
    fn json(&self) -> Value {
        let r = |x: f64| (x * 100.0).round() / 100.0;
        let mut codes = std::collections::BTreeMap::<String, u64>::new();
        for c in &self.codes {
            *codes.entry(c.to_string()).or_default() += 1;
        }
        json!({"n": self.ms.len(), "ok_200": self.ok(), "codes": codes, "busy_snapshots": self.busy,
               "p50_ms": r(self.pct(0.50)), "p99_ms": r(self.pct(0.99)), "max_ms": r(self.pct(1.0))})
    }
    fn line(&self) -> String {
        format!(
            "n {} ok {} p50 {:.2} ms p99 {:.2} ms max {:.2} ms busy {}",
            self.ms.len(),
            self.ok(),
            self.pct(0.5),
            self.pct(0.99),
            self.pct(1.0),
            self.busy
        )
    }
}

/// Sample /health and /stats alternately: `count` pairs, or until `window` ends.
fn sample(count: usize, window: Option<Duration>) -> (Samples, Samples) {
    let (mut h, mut s) = (Samples::default(), Samples::default());
    let t = Instant::now();
    for _ in 0..count {
        if window.is_some_and(|w| t.elapsed() >= w) {
            break;
        }
        let (c, raw, el) = http("GET", "/health", "", Duration::from_secs(15));
        h.add(c, &raw, el);
        if std::env::var_os("GALION_LOAD_TRACE").is_some() {
            println!(
                "  trace +{} ms /health {c} {:.1} ms",
                t.elapsed().as_millis(),
                el.as_secs_f64() * 1000.0
            );
        }
        let (c, raw, el) = http("GET", "/stats", "", Duration::from_secs(15));
        s.add(c, &raw, el);
        if std::env::var_os("GALION_LOAD_TRACE").is_some() {
            println!(
                "  trace +{} ms /stats {c} {:.1} ms",
                t.elapsed().as_millis(),
                el.as_secs_f64() * 1000.0
            );
        }
        if window.is_some() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    (h, s)
}

/// A nerve that accepts every connection and never answers.
fn hung_nerve(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<u64> {
    let l = TcpListener::bind(("127.0.0.1", NERVE_PORT)).expect("bind stub nerve");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        let mut n = 0u64;
        while !stop.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((s, _)) => {
                    n += 1;
                    held.push(s);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        n
    })
}

/// FLOOD idle connections; each reconnects as soon as the server closes it.
struct Flood {
    stop: Arc<AtomicBool>,
    reconnects: Arc<AtomicU64>,
    threads: Vec<std::thread::JoinHandle<()>>,
}
impl Flood {
    fn start() -> Flood {
        let stop = Arc::new(AtomicBool::new(false));
        let reconnects = Arc::new(AtomicU64::new(0));
        let threads = (0..FLOOD)
            .map(|_| {
                let (stop, rc) = (stop.clone(), reconnects.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        let Ok(mut s) = TcpStream::connect(("127.0.0.1", CORTEX_PORT)) else {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        };
                        let _ = s.set_read_timeout(Some(Duration::from_millis(100)));
                        let mut b = [0u8; 256];
                        loop {
                            if stop.load(Ordering::SeqCst) {
                                return;
                            }
                            match s.read(&mut b) {
                                Ok(0) => break,
                                Ok(_) => {} // a 503 "busy" reply; wait for the close
                                Err(e)
                                    if matches!(
                                        e.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) => {}
                                Err(_) => break,
                            }
                        }
                        rc.fetch_add(1, Ordering::Relaxed);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(300));
        Flood {
            stop,
            reconnects,
            threads,
        }
    }
    fn stop(self) -> u64 {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads {
            let _ = t.join();
        }
        self.reconnects.load(Ordering::Relaxed)
    }
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn push_failure(unit: &str, seq: u64) -> u16 {
    let ev = json!({"source": "t236-push", "kind": "unit.failed", "severity": "high", "unit": unit, "seq": seq});
    http(
        "POST",
        "/v1/events",
        &ev.to_string(),
        Duration::from_secs(15),
    )
    .0
}

#[test]
fn api_stays_fast_under_flood_and_hung_decider() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    let baseline = std::env::var("GALION_LOAD_BASELINE").as_deref() == Ok("1");
    let bin = std::env::var("GALION_CORTEX_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_gaia-cortex").into());
    let dir = std::env::temp_dir().join(format!("cortex-236-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let nerve_stop = Arc::new(AtomicBool::new(false));
    let nerve = hung_nerve(nerve_stop.clone());
    let stderr = std::fs::File::create(dir.join("cortex.log")).unwrap();
    let _cortex = Proc(
        Command::new(&bin)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("CORTEX_LISTEN", format!("127.0.0.1:{CORTEX_PORT}"))
            .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
            .env("CORTEX_DATA_DIR", dir.join("data"))
            .env("CORTEX_COALESCE_MS", "200")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("spawn cortex"),
    );
    let t = Instant::now();
    while http("GET", "/health", "", Duration::from_secs(2)).0 != 200 {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "cortex did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    log(format!("cortex {bin} up; baseline mode {baseline}"));
    let summary = Mutex::new(serde_json::Map::new());
    let record = |name: &str, h: &Samples, s: &Samples, extra: Value| {
        log(format!(
            "{name}: /health {} | /stats {} | {extra}",
            h.line(),
            s.line()
        ));
        summary.lock().unwrap().insert(
            name.into(),
            json!({"health": h.json(), "stats": s.json(), "extra": extra}),
        );
    };

    // P0 quiet
    let (h0, s0) = sample(100, None);
    record("P0_quiet", &h0, &s0, json!({}));

    // P1 flood
    let flood = Flood::start();
    let (h1, s1) = sample(100, Some(Duration::from_secs(20)));
    let rc = flood.stop();
    record("P1_flood", &h1, &s1, json!({"flood_reconnects": rc}));

    // P2 hung decider
    let tp = Instant::now();
    let inv0 = stats()["events_invalid"].as_u64().unwrap_or(0);
    let pre_stats_ms = tp.elapsed().as_millis() as u64;
    let tp = Instant::now();
    assert_eq!(push_failure("galion-desk.service", 236_001), 202);
    let push_ms = tp.elapsed().as_millis() as u64;
    log(format!(
        "P2 setup: /stats {pre_stats_ms} ms, push {push_ms} ms"
    ));
    std::thread::sleep(Duration::from_millis(900));
    // invalid push items while the decider is busy, in parallel with the reads
    let bad = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(100));
        let (c, _, el) = http(
            "POST",
            "/v1/events",
            "not json\n{\"x\":1}\n",
            Duration::from_secs(15),
        );
        (c, el)
    });
    let (h2, s2) = sample(1000, Some(Duration::from_secs(6)));
    let (pc, push_el) = bad.join().unwrap();
    record(
        "P2_hung_decider",
        &h2,
        &s2,
        json!({"invalid_push_status": pc, "invalid_push_ms": push_el.as_millis() as u64}),
    );

    // wait for the decider to give up on the hung nerve (3 x 3 s)
    let t = Instant::now();
    loop {
        let v = stats();
        if v["busy"] != true && v["decisions"].as_u64().unwrap_or(0) >= 1 {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(40),
            "decider never came back: {v}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let after = stats();
    let inv1 = after["events_invalid"].as_u64().unwrap_or(0);

    // P3 hung + flood
    assert_eq!(push_failure("galion-health.service", 236_002), 202);
    let flood = Flood::start(); // includes 300 ms settle
    std::thread::sleep(Duration::from_millis(600));
    let (h3, s3) = sample(1000, Some(Duration::from_secs(6)));
    let rc = flood.stop();
    record(
        "P3_hung_and_flood",
        &h3,
        &s3,
        json!({"flood_reconnects": rc}),
    );

    let t = Instant::now();
    let fin = loop {
        let v = stats();
        if v["busy"] != true && v["decisions"].as_u64().unwrap_or(0) >= 2 {
            break v;
        }
        assert!(
            t.elapsed() < Duration::from_secs(40),
            "decider never came back: {v}"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    let (_, draw, _) = http("GET", "/decisions?n=10", "", Duration::from_secs(15));
    let decs = body_json(&draw);
    let by: Vec<Value> = decs
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|d| d["rule"] != "R10-remediation")
        .map(|d| json!({"subject": d["subject"], "by": d["action"]["by"], "status": d["action"]["status"]}))
        .collect();
    log(format!(
        "after: events_invalid {inv0} -> {inv1} (pushed 2 invalid lines while busy), decisions {}, actions_unknown {}, decided {by:?}",
        fin["decisions"], fin["actions_unknown"]
    ));
    nerve_stop.store(true, Ordering::SeqCst);
    let nerve_conns = nerve.join().unwrap_or(0);
    {
        let mut m = summary.lock().unwrap();
        m.insert("binary".into(), json!(bin));
        m.insert("baseline_mode".into(), json!(baseline));
        m.insert("events_invalid_before_after".into(), json!([inv0, inv1]));
        m.insert("final_stats".into(), fin.clone());
        m.insert("decided".into(), json!(by));
        m.insert("stub_nerve_connections".into(), json!(nerve_conns));
        m.insert("finished".into(), json!(zagreb_time::from_unix(now_unix())));
    }
    let out = Value::Object(summary.lock().unwrap().clone());
    if let Ok(p) = std::env::var("GALION_LOAD_OUT") {
        let _ = std::fs::write(p, serde_json::to_string_pretty(&out).unwrap());
    }
    if let Ok(l) = std::fs::read_to_string(dir.join("cortex.log")) {
        for line in l.lines().take(40) {
            log(format!("cortex.log: {line}"));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    if baseline {
        return;
    }
    for (name, h, s) in [
        ("P0", &h0, &s0),
        ("P1", &h1, &s1),
        ("P2", &h2, &s2),
        ("P3", &h3, &s3),
    ] {
        for (route, x) in [("/health", h), ("/stats", s)] {
            assert!(
                x.ms.len() >= 20,
                "{name} {route}: only {} samples",
                x.ms.len()
            );
            assert_eq!(
                x.ok(),
                x.ms.len(),
                "{name} {route}: non-200 replies {:?}",
                x.codes
            );
            assert!(
                x.pct(0.99) < 100.0,
                "{name} {route}: p99 {:.1} ms",
                x.pct(0.99)
            );
        }
    }
    assert!(s2.busy > 0, "P2 must have hit the busy snapshot path");
    assert_eq!(pc, 400, "invalid-only push is a 400");
    assert!(
        push_el < Duration::from_millis(100),
        "push ingest waited {push_el:?}"
    );
    assert_eq!(
        inv1,
        inv0 + 2,
        "invalid items counted while busy are not lost"
    );
    assert!(fin["actions_unknown"].as_u64().unwrap_or(0) >= 2, "{fin}");
}
