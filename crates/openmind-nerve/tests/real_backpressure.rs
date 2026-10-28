// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Nerve T3.3(b) back-pressure, real-life box test (gated: `GALION_REAL=1`;
//! wave-4 slot 187, ports 20570-20579). Follow-up to the 2026-10-21 go-live of
//! `NERVE_URL` in hermes-rs and openmind-memory: whatever the nerve does, a
//! service request must never wait for it, and every signal that does not
//! reach it must be counted.
//!
//! | port  | process |
//! |-------|---------|
//! | 20570 | in-test fake nerve (modes below) |
//! | 20571 | A: hermes-gateway --mock, `NERVE_URL` unset |
//! | 20572 | B: hermes-gateway --mock, `NERVE_URL` -> 20570 |
//! | 20573 | A: openmind-memory, `NERVE_URL` unset |
//! | 20574 | B: openmind-memory, `NERVE_URL` -> 20570 |
//! | 20575 | real openmind-nerve serve (real 429 phase; soak: probes B) |
//! | 20576 | R: hermes-gateway --mock, `NERVE_URL` -> 20575 |
//! | 20577 | R: openmind-memory, `NERVE_URL` -> 20575 |
//!
//! Fake nerve modes: `ok` (202), `dead` (port closed, connect refused),
//! `slow` (reads the signal, answers 202 after 5 s), `429` (answers 429),
//! `trickle` (writes its reply one byte per 250 ms, never a newline) and
//! `blackhole` (accepts, never reads or answers, 10 s).
//!
//! Every phase sends the same traffic to A and B in interleaved blocks (order
//! flipped each round). Half the B requests fail on purpose (hermes
//! `mock-fail` -> 502, memory `POST /seed` -> 501), so each of them pushes one
//! error signal: the attempted-signal count is known from the replies. After
//! a phase the fake nerve goes back to `ok` and the next client heartbeat
//! (10 s) carries the counters; per phase the test checks
//! `queued + dropped` against the pushed errors and
//! `queued == sent + failed + rejected` (nothing lost uncounted).
//!
//! Tests: `nerve_backpressure_box` (phases, ~4 min) and
//! `nerve_backpressure_soak` (`T33_SOAK_MIN`, default 20, max 30 minutes;
//! modes cycle every minute; RSS, threads, fds and nerve cycles sampled).
//! Knobs: `N_REQ` (default 400) requests per stream per phase,
//! `T33_BIN_DIR` (prebuilt release binaries; otherwise built into
//! `<target>/real-nerve-t33`), `T33_EXPECT_DEADLINE=0` to run against a
//! pre-T3.3 build (trickle hold is then reported, not asserted).
//! Run with `--nocapture --test-threads 1` to see the report.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FAKE: u16 = 20570;
const A_HERMES: u16 = 20571;
const B_HERMES: u16 = 20572;
const A_MEMORY: u16 = 20573;
const B_MEMORY: u16 = 20574;
const REAL_NERVE: u16 = 20575;
const R_HERMES: u16 = 20576;
const R_MEMORY: u16 = 20577;

const HB_S: u64 = 10;

// ---------------------------------------------------------------- processes

struct Proc(Child);
impl Proc {
    fn pid(&self) -> u32 {
        self.0.id()
    }
}
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

fn proc_field(pid: u32, key: &str) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn fds(pid: u32) -> u64 {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .map(|d| d.count() as u64)
        .unwrap_or(0)
}

/// One HTTP/1.0 request; (status, body, elapsed). Status 0 = no answer.
fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String, Duration) {
    let t = Instant::now();
    let addr = format!("127.0.0.1:{port}").parse().unwrap();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return (0, String::new(), t.elapsed());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new(), t.elapsed());
    }
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let el = t.elapsed();
    let text = String::from_utf8_lossy(&buf).to_string();
    let Some((h, b)) = text.split_once("\r\n\r\n") else {
        return (0, String::new(), el);
    };
    let code = h
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, b.to_string(), el)
}

fn get_json(port: u16, path: &str) -> Value {
    serde_json::from_str(&http(port, "GET", path, &[], "").1).unwrap_or(Value::Null)
}

fn wait_up(port: u16, path: &str) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if http(port, "GET", path, &[], "").0 != 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("port {port} did not come up");
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
}

fn binaries() -> PathBuf {
    if let Some(d) = std::env::var_os("T33_BIN_DIR") {
        return PathBuf::from(d);
    }
    let td = target_dir().join("real-nerve-t33");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let ok = Command::new(cargo)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .env("CARGO_TARGET_DIR", &td)
        .args(["build", "-q", "-j", "2", "--release"])
        .args([
            "-p",
            "openmind-nerve",
            "-p",
            "hermes-gateway",
            "-p",
            "openmind-memory",
        ])
        .status()
        .expect("run cargo build")
        .success();
    assert!(ok, "release build failed");
    td.join("release")
}

struct Stack {
    bin: PathBuf,
    work: PathBuf,
    key: String,
}

