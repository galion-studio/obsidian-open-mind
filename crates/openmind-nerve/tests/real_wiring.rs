// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life wiring test, Rust only (gated: `GALION_REAL=1`; wave-4 slot
//! 101, ports 19710-19719). Replaces `scripts/real/nerve-wiring.sh`.
//!
//! It builds (release) and starts the real binaries on the box, offline and
//! with no spend (hermes-gateway runs `--mock`):
//!
//! | port  | process |
//! |-------|---------|
//! | 19710 | openmind-nerve serve |
//! | 19711 | hermes-gateway --mock, `NERVE_URL` set |
//! | 19712 | openmind-memory, `NERVE_URL` set |
//! | 19713 | baseline hermes-gateway --mock, `NERVE_URL` unset (phase 0 only) |
//! | 19714 | baseline openmind-memory, `NERVE_URL` unset (phase 0 only) |
//! | 19715 | stray openmind-memory (no unit, not in the table) -> "unknown owner" |
//!
//! Phases: baseline without the nerve; start-up events; ownership map;
//! load with 5xx + slow requests; p95 with the nerve up, hung (SIGSTOP)
//! and down (killed); nerve restart and client counters; hermes killed ->
//! probe-down incident with its Gaia owner; hermes restarted with a low
//! call cap -> spend-cap event + incident. Every process is killed on drop.
//!
//! Knobs: `N_REQ` (default 300) requests per stream and phase;
//! `NERVE_WIRING_BIN_DIR` = a directory that already holds the three
//! release binaries (skips the build). Otherwise the test runs
//! `cargo build --release` into `<target>/real-nerve-wiring` (a separate
//! target dir, so it never waits on the outer `cargo test` lock).
//! Run with `--nocapture` to see the report.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NERVE: u16 = 19710;
const HERMES: u16 = 19711;
const MEMORY: u16 = 19712;
const BASE_HERMES: u16 = 19713;
const BASE_MEMORY: u16 = 19714;
const STRAY: u16 = 19715;

/// A started process; killed (after SIGCONT, in case it was stopped) on drop.
struct Proc(Child);
impl Proc {
    fn pid(&self) -> u32 {
        self.0.id()
    }
    fn signal(&self, sig: &str) {
        let _ = Command::new("kill")
            .args([sig, &self.pid().to_string()])
            .status();
    }
}
impl Drop for Proc {
    fn drop(&mut self) {
        self.signal("-CONT");
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

/// One HTTP/1.0 request; returns (status, body, elapsed). Status 0 = no answer.
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
    let (_, b, _) = http(port, "GET", path, &[], "");
    serde_json::from_str(&b).unwrap_or(Value::Null)
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

fn sleep_s(s: u64) {
    std::thread::sleep(Duration::from_secs(s));
}

struct Stack {
    bin: PathBuf,
    work: PathBuf,
    key: String,
}

impl Stack {
    fn nerve(&self) -> Proc {
        let p = Command::new(self.bin.join("openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{NERVE}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", self.work.join("data"))
            .env("NERVE_COMPONENTS", self.work.join("components.json"))
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "19710-19719")
            .env("NERVE_UNOWNED_PORTS", STRAY.to_string())
            .stdout(Stdio::null())
            .stderr(self.log("nerve.log"))
            .spawn()
            .expect("spawn openmind-nerve");
        Proc(p)
    }

    /// hermes-gateway --mock; `nerve` = push to the nerve or not.
    fn hermes(&self, port: u16, cap: u64, home: &str, nerve: bool) -> Proc {
        let mut c = Command::new(self.bin.join("hermes-gateway"));
        c.arg("--mock")
            .env_remove("NERVE_URL")
            .env("API_SERVER_KEY", &self.key)
            .env("HERMES_HOME", self.work.join(home))
            .env("HERMES_RS_PORT", port.to_string())
            .env("HERMES_RS_MCP", "0")
            .env("HERMES_RS_DAILY_CALL_CAP", cap.to_string())
            .env("HERMES_RS_MOCK_DELAY_MS", "1500")
            .stdout(Stdio::null())
            .stderr(self.log(&format!("{home}.log")));
        if nerve {
            c.env("NERVE_URL", format!("http://127.0.0.1:{NERVE}"))
                .env("NERVE_QUEUE", "8")
                .env("NERVE_SLOW_MS", "1000")
                .env("NERVE_HEARTBEAT_S", "10");
        }
        Proc(c.spawn().expect("spawn hermes-gateway"))
    }

    fn memory(&self, port: u16, dir: &str, nerve: bool) -> Proc {
        let mut c = Command::new(self.bin.join("openmind-memory"));
        c.env_remove("NERVE_URL")
            .env("OPENMIND_MEMORY_PORT", port.to_string())
            .env("OPENMIND_MEMORY_DATA_DIR", self.work.join(dir))
            .stdout(Stdio::null())
            .stderr(self.log(&format!("{dir}.log")));
        if nerve {
            c.env("NERVE_URL", format!("http://127.0.0.1:{NERVE}"))
                .env("NERVE_SLOW_MS", "1000")
                .env("NERVE_HEARTBEAT_S", "10");
        }
        Proc(c.spawn().expect("spawn openmind-memory"))
    }

    fn log(&self, name: &str) -> Stdio {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.work.join(name))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null())
    }

