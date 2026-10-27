// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 247 (D-236-2, cortex-nerve-io-unlock): decision latency of the REAL
//! release `gaia-cortex` against a stub nerve that accepts every connection
//! and never answers (a hung / SIGSTOPped nerve). Box only, loopback only,
//! slot ports 21270-21279.
//!
//! Gated: `GALION_REAL=1`. Binary: `GALION_CORTEX_BIN` (default: this
//! crate's binary). `GALION_IO_BASELINE=1` only records the numbers (to
//! measure the "before" binary, e.g. 5e0adba); otherwise the targets are
//! asserted. `GALION_IO_OUT=<file>` writes the JSON summary.
//!
//! Run: `GALION_REAL=1 cargo test --release -p gaia-cortex --test real_nerve_io_unlock -- --nocapture`
//!
//! Phases:
//!   H1 one failure      decision latency; during the hang /stats is fresh (no snapshot age) and
//!                       POST /escalations/ack answers at once (404 for an unknown id, never 503)
//!   H2 two in a batch   one push with two unit failures: ONE POST /actions reaches the nerve, the
//!                       other request is "not sent" (per-batch breaker); both decided in one timeout
//!   H3 back to back     three single-failure batches 300 ms apart: the last decision lands within
//!                       about one timeout per batch (posts go to the courier, not the decider)
//!   checks              stub saw exactly one POST /actions per batch; nerve_posts dropped 0
//!
//! Nothing here touches systemd, the VPS or any address beyond 127.0.0.1.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 21270;
const CORTEX_PORT: u16 = 21271;

fn now_unix() -> i64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds; printed through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[247 {}] {}",
        zagreb_time::from_unix(now_unix()),
        msg.as_ref()
    );
}

/// One request on a fresh connection: (status, headers+body text, elapsed).
fn http(method: &str, path: &str, body: &str) -> (u16, String, Duration) {
    let t = Instant::now();
    let timeout = Duration::from_secs(15);
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], CORTEX_PORT).into(), timeout)
    else {
        return (0, String::new(), t.elapsed());
    };
    let _ = s.set_read_timeout(Some(timeout));
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

fn get(path: &str) -> Value {
    body_json(&http("GET", path, "").1)
}

/// Request lines seen by the hung stub nerve, in order.
type Seen = Arc<Mutex<Vec<String>>>;

/// A nerve that accepts every connection, reads its request line, and never answers.
fn hung_nerve(stop: Arc<AtomicBool>, seen: Seen) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(("127.0.0.1", NERVE_PORT)).expect("bind stub nerve");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((s, _)) => {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                    if let Ok(c) = s.try_clone() {
                        let seen = seen.clone();
                        std::thread::spawn(move || {
                            let mut line = String::new();
                            let _ = BufReader::new(c).read_line(&mut line);
                            seen.lock().unwrap().push(line.trim().to_string());
                        });
                    }
                    held.push(s);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    })
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn failure(unit: &str, seq: u64) -> Value {
    json!({"source": "t247-push", "kind": "unit.failed", "severity": "high", "unit": unit, "seq": seq})
}

fn push(body: &Value) -> u16 {
    http("POST", "/v1/events", &body.to_string()).0
}

fn decisions_for(unit: &str) -> Vec<Value> {
    get("/decisions?n=200")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["subject"] == format!("unit:{unit}"))
        .collect()
}