impl Stack {
    fn new(tag: &str) -> Arc<Stack> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let work =
            std::env::temp_dir().join(format!("nerve-t33-{tag}-{}-{now}", std::process::id()));
        std::fs::create_dir_all(work.join("data")).unwrap();
        // Throwaway box-only key for the mock gateway: not a credential, never printed.
        Arc::new(Stack {
            bin: binaries(),
            work,
            key: format!("boxtest-{now}-t33"),
        })
    }

    fn log(&self, name: &str) -> Stdio {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.work.join(name))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null())
    }

    fn nerve_env(&self, c: &mut Command, nerve: Option<u16>) {
        c.env_remove("NERVE_URL").env_remove("NERVE_QUEUE");
        if let Some(p) = nerve {
            // Live drop-ins set NERVE_URL only: default queue (256) and slow threshold.
            c.env("NERVE_URL", format!("http://127.0.0.1:{p}"))
                .env("NERVE_HEARTBEAT_S", HB_S.to_string());
        }
    }

    fn hermes(&self, port: u16, nerve: Option<u16>) -> Proc {
        let home = self.work.join(format!("hermes-{port}"));
        std::fs::create_dir_all(&home).unwrap();
        let mut c = Command::new(self.bin.join("hermes-gateway"));
        c.arg("--mock")
            .env("API_SERVER_KEY", &self.key)
            .env("HERMES_HOME", &home)
            .env("HERMES_RS_PORT", port.to_string())
            .env("HERMES_RS_MCP", "0")
            .env("HERMES_RS_DAILY_CALL_CAP", "1000000000")
            .stdout(Stdio::null())
            .stderr(self.log(&format!("hermes-{port}.log")));
        self.nerve_env(&mut c, nerve);
        Proc(c.spawn().expect("spawn hermes-gateway"))
    }

    fn memory(&self, port: u16, nerve: Option<u16>) -> Proc {
        let dir = self.work.join(format!("mem-{port}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Command::new(self.bin.join("openmind-memory"));
        c.env("OPENMIND_MEMORY_PORT", port.to_string())
            .env("OPENMIND_MEMORY_DATA_DIR", &dir)
            .stdout(Stdio::null())
            .stderr(self.log(&format!("mem-{port}.log")));
        self.nerve_env(&mut c, nerve);
        Proc(c.spawn().expect("spawn openmind-memory"))
    }

    /// The real nerve, probing `components` (name, port) every 5 s.
    fn nerve(&self, components: &[(&str, u16)]) -> Proc {
        let rows: Vec<String> = components
            .iter()
            .map(|(n, p)| format!(r#"{{"name":"{n}","port":{p},"path":"/health"}}"#))
            .collect();
        let comp = self.work.join("components.json");
        std::fs::write(&comp, format!("[{}]", rows.join(","))).unwrap();
        let p = Command::new(self.bin.join("openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{REAL_NERVE}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", self.work.join("data"))
            .env("NERVE_COMPONENTS", comp)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "20570-20579")
            .stdout(Stdio::null())
            .stderr(self.log("nerve.log"))
            .spawn()
            .expect("spawn openmind-nerve");
        Proc(p)
    }

    fn chat(&self, port: u16, text: &str) -> (u16, Duration) {
        let body = serde_json::json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}]})
            .to_string();
        let auth = format!("Bearer {}", self.key);
        let (c, _, d) = http(
            port,
            "POST",
            "/v1/chat/completions",
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            &body,
        );
        (c, d)
    }
}

/// Request `i` to a hermes port: every 2nd one fails on purpose (502, one error signal).
/// 429 = hermes' own max_concurrent_runs slot limit under 4-way load (no signal).
fn hermes_req(st: &Stack, port: u16, i: usize) -> (u16, Duration) {
    st.chat(
        port,
        &if i.is_multiple_of(2) {
            format!("mock-fail {i}")
        } else {
            format!("hi {i}")
        },
    )
}

/// Request `i` to a memory port: every 2nd one is `POST /seed` (501, one error signal).
fn memory_req(port: u16, i: usize) -> (u16, Duration) {
    let (path, body) = if i.is_multiple_of(2) {
        ("/seed", "{}")
    } else {
        (
            "/recall",
            r#"{"query":"t33 back-pressure","session_key":"s1"}"#,
        )
    };
    let (c, _, d) = http(
        port,
        "POST",
        path,
        &[("Content-Type", "application/json")],
        body,
    );
    (c, d)
}

// ---------------------------------------------------------------- latency stats

#[derive(Clone)]
struct Lat {
    n: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    codes: BTreeMap<u16, usize>,
}

impl Lat {
    fn of(v: &[(u16, Duration)]) -> Lat {
        let mut ms: Vec<f64> = v.iter().map(|(_, d)| d.as_secs_f64() * 1e3).collect();
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let at = |q: f64| ms[((ms.len() as f64 * q) as usize).min(ms.len() - 1)];
        let mut codes = BTreeMap::new();
        for (c, _) in v {
            *codes.entry(*c).or_insert(0) += 1;
        }
        Lat {
            n: v.len(),
            p50: at(0.5),
            p95: at(0.95),
            p99: at(0.99),
            max: ms[ms.len() - 1],
            codes,
        }
    }
    fn line(&self) -> String {
        format!(
            "n={} p50={:.2} p95={:.2} p99={:.2} max={:.2} ms codes={:?}",
            self.n, self.p50, self.p95, self.p99, self.max, self.codes
        )
    }
    fn errors(&self) -> u64 {
        self.codes
            .iter()
            .filter(|(c, _)| **c >= 500)
            .map(|(_, n)| *n as u64)
            .sum()
    }
}