    fn chat(&self, port: u16, text: &str) -> (u16, Duration) {
        let body = serde_json::json!({
            "model": "hermes-agent",
            "messages": [{"role": "user", "content": text}]
        })
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

fn recall(port: u16) -> (u16, Duration) {
    let (c, _, d) = http(
        port,
        "POST",
        "/recall",
        &[("Content-Type", "application/json")],
        r#"{"query":"nerve wiring test","session_key":"s1"}"#,
    );
    (c, d)
}

/// Latency summary of one stream in one phase.
struct Stats {
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    codes: BTreeMap<u16, usize>,
}

impl Stats {
    fn of(v: &[(u16, Duration)]) -> Stats {
        let mut ms: Vec<f64> = v.iter().map(|(_, d)| d.as_secs_f64() * 1e3).collect();
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let at = |q: f64| ms[((ms.len() as f64 * q) as usize).min(ms.len() - 1)];
        let mut codes = BTreeMap::new();
        for (c, _) in v {
            *codes.entry(*c).or_insert(0) += 1;
        }
        Stats {
            p50: at(0.5),
            p95: at(0.95),
            p99: at(0.99),
            max: ms[ms.len() - 1],
            codes,
        }
    }
    fn line(&self, n: usize) -> String {
        format!(
            "n={n} p50={:.2}ms p95={:.2}ms p99={:.2}ms max={:.2}ms codes={:?}",
            self.p50, self.p95, self.p99, self.max, self.codes
        )
    }
}

/// `n` items through `f`, 4 in parallel (like `xargs -P 4`).
fn parallel<F>(n: usize, f: F) -> Vec<(u16, Duration)>
where
    F: Fn(usize) -> (u16, Duration) + Send + Sync + 'static,
{
    let f = Arc::new(f);
    let next = Arc::new(Mutex::new(1usize));
    let out = Arc::new(Mutex::new(Vec::with_capacity(n)));
    let hs: Vec<_> = (0..4)
        .map(|_| {
            let (f, next, out) = (f.clone(), next.clone(), out.clone());
            std::thread::spawn(move || loop {
                let i = {
                    let mut g = next.lock().unwrap();
                    let i = *g;
                    *g += 1;
                    i
                };
                if i > n {
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

/// N chats (every 10th `mock-fail` -> 502) + N recalls; returns (hermes, memory).
fn load(st: &Arc<Stack>, label: &str, n: usize, h: u16, m: u16) -> (Stats, Stats) {
    let s = st.clone();
    let hv = parallel(n, move |i| {
        let text = if i % 10 == 0 {
            "mock-fail".to_string()
        } else {
            format!("hi {i}")
        };
        s.chat(h, &text)
    });
    let mv = parallel(n, move |_| recall(m));
    let (hs, ms) = (Stats::of(&hv), Stats::of(&mv));
    println!("  hermes ({label}): {}", hs.line(hv.len()));
    println!("  memory ({label}): {}", ms.line(mv.len()));
    (hs, ms)
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
}

fn binaries() -> PathBuf {
    if let Some(d) = std::env::var_os("NERVE_WIRING_BIN_DIR") {
        return PathBuf::from(d);
    }
    let td = target_dir().join("real-nerve-wiring");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let ok = Command::new(cargo)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .env("CARGO_TARGET_DIR", &td)
        .args(["build", "-q", "-j", "2", "--release"])
        .args(["-p", "openmind-nerve", "-p", "hermes-gateway"])
        .args(["-p", "openmind-memory"])
        .status()
        .expect("run cargo build")
        .success();
    assert!(ok, "release build failed");
    td.join("release")
}

fn rss_kb(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string())
        })
        .unwrap_or_else(|| "?".into())
}

fn sources(nerve: &Value) -> Vec<Value> {
    nerve["sources"].as_array().cloned().unwrap_or_default()
}

fn source(sig: &Value, name: &str) -> Value {
    sources(sig)
        .into_iter()
        .find(|s| s["source"] == name)
        .unwrap_or(Value::Null)
}

fn heartbeat(src: &Value) -> String {
    src["recent"]
        .as_array()
        .and_then(|r| r.iter().find(|e| e["kind"] == "heartbeat"))
        .and_then(|e| e["msg"].as_str())
        .unwrap_or("")
        .to_string()
}

#[test]
fn nerve_wiring_end_to_end() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run the real-life wiring test");
        return;
    }
    let n: usize = std::env::var("N_REQ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let bin = binaries();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!("nerve-wiring-real-{}-{now}", std::process::id()));
    for d in ["data", "hermes", "mem", "stray", "base-hermes", "base-mem"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(
        work.join("components.json"),
        format!(
            r#"[{{"name":"hermes-rs","port":{HERMES},"path":"/health","gaia":"ARTEMIS","system":"hermes-agents"}},
 {{"name":"openmind-memory","port":{MEMORY},"path":"/health","gaia":"APOLLO","system":"memory-gateway"}}]"#
        ),
    )
    .unwrap();
    // Throwaway box-only key for the mock gateway: not a credential, never printed.
    let st = Arc::new(Stack {
        bin,
        work: work.clone(),
        key: format!("boxtest-{now}-nerve-wiring"),
    });

    // Phase 0: baseline, NERVE_URL unset, no nerve running.
    println!("== {} PHASE base: NERVE_URL unset, no nerve", stamp());
    let base = {
        let _h = st.hermes(BASE_HERMES, 1_000_000, "base-hermes", false);
        let _m = st.memory(BASE_MEMORY, "base-mem", false);
        wait_up(BASE_HERMES, "/health");
        wait_up(BASE_MEMORY, "/health");
        load(&st, "base", n, BASE_HERMES, BASE_MEMORY)
    };

    let mut nerve = st.nerve();
    let mut hermes = st.hermes(HERMES, 1_000_000, "hermes", true);
    let mem = st.memory(MEMORY, "mem", true);
    let stray = st.memory(STRAY, "stray", false);
    wait_up(NERVE, "/healthz");
    sleep_s(12);
    println!(
        "== {} started: nerve {} hermes {} memory {} stray {}",
        stamp(),
        nerve.pid(),
        hermes.pid(),
        mem.pid(),
        stray.pid()
    );

    let sig = get_json(NERVE, "/api/v1/signals");
    println!("-- pushed sources (started events)");
    for s in sources(&sig) {
        println!(
            "{}",
            serde_json::json!({"source": s["source"], "total": s["total"], "dropped": s["dropped"]})
        );
    }
    for (src, port) in [("hermes-rs", HERMES), ("openmind-memory", MEMORY)] {
        let want = format!("listening on 127.0.0.1:{port}");
        let s = source(&sig, src);
        let found = s["recent"].as_array().is_some_and(|r| {
            r.iter()
                .any(|e| e["name"] == "started" && e["msg"].as_str() == Some(want.as_str()))
        });
        assert!(found, "no started event from {src}: {s}");
    }

    let cx = get_json(NERVE, "/api/v1/cortex");
    println!("-- ownership (listen scope 19710-19719)");
    let own = cx["exposure"]["ownership"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for o in &own {
        println!("{o}");
    }
    let stray_row = own
        .iter()
        .find(|o| o["port"] == STRAY)
        .unwrap_or_else(|| panic!("no ownership row for :{STRAY}: {cx}"));
    assert_eq!(stray_row["owner"], "unknown owner", "{stray_row}");
    assert_eq!(stray_row["process"], "openmind-memory", "{stray_row}");
    let by_port = |p: u16| {
        own.iter()
            .find(|o| o["port"] == p)
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(by_port(HERMES)["gaia"], "ARTEMIS");
    assert_eq!(by_port(MEMORY)["gaia"], "APOLLO");
    let unmanaged = cx["vulnerabilities"]
        .as_array()
        .and_then(|v| v.iter().find(|f| f["id"] == "unmanaged-listeners").cloned())
        .unwrap_or_else(|| panic!("no unmanaged-listeners finding: {cx}"));
    println!("-- unmanaged listener finding\n{}", unmanaged["title"]);
    assert!(unmanaged["title"]
        .as_str()
        .unwrap_or("")
        .contains(&format!(":{STRAY}")));

    http(
        MEMORY,
        "POST",
        "/capture",
        &[("Content-Type", "application/json")],
        r#"{"session_key":"s1","messages":[{"role":"user","content":"the nerve wiring test remembers this line"}]}"#,
    );
    println!("== {} PHASE up: nerve running", stamp());
    let up = load(&st, "up", n, HERMES, MEMORY);

    println!("-- 3 slow chats (mock-slow, 1.5 s) and 3 memory 5xx (POST /seed -> 501)");
    let slow: Vec<_> = (1..=3)
        .map(|i| {
            let s = st.clone();
            std::thread::spawn(move || s.chat(HERMES, &format!("mock-slow {i}")))
        })
        .collect();
    for h in slow {
        let (c, d) = h.join().unwrap();
        println!("{c} {:.3}s", d.as_secs_f64());
        assert_eq!(c, 200);
        assert!(d >= Duration::from_millis(1400));
    }
    for _ in 0..3 {
        let (c, _, _) = http(
            MEMORY,
            "POST",
            "/seed",
            &[("Content-Type", "application/json")],
            "{}",
        );
        println!("{c}");
        assert_eq!(c, 501);
    }
    sleep_s(11);
    let inc = get_json(NERVE, "/api/v1/incidents");
    println!("-- incidents (with owning Gaia function)");
    let open = inc["open"].as_array().cloned().unwrap_or_default();
    for i in &open {
        println!(
            "{}",
            serde_json::json!({"severity": i["severity"], "cause": i["cause"], "title": i["title"], "gaia": i["gaia"]})
        );
    }
    let inc_of = |cause: &str| {
        open.iter()
            .find(|i| i["cause"] == cause)
            .cloned()
            .unwrap_or_else(|| panic!("no {cause} incident: {inc}"))
    };
    let hi = inc_of("hermes-rs");
    let mi = inc_of("openmind-memory");
    assert_eq!(hi["gaia"], serde_json::json!(["ARTEMIS"]));
    assert_eq!(mi["gaia"], serde_json::json!(["APOLLO"]));
    let hs = hi.to_string();
    assert!(hs.contains("pushed 3 slow request(s)"), "{hs}");
    assert!(hs.contains("-> 502"), "{hs}");
    assert!(mi.to_string().contains("POST /seed -> 501"), "{mi}");
    let cxi = get_json(NERVE, "/api/v1/cortex")["incidents"].clone();
    println!("-- cortex incident owners\n{cxi}");
    assert!(
        cxi.to_string().contains(r#""system":"hermes-agents""#),
        "{cxi}"
    );
    assert!(
        cxi.to_string().contains(r#""system":"memory-gateway""#),
        "{cxi}"
    );

    println!(
        "== {} PHASE hung: nerve SIGSTOPped (accepts, never answers)",
        stamp()
    );
    nerve.signal("-STOP");
    let hung = load(&st, "hung", n, HERMES, MEMORY);
    nerve.signal("-CONT");
    sleep_s(3);
    println!("== {} PHASE down: nerve killed", stamp());
    drop(nerve);
    let down = load(&st, "down", n, HERMES, MEMORY);

    println!("== {} nerve restarted (fresh state)", stamp());
    nerve = st.nerve();
    wait_up(NERVE, "/healthz");
    sleep_s(12);
    let sig = get_json(NERVE, "/api/v1/signals");
    println!("-- client counters after hung + down phases (first heartbeats to the new nerve)");
    for s in sources(&sig) {
        println!("{} heartbeat: {}", s["source"], heartbeat(&s));
    }
    let hb = heartbeat(&source(&sig, "hermes-rs"));
    assert!(
        hb.contains("failed="),
        "no hermes heartbeat after restart: {sig}"
    );
    let failed: u64 = hb
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("failed="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let dropped: u64 = hb
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("dropped="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    assert!(failed + dropped > 0, "hung/down phases left no trace: {hb}");

    println!("== {} PHASE up2: nerve running again", stamp());
    let up2 = load(&st, "up2", n, HERMES, MEMORY);

    // Requests are answered in every phase; the nerve never gates them.
    for (label, (h, m)) in [
        ("base", &base),
        ("up", &up),
        ("hung", &hung),
        ("down", &down),
        ("up2", &up2),
    ] {
        assert_eq!(m.codes.get(&200), Some(&n), "memory {label}: {:?}", m.codes);
        assert!(
            !h.codes.contains_key(&0),
            "hermes {label} unanswered: {:?}",
            h.codes
        );
        assert!(
            h.codes.get(&200).copied().unwrap_or(0) >= n * 8 / 10,
            "hermes {label}: {:?}",
            h.codes
        );
        let fails = h.codes.get(&502).copied().unwrap_or(0);
        assert!(
            fails + 3 >= n / 10 && fails <= n / 10,
            "hermes {label}: {:?}",
            h.codes
        );
        assert!(
            h.codes.keys().all(|c| [200, 429, 502].contains(c)),
            "hermes {label}: {:?}",
            h.codes
        );
    }
    // Hung/down nerve must not slow requests down: generous bound for a shared box.
    for (label, (h, _)) in [("hung", &hung), ("down", &down)] {
        let bound = (base.0.p95 * 3.0).max(base.0.p95 + 50.0);
        assert!(
            h.p95 <= bound,
            "hermes p95 {label} {:.2} ms vs base {:.2} ms",
            h.p95,
            base.0.p95
        );
    }
    println!(
        "-- p99 vs baseline (NERVE_URL unset): hermes base {:.2} / down {:.2} ms ({:+.1}%), memory base {:.2} / down {:.2} ms",
        base.0.p99,
        down.0.p99,
        (down.0.p99 / base.0.p99 - 1.0) * 100.0,
        base.1.p99,
        down.1.p99
    );

    println!("== {} KILL hermes-gateway (pid {})", stamp(), hermes.pid());
    drop(hermes);
    sleep_s(11);
    let inc = get_json(NERVE, "/api/v1/incidents");
    let down_inc = inc["open"]
        .as_array()
        .and_then(|o| {
            o.iter()
                .find(|i| i["cause"] == "hermes-rs" && i["severity"] == "critical")
                .cloned()
        })
        .unwrap_or_else(|| panic!("no critical hermes-rs incident: {inc}"));
    println!(
        "{}",
        serde_json::json!({"severity": down_inc["severity"], "title": down_inc["title"], "gaia": down_inc["gaia"]})
    );
    assert!(down_inc["title"]
        .as_str()
        .unwrap_or("")
        .contains("health probe down"));
    assert_eq!(down_inc["gaia"], serde_json::json!(["ARTEMIS"]));

    println!(
        "== {} restart hermes with HERMES_RS_DAILY_CALL_CAP=5 (spend file already counts > 5 mock calls)",
        stamp()
    );
    hermes = st.hermes(HERMES, 5, "hermes", true);
    wait_up(HERMES, "/health");
    for i in 1..=6 {
        let (c, d) = st.chat(HERMES, &format!("after cap {i}"));
        println!("{c} {:.4}s", d.as_secs_f64());
        assert_eq!(c, 402);
    }
    sleep_s(11);
    let sig = get_json(NERVE, "/api/v1/signals");
    let hsrc = source(&sig, "hermes-rs");
    let cap_ev = hsrc["recent"]
        .as_array()
        .and_then(|r| r.iter().find(|e| e["name"] == "spend_cap").cloned())
        .unwrap_or_else(|| panic!("no spend_cap event: {hsrc}"));
    println!("-- hermes spend_cap event\n{cap_ev}");
    let inc = get_json(NERVE, "/api/v1/incidents");
    let cap_inc = inc["open"]
        .as_array()
        .and_then(|o| {
            o.iter()
                .find(|i| i["cause"] == "spend-cap:hermes-rs")
                .cloned()
        })
        .unwrap_or_else(|| panic!("no spend-cap incident: {inc}"));
    println!(
        "{}",
        serde_json::json!({"severity": cap_inc["severity"], "title": cap_inc["title"], "gaia": cap_inc["gaia"]})
    );
    assert!(cap_inc["title"]
        .as_str()
        .unwrap_or("")
        .contains("spend cap hit (provider calls refused)"));
    assert_eq!(cap_inc["gaia"], serde_json::json!(["ARTEMIS"]));
    let cxi = get_json(NERVE, "/api/v1/cortex")["incidents"].to_string();
    assert!(cxi.contains("spend-cap:hermes-rs"), "{cxi}");

    println!("-- client queue counters (10 s heartbeats)");
    for s in sources(&sig) {
        println!("{} heartbeat: {}", s["source"], heartbeat(&s));
    }
    println!(
        "-- RSS kB: nerve {} hermes {} memory {} stray {}",
        rss_kb(nerve.pid()),
        rss_kb(hermes.pid()),
        rss_kb(mem.pid()),
        rss_kb(stray.pid())
    );
    println!("== {} done; tearing down", stamp());
    drop((nerve, hermes, mem, stray));
    let _ = std::fs::remove_dir_all(&work);
}