/// Wait for the first decision on `unit`; returns it and the time from `t`.
fn decided(unit: &str, t: Instant, limit: Duration) -> (Value, u64) {
    loop {
        if let Some(d) = decisions_for(unit).into_iter().next() {
            return (d, t.elapsed().as_millis() as u64);
        }
        assert!(t.elapsed() < limit, "no decision for {unit} in {limit:?}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn actions_seen(seen: &Seen) -> usize {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("POST /actions"))
        .count()
}

#[test]
fn hung_nerve_cannot_stall_decisions() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    let baseline = std::env::var("GALION_IO_BASELINE").as_deref() == Ok("1");
    let bin = std::env::var("GALION_CORTEX_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_gaia-cortex").into());
    let dir = std::env::temp_dir().join(format!("cortex-247-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let seen: Seen = Arc::default();
    let nerve = hung_nerve(stop.clone(), seen.clone());
    let stderr = std::fs::File::create(dir.join("cortex.log")).unwrap();
    let cortex = Proc(
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
    while http("GET", "/health", "").0 != 200 {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "cortex did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    log(format!(
        "cortex {bin} up (pid {}); baseline {baseline}",
        cortex.0.id()
    ));
    let mut sum = serde_json::Map::new();

    // ---- H1 one failure
    let t = Instant::now();
    assert_eq!(push(&failure("galion-desk.service", 247_001)), 202);
    std::thread::sleep(Duration::from_millis(900)); // inside POST /actions
    let (sc, sraw, sel) = http("GET", "/stats", "");
    let (ac, araw, ael) = http("POST", "/escalations/ack?id=esc-none", "");
    let fresh = !sraw.contains("x-cortex-snapshot-age-ms");
    let busy = sraw.contains("x-cortex-busy: 1");
    let (d1, h1_ms) = decided("galion-desk.service", t, Duration::from_secs(40));
    log(format!(
        "H1: decision {} {} by {} after {h1_ms} ms; during the hang /stats {sc} in {} ms (fresh {fresh}, busy {busy}), \
         POST /escalations/ack {ac} in {} ms: {}",
        d1["rule"], d1["action"]["status"], d1["action"]["by"], sel.as_millis(), ael.as_millis(),
        body_json(&araw)
    ));
    sum.insert("H1".into(), json!({"decision_ms": h1_ms, "by": d1["action"]["by"], "stats_ms": sel.as_millis() as u64,
        "stats_fresh": fresh, "stats_busy": busy, "ack_status": ac, "ack_ms": ael.as_millis() as u64}));
    // let the first batch finish completely before the next
    let wait_idle = || {
        let t = Instant::now();
        while get("/health")["busy"] == true {
            assert!(t.elapsed() < Duration::from_secs(40), "decider never idle");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    wait_idle();
    let a1 = actions_seen(&seen);

    // ---- H2 two failures in one batch
    let t = Instant::now();
    assert_eq!(
        push(&json!([
            failure("tuner.service", 247_002),
            failure("galion-health.service", 247_003)
        ])),
        202
    );
    let (dt, _) = decided("tuner.service", t, Duration::from_secs(40));
    let (dh, _) = decided("galion-health.service", t, Duration::from_secs(40));
    let h2_ms = t.elapsed().as_millis() as u64;
    wait_idle();
    let a2 = actions_seen(&seen) - a1;
    log(format!(
        "H2: both decided after {h2_ms} ms; tuner {} by {} ({}), galion-health {} by {} ({}); POST /actions at the nerve: {a2}",
        dt["action"]["status"], dt["action"]["by"], dt["action"]["reason"],
        dh["action"]["status"], dh["action"]["by"], dh["action"]["reason"]
    ));
    let bys = [dt["action"]["by"].clone(), dh["action"]["by"].clone()];
    sum.insert(
        "H2".into(),
        json!({"decision_ms": h2_ms, "by": bys, "actions_at_nerve": a2}),
    );

    // ---- H3 three batches back to back
    let units = [
        "openmind-memory.service",
        "hermes-rs.service",
        "console.service",
    ];
    let t = Instant::now();
    for (i, u) in units.iter().enumerate() {
        assert_eq!(push(&failure(u, 247_010 + i as u64)), 202);
        std::thread::sleep(Duration::from_millis(300));
    }
    let mut per = vec![];
    for u in units {
        let (d, ms) = decided(u, t, Duration::from_secs(60));
        per.push(
            json!({"unit": u, "ms": ms, "by": d["action"]["by"], "status": d["action"]["status"]}),
        );
    }
    let h3_ms = t.elapsed().as_millis() as u64;
    wait_idle();
    let a3 = actions_seen(&seen) - a1 - a2;
    log(format!(
        "H3: 3 batches decided, last after {h3_ms} ms: {per:?}; POST /actions at the nerve: {a3}"
    ));
    sum.insert(
        "H3".into(),
        json!({"last_decision_ms": h3_ms, "per": per, "actions_at_nerve": a3}),
    );

    std::thread::sleep(Duration::from_secs(1));
    let st = get("/stats");
    let lines = seen.lock().unwrap().clone();
    let mut kinds: std::collections::BTreeMap<String, u64> = Default::default();
    for l in &lines {
        let k = l.split('?').next().unwrap_or("").to_string();
        *kinds.entry(k).or_default() += 1;
    }
    log(format!(
        "stub nerve request lines: {kinds:?}; cortex decisions {} actions_unknown {} alerts {} escalations {} nerve_posts {}",
        st["decisions"], st["actions_unknown"], st["alerts"], st["escalations"], st["nerve_posts"]
    ));
    sum.insert("stats".into(), st.clone());
    sum.insert("stub_requests".into(), json!(kinds));
    sum.insert("binary".into(), json!(bin));
    sum.insert("finished".into(), json!(zagreb_time::from_unix(now_unix())));
    if let Ok(p) = std::env::var("GALION_IO_OUT") {
        let _ = std::fs::write(
            p,
            serde_json::to_string_pretty(&Value::Object(sum)).unwrap(),
        );
    }
    drop(cortex);
    stop.store(true, Ordering::SeqCst);
    let _ = nerve.join();
    let _ = std::fs::remove_dir_all(&dir);
    if baseline {
        return;
    }
    // H1: one action timeout (3 s) + coalesce (200 ms) + slack.
    assert!(h1_ms < 4500, "H1 decision {h1_ms} ms");
    assert_eq!(d1["action"]["by"], "unknown");
    assert_eq!(d1["escalate"], true);
    assert_eq!(sc, 200);
    assert!(sel < Duration::from_millis(100), "/stats {sel:?}");
    assert!(
        fresh,
        "/stats is fresh while the decider waits on the nerve"
    );
    assert!(busy, "/stats says busy while the batch is in flight");
    assert_eq!(ac, 404, "ack answers (unknown id), not 503 busy");
    assert!(ael < Duration::from_millis(100), "ack {ael:?}");
    assert_eq!(a1, 1);
    // H2: one request sent, one held back; one timeout for the whole batch.
    assert_eq!(a2, 1, "breaker: one POST /actions per batch");
    let mut b = bys
        .iter()
        .map(|v| v.as_str().unwrap_or(""))
        .collect::<Vec<_>>();
    b.sort();
    assert_eq!(b, ["local", "unknown"]);
    assert!(h2_ms < 4500, "H2 {h2_ms} ms");
    // H3: about one timeout per batch, no 3 s posts in between.
    // batches 2 and 3 may coalesce while batch 1 waits on the nerve
    assert!((2..=3).contains(&a3), "H3 POST /actions {a3}");
    assert!(h3_ms < 3 * 3200 + 1500, "H3 {h3_ms} ms");
    assert_eq!(st["nerve_posts"]["dropped"], 0);
}