/// `n` items through `f`, 4 in parallel.
fn parallel<F>(n: usize, f: F) -> Vec<(u16, Duration)>
where
    F: Fn(usize) -> (u16, Duration) + Send + Sync + 'static,
{
    let f = Arc::new(f);
    let next = Arc::new(Mutex::new(0usize));
    let out = Arc::new(Mutex::new(Vec::with_capacity(n)));
    let hs: Vec<_> = (0..4)
        .map(|_| {
            let (f, next, out) = (f.clone(), next.clone(), out.clone());
            std::thread::spawn(move || loop {
                let i = {
                    let mut g = next.lock().unwrap();
                    *g += 1;
                    *g - 1
                };
                if i >= n {
                    break;
                }
                let r = f(i);
                out.lock().unwrap().push(r);
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    Arc::try_unwrap(out).unwrap().into_inner().unwrap()
}

struct Ab {
    ah: Lat,
    am: Lat,
    bh: Lat,
    bm: Lat,
}

/// `n` requests per stream, A vs B interleaved in 8 rounds (order flipped).
fn ab_load(st: &Arc<Stack>, n: usize, bh: u16, bm: u16) -> Ab {
    let rounds = 8;
    let block = (n / rounds).max(2);
    let mut v: BTreeMap<u16, Vec<(u16, Duration)>> = BTreeMap::new();
    for r in 0..rounds {
        let order = if r % 2 == 0 {
            [(A_HERMES, A_MEMORY), (bh, bm)]
        } else {
            [(bh, bm), (A_HERMES, A_MEMORY)]
        };
        for (h, m) in order {
            let s = st.clone();
            let hv = parallel(block, move |i| hermes_req(&s, h, i));
            let mv = parallel(block, move |i| memory_req(m, i));
            v.entry(h).or_default().extend(hv);
            v.entry(m).or_default().extend(mv);
        }
    }
    Ab {
        ah: Lat::of(&v[&A_HERMES]),
        am: Lat::of(&v[&A_MEMORY]),
        bh: Lat::of(&v[&bh]),
        bm: Lat::of(&v[&bm]),
    }
}

fn check_codes(label: &str, l: &Lat, ok: &[u16]) {
    assert!(
        l.codes.keys().all(|c| ok.contains(c)),
        "{label}: unexpected codes {:?}",
        l.codes
    );
}

/// B must not be slower than A beyond box noise, and never by a nerve wait (5 s / 10 s).
fn check_latency(label: &str, a: &Lat, b: &Lat) {
    let bound = (a.p95 * 3.0).max(a.p95 + 50.0);
    assert!(
        b.p95 <= bound,
        "{label}: p95 B {:.2} ms vs A {:.2} ms",
        b.p95,
        a.p95
    );
    assert!(
        b.max < 2500.0,
        "{label}: a B request took {:.0} ms (waited on the nerve?)",
        b.max
    );
}

// ---------------------------------------------------------------- fake nerve

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Ok = 0,
    Dead = 1,
    Slow = 2,
    R429 = 3,
    Trickle = 4,
    Blackhole = 5,
}

impl Mode {
    const ALL: [Mode; 6] = [
        Mode::Ok,
        Mode::Dead,
        Mode::Slow,
        Mode::R429,
        Mode::Trickle,
        Mode::Blackhole,
    ];
    fn name(self) -> &'static str {
        ["ok", "dead", "slow", "429", "trickle", "blackhole"][self as usize]
    }
}

#[derive(Default)]
struct FakeLog {
    /// Connections accepted per mode.
    conns: BTreeMap<&'static str, u64>,
    /// Signals answered 202 per source.
    accepted: BTreeMap<String, u64>,
    /// Signals answered 429 per source.
    refused: BTreeMap<String, u64>,
    /// (source, received, msg) of every heartbeat answered 202.
    heartbeats: Vec<(String, Instant, String)>,
    /// Longest time a client kept a trickle / slow connection (ms) per mode.
    hold_ms: BTreeMap<&'static str, u64>,
}

struct Fake {
    mode: AtomicU8,
    stop: AtomicBool,
    log: Mutex<FakeLog>,
}

impl Fake {
    fn start() -> Arc<Fake> {
        let f = Arc::new(Fake {
            mode: AtomicU8::new(0),
            stop: AtomicBool::new(false),
            log: Mutex::default(),
        });
        let f2 = f.clone();
        std::thread::spawn(move || f2.run());
        let t = Instant::now();
        while TcpStream::connect(("127.0.0.1", FAKE)).is_err()
            && t.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        f
    }
    fn mode(&self) -> Mode {
        Mode::ALL[self.mode.load(Ordering::SeqCst) as usize]
    }
    fn set(&self, m: Mode) {
        self.mode.store(m as u8, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
    }
    fn run(self: Arc<Self>) {
        let mut l: Option<TcpListener> = None;
        while !self.stop.load(Ordering::SeqCst) {
            if self.mode() == Mode::Dead {
                l = None; // port closed: connects are refused
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            if l.is_none() {
                match TcpListener::bind(("127.0.0.1", FAKE)) {
                    Ok(x) => {
                        x.set_nonblocking(true).unwrap();
                        l = Some(x);
                    }
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                }
            }
            match l.as_ref().unwrap().accept() {
                Ok((s, _)) => {
                    let m = self.mode();
                    let me = self.clone();
                    std::thread::spawn(move || me.handle(s, m));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
    fn handle(&self, mut s: TcpStream, m: Mode) {
        let _ = s.set_nonblocking(false);
        *self.log.lock().unwrap().conns.entry(m.name()).or_insert(0) += 1;
        if m == Mode::Blackhole {
            std::thread::sleep(Duration::from_secs(10));
            return;
        }
        let body = read_signal(&mut s);
        let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let src = v["source"].as_str().unwrap_or("?").to_string();
        let t = Instant::now();
        match m {
            Mode::Ok => {
                {
                    let mut g = self.log.lock().unwrap();
                    *g.accepted.entry(src.clone()).or_insert(0) += 1;
                    if v["kind"] == "heartbeat" {
                        g.heartbeats.push((
                            src,
                            Instant::now(),
                            v["msg"].as_str().unwrap_or("").into(),
                        ));
                    }
                }
                let _ = s
                    .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 11\r\n\r\n{\"ok\":true}");
            }
            Mode::R429 => {
                *self.log.lock().unwrap().refused.entry(src).or_insert(0) += 1;
                let _ = s.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n");
            }
            Mode::Slow => {
                std::thread::sleep(Duration::from_secs(5));
                let _ = s.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n");
            }
            Mode::Trickle => {
                for _ in 0..64 {
                    if s.write_all(b"H").is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                let ms = t.elapsed().as_millis() as u64;
                let mut g = self.log.lock().unwrap();
                let e = g.hold_ms.entry("trickle").or_insert(0);
                *e = (*e).max(ms);
            }
            Mode::Dead | Mode::Blackhole => {}
        }
    }
    /// Last heartbeat counters per source received after `since`.
    fn heartbeat_after(&self, src: &str, since: Instant) -> Option<Counters> {
        let g = self.log.lock().unwrap();
        g.heartbeats
            .iter()
            .rev()
            .find(|(s, t, _)| s == src && *t >= since)
            .map(|(_, _, m)| Counters::parse(m))
    }
    fn accepted(&self, src: &str) -> u64 {
        self.log
            .lock()
            .unwrap()
            .accepted
            .get(src)
            .copied()
            .unwrap_or(0)
    }
}

/// Request head + Content-Length body (2 s budget).
fn read_signal(s: &mut TcpStream) -> String {
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let mut got = Vec::new();
    let mut buf = [0u8; 2048];
    let t = Instant::now();
    loop {
        if let Some(i) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&got[..i]).to_ascii_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            if got.len() >= i + 4 + len {
                return String::from_utf8_lossy(&got[i + 4..i + 4 + len]).into_owned();
            }
        }
        if t.elapsed() > Duration::from_secs(2) {
            return String::new();
        }
        match s.read(&mut buf) {
            Ok(0) | Err(_) => return String::new(),
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Counters {
    queued: u64,
    sent: u64,
    dropped: u64,
    failed: u64,
    rejected: u64,
}

impl Counters {
    fn parse(msg: &str) -> Counters {
        let get = |k: &str| {
            msg.split_whitespace()
                .find_map(|kv| kv.strip_prefix(k))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        Counters {
            queued: get("queued="),
            sent: get("sent="),
            dropped: get("dropped="),
            failed: get("failed="),
            rejected: get("rejected="),
        }
    }
    fn minus(&self, o: &Counters) -> Counters {
        Counters {
            queued: self.queued - o.queued,
            sent: self.sent - o.sent,
            dropped: self.dropped - o.dropped,
            failed: self.failed - o.failed,
            rejected: self.rejected - o.rejected,
        }
    }
    fn attempted(&self) -> u64 {
        self.queued + self.dropped
    }
    /// Signals taken from the queue but not yet resolved (must be ~0 when idle).
    fn unresolved(&self) -> i64 {
        self.queued as i64 - (self.sent + self.failed + self.rejected) as i64
    }
}

/// Wait for a heartbeat from `src` that left the client at least 2 s after `since`.
fn fresh_heartbeat(f: &Fake, src: &str, since: Instant) -> Counters {
    let from = since + Duration::from_secs(2);
    let t = Instant::now();
    loop {
        if let Some(c) = f.heartbeat_after(src, from) {
            return c;
        }
        assert!(
            t.elapsed() < Duration::from_secs(3 * HB_S),
            "no heartbeat from {src} after {:?}",
            since
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn expect_deadline() -> bool {
    std::env::var("T33_EXPECT_DEADLINE").as_deref() != Ok("0")
}

fn real() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run the real-life back-pressure test");
        return false;
    }
    true
}

fn latest_real_heartbeat(sig: &Value, src: &str) -> Option<String> {
    let s = sig["sources"]
        .as_array()?
        .iter()
        .find(|s| s["source"] == src)?
        .clone();
    s["recent"]
        .as_array()?
        .iter()
        .rev()
        .find(|e| e["kind"] == "heartbeat")
        .and_then(|e| e["msg"].as_str().map(String::from))
}

fn real_source_dropped(sig: &Value, src: &str) -> u64 {
    sig["sources"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["source"] == src))
        .and_then(|s| s["dropped"].as_u64())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- box phases

#[test]
fn nerve_backpressure_box() {
    if !real() {
        return;
    }
    let n: usize = std::env::var("N_REQ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    let st = Stack::new("box");
    let fake = Fake::start();
    let ah = st.hermes(A_HERMES, None);
    let am = st.memory(A_MEMORY, None);
    let bh = st.hermes(B_HERMES, Some(FAKE));
    let bm = st.memory(B_MEMORY, Some(FAKE));
    for p in [A_HERMES, A_MEMORY, B_HERMES, B_MEMORY] {
        wait_up(p, "/health");
    }
    println!(
        "== {} T3.3 box test, {n} requests per stream per phase, bins {}",
        stamp(),
        st.bin.display()
    );
    let srcs = ["hermes-rs", "openmind-memory"];
    let t0 = Instant::now();
    let mut prev: BTreeMap<&str, Counters> = srcs
        .iter()
        .map(|s| (*s, fresh_heartbeat(&fake, s, t0 - Duration::from_secs(2))))
        .collect();
    let mut rows = Vec::new();
    let order = [
        Mode::Ok,
        Mode::Dead,
        Mode::Slow,
        Mode::R429,
        Mode::Trickle,
        Mode::Blackhole,
        Mode::Ok,
    ];
    for (k, m) in order.into_iter().enumerate() {
        let acc0: BTreeMap<&str, u64> = srcs.iter().map(|s| (*s, fake.accepted(s))).collect();
        fake.set(m);
        let tp = Instant::now();
        println!("== {} PHASE {} ({})", stamp(), k, m.name());
        let ab = ab_load(&st, n, B_HERMES, B_MEMORY);
        let took = tp.elapsed();
        println!("  hermes A: {}\n  hermes B: {}", ab.ah.line(), ab.bh.line());
        println!("  memory A: {}\n  memory B: {}", ab.am.line(), ab.bm.line());
        for (lab, l) in [("hermes A", &ab.ah), ("hermes B", &ab.bh)] {
            check_codes(&format!("{} {lab}", m.name()), l, &[200, 429, 502]);
        }
        for (lab, l) in [("memory A", &ab.am), ("memory B", &ab.bm)] {
            check_codes(&format!("{} {lab}", m.name()), l, &[200, 501]);
        }
        check_latency(&format!("{} hermes", m.name()), &ab.ah, &ab.bh);
        check_latency(&format!("{} memory", m.name()), &ab.am, &ab.bm);
        fake.set(Mode::Ok);
        let back = Instant::now();
        for (src, errs) in [
            ("hermes-rs", ab.bh.errors()),
            ("openmind-memory", ab.bm.errors()),
        ] {
            let now = fresh_heartbeat(&fake, src, back);
            let d = now.minus(&prev[src]);
            let window = back.duration_since(tp).as_secs() + 3 * HB_S;
            let hb_slack = 2 * (window / HB_S + 2);
            println!(
                "  {src}: pushed errors {errs}; client delta queued={} sent={} dropped={} failed={} rejected={} (unresolved {}); fake accepted {}",
                d.queued, d.sent, d.dropped, d.failed, d.rejected, now.unresolved(),
                fake.accepted(src) - acc0[src]
            );
            // A pre-T3.3 build (no POST deadline) is held ~16 s by one trickling
            // reply, so its heartbeat lags the backlog: reported, not asserted.
            let strict = m != Mode::Trickle || expect_deadline();
            if !strict {
                println!("  (pre-T3.3 build: worker held by the trickling nerve; accounting not asserted)");
                prev.insert(src, now);
                continue;
            }
            assert!(
                d.attempted() >= errs && d.attempted() <= errs + hb_slack,
                "{} {src}: attempted {} vs pushed errors {errs}",
                m.name(),
                d.attempted()
            );
            assert!(
                (0..=2).contains(&now.unresolved()),
                "{} {src}: {now:?}",
                m.name()
            );
            let lost = d.dropped + d.failed + d.rejected;
            match m {
                Mode::Ok => assert!(d.sent > 0, "{src} ok: nothing sent {d:?}"),
                Mode::Dead | Mode::Slow | Mode::Trickle | Mode::Blackhole => {
                    assert!(
                        lost + hb_slack >= errs,
                        "{} {src}: lost {lost} < {errs}",
                        m.name()
                    );
                    assert!(d.failed > 0, "{} {src}: no failure counted {d:?}", m.name());
                }
                Mode::R429 => assert!(d.rejected > 0, "{src} 429: nothing rejected {d:?}"),
            }
            rows.push(format!(
                "| {} | {src} | {errs} | {} | {} | {} | {} | {} |",
                m.name(),
                d.queued,
                d.sent,
                d.dropped,
                d.failed,
                d.rejected
            ));
            prev.insert(src, now);
        }
        println!(
            "  phase took {:.1} s; B-A p99 hermes {:+.2} ms, memory {:+.2} ms",
            took.as_secs_f64(),
            ab.bh.p99 - ab.ah.p99,
            ab.bm.p99 - ab.am.p99
        );
        rows.push(format!(
            "| {} | latency ms p50/p95/p99/max | hermes A {:.2}/{:.2}/{:.2}/{:.2} B {:.2}/{:.2}/{:.2}/{:.2} | memory A {:.2}/{:.2}/{:.2}/{:.2} B {:.2}/{:.2}/{:.2}/{:.2} | | | |",
            m.name(),
            ab.ah.p50, ab.ah.p95, ab.ah.p99, ab.ah.max, ab.bh.p50, ab.bh.p95, ab.bh.p99, ab.bh.max,
            ab.am.p50, ab.am.p95, ab.am.p99, ab.am.max, ab.bm.p50, ab.bm.p95, ab.bm.p99, ab.bm.max
        ));
    }
    // Burst: memory-only flood of fast 501s while the nerve accepts and never
    // answers, so more signals arrive during one held POST than the queue
    // (256) holds: the overflow must be dropped AND counted.
    let burst: usize = n * 10;
    fake.set(Mode::Blackhole);
    println!(
        "== {} PHASE burst (blackhole; {burst} x POST /seed to memory A, then B)",
        stamp()
    );
    let tb = Instant::now();
    let la = Lat::of(&parallel(burst, |_| memory_req(A_MEMORY, 0)));
    let lb = Lat::of(&parallel(burst, |_| memory_req(B_MEMORY, 0)));
    println!("  memory A: {}\n  memory B: {}", la.line(), lb.line());
    check_codes("burst memory A", &la, &[501]);
    check_codes("burst memory B", &lb, &[501]);
    check_latency("burst memory", &la, &lb);
    fake.set(Mode::Ok);
    let back = Instant::now();
    let now = fresh_heartbeat(&fake, "openmind-memory", back);
    let d = now.minus(&prev["openmind-memory"]);
    let errs = lb.errors();
    let hb_slack = 2 * ((back.duration_since(tb).as_secs() + 3 * HB_S) / HB_S + 2);
    println!(
        "  openmind-memory: pushed errors {errs}; client delta queued={} sent={} dropped={} failed={} rejected={} (unresolved {})",
        d.queued, d.sent, d.dropped, d.failed, d.rejected, now.unresolved()
    );
    assert!(
        d.attempted() >= errs && d.attempted() <= errs + hb_slack,
        "burst: attempted {} vs {errs}",
        d.attempted()
    );
    assert!((0..=2).contains(&now.unresolved()), "burst: {now:?}");
    assert!(d.dropped > 0, "burst: queue never overflowed {d:?}");
    rows.push(format!(
        "| burst | openmind-memory | {errs} | {} | {} | {} | {} | {} |",
        d.queued, d.sent, d.dropped, d.failed, d.rejected
    ));
    rows.push(format!(
        "| burst | latency ms p50/p95/p99/max | | memory A {:.2}/{:.2}/{:.2}/{:.2} B {:.2}/{:.2}/{:.2}/{:.2} | | | |",
        la.p50, la.p95, la.p99, la.max, lb.p50, lb.p95, lb.p99, lb.max
    ));
    prev.insert("openmind-memory", now);

    let hold = fake
        .log
        .lock()
        .unwrap()
        .hold_ms
        .get("trickle")
        .copied()
        .unwrap_or(0);
    println!("-- longest client hold on a trickling nerve: {hold} ms (POST_DEADLINE 1000 ms)");
    if expect_deadline() {
        assert!(hold > 0 && hold < 2000, "trickle hold {hold} ms");
    }
    println!(
        "-- fake nerve connections per mode: {:?}",
        fake.log.lock().unwrap().conns
    );
    for (lab, p) in [
        ("hermes A", &ah),
        ("hermes B", &bh),
        ("memory A", &am),
        ("memory B", &bm),
    ] {
        println!(
            "-- {lab}: RSS {} kB, threads {}, fds {}",
            proc_field(p.pid(), "VmRSS:"),
            proc_field(p.pid(), "Threads:"),
            fds(p.pid())
        );
    }
    println!("-- table\n| phase | source | pushed errors | queued | sent | dropped | failed | rejected |\n|---|---|---|---|---|---|---|---|");
    for r in &rows {
        println!("{r}");
    }

    // Real nerve: its own per-source limit (120/min) answers 429.
    println!(
        "== {} PHASE real-429: real openmind-nerve, per-source limit 120/min",
        stamp()
    );
    let nerve = st.nerve(&[("hermes-rs", R_HERMES), ("openmind-memory", R_MEMORY)]);
    wait_up(REAL_NERVE, "/healthz");
    let rh = st.hermes(R_HERMES, Some(REAL_NERVE));
    let rm = st.memory(R_MEMORY, Some(REAL_NERVE));
    wait_up(R_HERMES, "/health");
    wait_up(R_MEMORY, "/health");
    let tflood = Instant::now();
    let ab = ab_load(&st, n, R_HERMES, R_MEMORY);
    println!("  hermes A: {}\n  hermes R: {}", ab.ah.line(), ab.bh.line());
    println!("  memory A: {}\n  memory R: {}", ab.am.line(), ab.bm.line());
    check_codes("real hermes R", &ab.bh, &[200, 429, 502]);
    check_codes("real memory R", &ab.bm, &[200, 501]);
    check_latency("real hermes", &ab.ah, &ab.bh);
    check_latency("real memory", &ab.am, &ab.bm);
    // The per-source window is 60 s: heartbeats are refused too until it frees.
    for (src, errs) in [
        ("hermes-rs", ab.bh.errors()),
        ("openmind-memory", ab.bm.errors()),
    ] {
        let t = Instant::now();
        let hb = loop {
            let sig = get_json(REAL_NERVE, "/api/v1/signals");
            if let Some(m) = latest_real_heartbeat(&sig, src) {
                let c = Counters::parse(&m);
                if c.rejected > 0 {
                    break (c, sig, t.elapsed());
                }
            }
            assert!(
                t.elapsed() < Duration::from_secs(120),
                "no post-flood heartbeat from {src}"
            );
            std::thread::sleep(Duration::from_secs(2));
        };
        let (c, sig, waited) = hb;
        let nd = real_source_dropped(&sig, src);
        println!(
            "  {src}: pushed errors {errs}; client queued={} sent={} dropped={} failed={} rejected={}; nerve rate-limited {nd}; first heartbeat through {:.0} s after the flood started",
            c.queued, c.sent, c.dropped, c.failed, c.rejected,
            (waited + t.duration_since(tflood)).as_secs_f64()
        );
        assert!(c.attempted() >= errs, "{src}: {c:?}");
        assert!(c.rejected > 0 && c.sent >= 1);
        // Both sides count the same refusals (the nerve may have refused a few more since).
        assert!(
            nd >= c.rejected && nd <= c.rejected + 8,
            "{src}: nerve {nd} vs client {}",
            c.rejected
        );
    }
    let h = get_json(REAL_NERVE, "/health");
    println!(
        "-- real nerve: cycle {} RSS {} kB; R hermes RSS {} kB threads {}; R memory RSS {} kB threads {}",
        h["cycle"],
        proc_field(nerve.pid(), "VmRSS:"),
        proc_field(rh.pid(), "VmRSS:"),
        proc_field(rh.pid(), "Threads:"),
        proc_field(rm.pid(), "VmRSS:"),
        proc_field(rm.pid(), "Threads:")
    );
    println!("== {} done; tearing down", stamp());
    fake.stop.store(true, Ordering::SeqCst);
    drop((nerve, rh, rm, ah, am, bh, bm));
    let _ = std::fs::remove_dir_all(&st.work);
}

// ---------------------------------------------------------------- soak

/// (half-minute index, per process (RSS kB, threads, fds), nerve cycle, nerve RSS kB).
type Sample = (u64, Vec<(u64, u64, u64)>, u64, u64);
/// Replies per mode: hermes A, hermes B, memory A, memory B.
type Replies = Vec<(u16, Duration)>;
type ModeLat = (Replies, Replies, Replies, Replies);

#[derive(Default)]
struct Bucket {
    v: BTreeMap<u16, Vec<(u16, Duration)>>,
}

#[test]
fn nerve_backpressure_soak() {
    if !real() {
        return;
    }
    let mins: u64 = std::env::var("T33_SOAK_MIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
        .clamp(2, 30);
    let st = Stack::new("soak");
    let fake = Fake::start();
    let nerve = st.nerve(&[("hermes-rs", B_HERMES), ("openmind-memory", B_MEMORY)]);
    let procs = [
        ("hermes A", st.hermes(A_HERMES, None)),
        ("hermes B", st.hermes(B_HERMES, Some(FAKE))),
        ("memory A", st.memory(A_MEMORY, None)),
        ("memory B", st.memory(B_MEMORY, Some(FAKE))),
    ];
    for p in [A_HERMES, A_MEMORY, B_HERMES, B_MEMORY] {
        wait_up(p, "/health");
    }
    wait_up(REAL_NERVE, "/healthz");
    println!(
        "== {} T3.3 soak {mins} min; modes cycle every 60 s; bins {}",
        stamp(),
        st.bin.display()
    );
    let end = Instant::now() + Duration::from_secs(mins * 60);
    let stop = Arc::new(AtomicBool::new(false));
    let bucket: Arc<Mutex<Bucket>> = Arc::default();
    // 2 threads per port, ~40 req/s per port; half the B requests push a signal.
    let mut hs = Vec::new();
    for port in [A_HERMES, B_HERMES, A_MEMORY, B_MEMORY] {
        for w in 0..2 {
            let (s, stop, b) = (st.clone(), stop.clone(), bucket.clone());
            hs.push(std::thread::spawn(move || {
                let mut i = w;
                while !stop.load(Ordering::SeqCst) {
                    let r = if port == A_HERMES || port == B_HERMES {
                        hermes_req(&s, port, i)
                    } else {
                        memory_req(port, i)
                    };
                    b.lock().unwrap().v.entry(port).or_default().push(r);
                    i += 2;
                    std::thread::sleep(Duration::from_millis(50));
                }
            }));
        }
    }
    let mut minute = 0u64;
    let mut samples: Vec<Sample> = Vec::new();
    let mut per_mode: BTreeMap<&str, ModeLat> = BTreeMap::new();
    let mut total_answered = 0usize;
    while Instant::now() < end {
        let m = Mode::ALL[(minute % 6) as usize];
        fake.set(m);
        for half in 0..2 {
            std::thread::sleep(
                Duration::from_secs(30).min(end.saturating_duration_since(Instant::now())),
            );
            let s: Vec<(u64, u64, u64)> = procs
                .iter()
                .map(|(_, p)| {
                    (
                        proc_field(p.pid(), "VmRSS:"),
                        proc_field(p.pid(), "Threads:"),
                        fds(p.pid()),
                    )
                })
                .collect();
            let h = get_json(REAL_NERVE, "/health");
            let cyc = h["cycle"].as_u64().unwrap_or(0);
            println!(
                "{} min {:>2}.{} mode {:<9} RSS kB hA {} hB {} mA {} mB {} | threads hA {} hB {} mA {} mB {} | fds hA {} hB {} mA {} mB {} | nerve cycle {cyc} RSS {} kB status {}",
                stamp(), minute, half * 5, m.name(),
                s[0].0, s[1].0, s[2].0, s[3].0, s[0].1, s[1].1, s[2].1, s[3].1, s[0].2, s[1].2, s[2].2, s[3].2,
                proc_field(nerve.pid(), "VmRSS:"), h["system"]
            );
            samples.push((minute * 2 + half, s, cyc, proc_field(nerve.pid(), "VmRSS:")));
        }
        let b = std::mem::take(&mut *bucket.lock().unwrap());
        let e = per_mode.entry(m.name()).or_default();
        for (port, v) in b.v {
            total_answered += v.len();
            match port {
                A_HERMES => e.0.extend(v),
                B_HERMES => e.1.extend(v),
                A_MEMORY => e.2.extend(v),
                _ => e.3.extend(v),
            }
        }
        minute += 1;
    }
    stop.store(true, Ordering::SeqCst);
    for h in hs {
        h.join().unwrap();
    }
    fake.set(Mode::Ok);
    let back = Instant::now();
    println!("-- per mode over the soak (ms)");
    for (mode, (ah, bh, am, bm)) in &per_mode {
        let (lah, lbh, lam, lbm) = (Lat::of(ah), Lat::of(bh), Lat::of(am), Lat::of(bm));
        println!(
            "  {mode:<9} hermes A {}\n  {mode:<9} hermes B {}",
            lah.line(),
            lbh.line()
        );
        println!(
            "  {mode:<9} memory A {}\n  {mode:<9} memory B {}",
            lam.line(),
            lbm.line()
        );
        check_codes("soak hermes", &lbh, &[200, 429, 502]);
        check_codes("soak memory", &lbm, &[200, 501]);
        check_codes("soak hermes A", &lah, &[200, 429, 502]);
        check_codes("soak memory A", &lam, &[200, 501]);
        check_latency(&format!("soak {mode} hermes"), &lah, &lbh);
        check_latency(&format!("soak {mode} memory"), &lam, &lbm);
    }
    for src in ["hermes-rs", "openmind-memory"] {
        let c = fresh_heartbeat(&fake, src, back);
        println!(
            "-- {src} client counters after the soak: queued={} sent={} dropped={} failed={} rejected={} (unresolved {})",
            c.queued, c.sent, c.dropped, c.failed, c.rejected, c.unresolved()
        );
        assert!((0..=2).contains(&c.unresolved()), "{src}: {c:?}");
        assert!(c.sent > 0 && c.failed > 0 && c.rejected > 0, "{src}: {c:?}");
    }
    // Growth after warm-up (sample at 2 min) to the end, B vs A.
    let warm = samples
        .iter()
        .find(|s| s.0 >= 4)
        .unwrap_or(&samples[0])
        .clone();
    let last = samples.last().unwrap().clone();
    for (i, (lab, _)) in procs.iter().enumerate() {
        let (r0, t0, f0) = warm.1[i];
        let (r1, t1, f1) = last.1[i];
        println!(
            "-- {lab}: RSS {r0} -> {r1} kB ({:+} kB), threads {t0} -> {t1}, fds {f0} -> {f1}",
            r1 as i64 - r0 as i64
        );
        let _ = (t0, t1);
        assert!(f1 <= f0 + 16, "{lab}: fds grew {f0} -> {f1}");
    }
    // nerve-client cost = B growth minus A growth (A runs the same build and
    // traffic with NERVE_URL unset), so service-own growth cancels out.
    for (a, b) in [(0, 1), (2, 3)] {
        let g = |i: usize| last.1[i].0 as i64 - warm.1[i].0 as i64;
        let slack = (warm.1[a].0 as i64 / 10).max(4096);
        println!(
            "-- RSS growth after warm-up: {} {:+} kB vs {} {:+} kB (bound +{slack} kB)",
            procs[b].0,
            g(b),
            procs[a].0,
            g(a)
        );
        assert!(
            g(b) <= g(a) + slack,
            "{}: RSS grew {:+} kB vs {} {:+} kB",
            procs[b].0,
            g(b),
            procs[a].0,
            g(a)
        );
    }
    for (a, b) in [(0, 1), (2, 3)] {
        assert!(
            last.1[b].1 <= last.1[a].1 + 4,
            "B threads {} vs A {}",
            last.1[b].1,
            last.1[a].1
        );
    }
    println!(
        "-- {} requests answered; fake nerve connections per mode {:?}; real nerve cycles {} -> {}, RSS {} -> {} kB",
        total_answered,
        fake.log.lock().unwrap().conns,
        samples[0].2,
        last.2,
        samples[0].3,
        last.3
    );
    assert!(last.2 > samples[0].2, "real nerve did not cycle");
    println!("== {} soak done; tearing down", stamp());
    fake.stop.store(true, Ordering::SeqCst);
    drop(procs);
    drop(nerve);
    let _ = std::fs::remove_dir_all(&st.work);
}
