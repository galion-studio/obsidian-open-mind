// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL fault-injection scenarios (GALION_REAL=1), box only, for Gaia S3.2
//! (wave-4 slot 189, gaia-fault-inject) on gaia-loop-decisions b40208a.
//!
//! Every scenario runs the real release `gaia` binary (or, for the DST
//! clock, `gaia::live_heal::heal_tick_at` in-process with a fake wall
//! clock) against real `gaia-dummy` fault services, real hades-gate tokens
//! and ledgers, and a nerve: a programmable fake nerve on 127.0.0.1:20590
//! (this test's own thread) or the real `openmind-nerve serve` on
//! 127.0.0.1:20591 (GAIA_NERVE_BIN). Faults: hang, slow /health, bad JSON
//! (8 bodies), flapping (two concurrent loops), port stolen (answering and
//! hung thief), nerve down and back, nerve partial faults, the 2026-10-25
//! CEST -> CET switch on a fake clock, corrupt ledgers, disabled gate.
//! Invariants checked everywhere: at most 3 restarts per service per rolling
//! hour (global ledger), Gaia never signals a process, no restart without
//! the nerve, no restart without a consumed signed token, and every setup
//! fault fails closed (no spawn).
//!
//! Ports (slot 199 moved them, D-FI4): nerves 20590-20591 and fault services
//! 20592-20599 in slot 189's band 20590-20599; three more fault services
//! (20690-20692) in slot 199's band 20690-20699. Each tick allows only its
//! own port; live VPS ports (`gaia_live::heal::LIVE_PORTS`) are refused.
//!
//! Run: `GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve cargo test
//! --release -p gaia --test real_gaia_faults -- --nocapture --test-threads 1`.

use gaia::live_heal::{self, HealCfg};
use gaia_live::heal::TestTarget;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

const FAKE: &str = "127.0.0.1:20590";
const REAL: &str = "127.0.0.1:20591";
const P_HANG: u16 = 20592;
const P_SLOW: u16 = 20593;
const P_BAD: u16 = 20594;
const P_FLAP: u16 = 20595;
const P_STOLEN: u16 = 20596;
const P_STOLEN_HANG: u16 = 20597;
const P_NERVE: u16 = 20598;
const P_DST: u16 = 20599;
const P_NFAULT: u16 = 20690;
const P_LEDGER: u16 = 20691;
const P_UNREC: u16 = 20692;
const TARGET_PORTS: [u16; 11] = [
    P_HANG,
    P_SLOW,
    P_BAD,
    P_FLAP,
    P_STOLEN,
    P_STOLEN_HANG,
    P_NERVE,
    P_DST,
    P_NFAULT,
    P_LEDGER,
    P_UNREC,
];

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

fn real_on() -> bool {
    std::env::var("GALION_REAL").ok().as_deref() == Some("1")
}

// ------------------------------------------------------------ fake nerve

#[derive(Default)]
struct Faults {
    /// /state answers this (code, body).
    state: (u16, String),
    /// After this many further /state GETs, answer 500.
    state_ok_left: Option<u64>,
    /// POST /event of this kind answers 500.
    post_fail_kind: Option<String>,
    /// /events never contains `service.probe` events.
    drop_probe: bool,
    /// /health answers 503.
    health_down: bool,
}

#[derive(Clone, Default)]
struct Fake {
    log: Arc<Mutex<Vec<Value>>>,
    f: Arc<Mutex<Faults>>,
    state_gets: Arc<Mutex<u64>>,
}

impl Fake {
    fn reset(&self) {
        *self.f.lock().unwrap() = Faults {
            state: (200, json!({"cycle": 7, "components": []}).to_string()),
            ..Faults::default()
        };
    }
    fn faults(&self) -> MutexGuard<'_, Faults> {
        self.f.lock().unwrap()
    }
    fn events(&self, svc: &str, kind: &str) -> Vec<Value> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == kind && e["data"]["service"] == svc)
            .cloned()
            .collect()
    }
    fn count(&self, svc: &str, kind: &str) -> usize {
        self.events(svc, kind).len()
    }
}

fn serve_fake(l: TcpListener, f: Fake) {
    for s in l.incoming() {
        let Ok(mut s) = s else { continue };
        let Ok(c) = s.try_clone() else { continue };
        let mut r = BufReader::new(c);
        let mut first = String::new();
        if r.read_line(&mut first).is_err() {
            continue;
        }
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; len];
        let _ = r.read_exact(&mut body);
        let (code, resp) = if first.starts_with("GET /health") {
            if f.faults().health_down {
                (503, r#"{"status":"warming_up"}"#.to_string())
            } else {
                (200, r#"{"status":"ok"}"#.to_string())
            }
        } else if first.starts_with("GET /state") {
            *f.state_gets.lock().unwrap() += 1;
            let mut g = f.faults();
            match g.state_ok_left {
                Some(0) => (500, r#"{"error":"injected /state fault"}"#.to_string()),
                Some(n) => {
                    g.state_ok_left = Some(n - 1);
                    g.state.clone()
                }
                None => g.state.clone(),
            }
        } else if first.starts_with("POST /event") {
            let mut v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let fail = f.faults().post_fail_kind.clone();
            if fail.is_some_and(|k| v["kind"] == k.as_str()) {
                v["injected_fault"] = Value::from("post-500");
                v["ts"] = Value::from(stamp());
                f.log.lock().unwrap().push(v);
                (500, r#"{"error":"injected POST fault"}"#.to_string())
            } else {
                v["ts"] = Value::from(stamp());
                f.log.lock().unwrap().push(v);
                (200, r#"{"ok":true}"#.to_string())
            }
        } else {
            let drop = f.faults().drop_probe;
            let all: Vec<Value> = f
                .log
                .lock()
                .unwrap()
                .iter()
                .filter(|e| !(drop && e["kind"] == "service.probe"))
                .cloned()
                .collect();
            (200, Value::Array(all).to_string())
        };
        let _ = write!(
            s,
            "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
            resp.len()
        );
    }
}

// ------------------------------------------------------------- shared env

struct Env {
    fake: Fake,
    dir: PathBuf,
    config: String,
    seed: String,
    dummy: String,
}

static ENV: OnceLock<Env> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

fn write_gate(dir: &Path, public: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let config = dir.join("hades.toml");
    std::fs::write(
        &config,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            public.display(),
            dir.join("ledger.jsonl").display(),
            dir.join("DISABLED").display()
        ),
    )
    .unwrap();
    config
}

fn env() -> &'static Env {
    ENV.get_or_init(|| {
        for p in [20590u16, 20591] {
            assert!(!open(p), "port {p} is busy before the test");
        }
        for p in TARGET_PORTS {
            assert!(!open(p), "port {p} is busy before the test");
        }
        assert!(our_dummies().is_empty(), "leftover dummies before the test");
        let dir = std::env::temp_dir().join(format!("gaia-faults-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("gate")).unwrap();
        let (seed, public) = (dir.join("admin.seed"), dir.join("gate/admin.pub"));
        hades_gate::keys::generate(&seed, &public).unwrap();
        let config = write_gate(&dir.join("gate"), &public);
        let fake = Fake::default();
        fake.reset();
        let l = TcpListener::bind(FAKE).expect("bind fake nerve 20590");
        let f2 = fake.clone();
        std::thread::spawn(move || serve_fake(l, f2));
        wait_port(20590);
        eprintln!(
            "[{}] env: fake nerve {FAKE}, gate {}, dir {}",
            stamp(),
            config.display(),
            dir.display()
        );
        Env {
            fake,
            dir,
            config: config.display().to_string(),
            seed: seed.display().to_string(),
            dummy: env!("CARGO_BIN_EXE_gaia-dummy").to_string(),
        }
    })
}

/// Scenarios that finished without a panic (the teardown test only removes
/// the shared directory when all of them did).
static DONE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const SCENARIOS: usize = 9;

/// Holds the scenario lock; counts a clean finish on drop.
struct Scn(#[allow(dead_code)] MutexGuard<'static, ()>);
impl Drop for Scn {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            DONE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// Serialise scenarios (shared ports and nerve) and reset the fake nerve.
fn setup() -> Option<(Scn, &'static Env)> {
    if !real_on() {
        eprintln!(
            "SKIP: set GALION_REAL=1 (and GAIA_NERVE_BIN) to run the slot 189 fault scenarios"
        );
        return None;
    }
    let g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let e = env();
    e.fake.reset();
    Some((Scn(g), e))
}

// ---------------------------------------------------------------- helpers

fn open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn wait_port(port: u16) {
    for _ in 0..80 {
        if open(port) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("port {port} never opened");
}

fn wait_closed(port: u16, secs: u64) {
    let t0 = Instant::now();
    while open(port) {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "port {port} stayed open"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) -> Duration {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "timeout waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    t0.elapsed()
}

fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| !s.rsplit_once(") ").is_some_and(|x| x.1.starts_with('Z')))
        .unwrap_or(false)
}

/// gaia-dummy processes on our target ports (by /proc cmdline), not zombies.
fn our_dummies() -> Vec<u32> {
    let mut v = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let args: Vec<String> = cmd
            .split(|b| *b == 0)
            .map(|s| String::from_utf8_lossy(s).to_string())
            .collect();
        let dummy = args.first().is_some_and(|a| a.ends_with("/gaia-dummy"));
        let ours = args
            .iter()
            .any(|a| TARGET_PORTS.iter().any(|p| a == &p.to_string()));
        if dummy && ours && alive(pid) {
            v.push(pid);
        }
    }
    v
}

/// Test teardown only (Gaia itself never signals anything): kill our own
/// children and any gaia-dummy left on our target ports.
struct Kids(Vec<Child>);
impl Drop for Kids {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
        sweep();
    }
}

fn sweep() {
    for pid in our_dummies() {
        let _ = Command::new("kill")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status();
    }
    let _ = gaia_live::heal::reap();
    let t0 = Instant::now();
    while !our_dummies().is_empty() && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(100));
        let _ = gaia_live::heal::reap();
    }
}

struct Run {
    code: i32,
    out: String,
    err: String,
}

fn gaia(args: &[String]) -> Run {
    let o = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).to_string(),
        err: String::from_utf8_lossy(&o.stderr).to_string(),
    }
}

fn result(r: &Run) -> Value {
    let last = r.out.lines().last().unwrap_or("{}");
    serde_json::from_str::<Value>(last).unwrap_or(Value::Null)["result"].clone()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

impl Env {
    fn argv(&self, port: u16, mode: &[&str]) -> Vec<String> {
        let mut v = vec![self.dummy.clone(), "--port".into(), port.to_string()];
        v.extend(mode.iter().map(|s| s.to_string()));
        v
    }
    fn spawn_dummy(&self, port: u16, mode: &[&str]) -> Child {
        let a = self.argv(port, mode);
        let c = Command::new(&a[0])
            .args(&a[1..])
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_port(port);
        c
    }
    fn target_args(&self, svc: &str, port: u16, argv: &[String]) -> Vec<String> {
        let mut a = s(&[
            "--service",
            svc,
            "--port",
            &port.to_string(),
            "--allow-ports",
            &format!("{port}-{port}"),
        ]);
        a.push("--".into());
        a.extend(argv.iter().cloned());
        a
    }
    fn tok_dir(&self, name: &str) -> PathBuf {
        self.dir.join(format!("tok-{name}"))
    }
    /// request + admin-side mint through the real CLI into a token dir.
    fn batch(&self, dir: &Path, svc: &str, port: u16, argv: &[String], count: u64, ttl: u64) {
        let mut a = s(&["tokens", "request", "--compact", "--token-dir"]);
        a.push(dir.display().to_string());
        a.extend(s(&[
            "--count",
            &count.to_string(),
            "--ttl-s",
            &ttl.to_string(),
        ]));
        a.extend(self.target_args(svc, port, argv));
        let r = gaia(&a);
        assert_eq!(r.code, 0, "request: {} {}", r.out, r.err);
        let req = result(&r);
        let m = gaia(&s(&[
            "tokens",
            "mint",
            "--compact",
            "--request",
            req["request"].as_str().unwrap(),
            "--seed",
            &self.seed,
        ]));
        assert_eq!(m.code, 0, "mint: {} {}", m.out, m.err);
        assert!(!m.out.contains("hades1."), "mint printed a token");
    }
    /// One fresh single-use token file for one-shot ticks.
    fn one_token(&self, name: &str, svc: &str, port: u16, argv: &[String]) -> PathBuf {
        let d = self.tok_dir(name);
        self.batch(&d, svc, port, argv, 1, 600);
        first_tok(&d).unwrap()
    }
    fn heal_args(&self, nerve: &str, state: &str, config: &str) -> Vec<String> {
        let mut a = s(&["heal-tick", "--compact", "--nerve", nerve, "--state"]);
        a.push(self.dir.join(state).display().to_string());
        a.extend(s(&[
            "--hades-config",
            config,
            "--wait-ms",
            "2500",
            "--health",
            "/health",
        ]));
        a
    }
    /// One-shot heal-tick.
    fn tick(&self, nerve: &str, svc: &str, port: u16, argv: &[String], extra: &[&str]) -> Run {
        let mut a = self.heal_args(nerve, &format!("state-{svc}"), &self.config);
        a.extend(s(extra));
        a.extend(self.target_args(svc, port, argv));
        gaia(&a)
    }
    /// `heal-tick --loop` argument list.
    fn loop_args(
        &self,
        nerve: &str,
        svc: &str,
        state: &str,
        port: u16,
        argv: &[String],
        extra: &[&str],
    ) -> Vec<String> {
        let mut a = self.heal_args(nerve, state, &self.config);
        a.insert(1, "--loop".into());
        a.extend(s(extra));
        a.extend(self.target_args(svc, port, argv));
        a
    }
    fn consumed(&self, svc: &str) -> usize {
        ledger_count(&self.dir.join("gate/ledger.jsonl"), svc, "consumed")
    }
    /// Gaia restarts recorded in the global ledger for `svc`.
    fn global(&self, svc: &str) -> Vec<Value> {
        jsonl(&self.dir.join("gate/gaia-restarts.jsonl"))
            .into_iter()
            .filter(|v| v["service"] == svc)
            .collect()
    }
}

fn jsonl(p: &Path) -> Vec<Value> {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn ledger_count(p: &Path, svc: &str, event: &str) -> usize {
    jsonl(p)
        .iter()
        .filter(|v| v["event"] == event && v["target"] == svc)
        .count()
}

fn first_tok(d: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(d)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "tok"))
        .collect();
    v.sort();
    v.into_iter().next()
}

fn step<'a>(res: &'a Value, name: &str) -> Option<&'a Value> {
    res["steps"]
        .as_array()?
        .iter()
        .rev()
        .find(|s| s["step"] == name)
}

/// A loop child whose stdout lines are collected as they arrive.
struct LoopChild {
    child: Child,
    lines: Arc<Mutex<Vec<Value>>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

fn spawn_loop(args: &[String]) -> LoopChild {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let l2 = Arc::clone(&lines);
    let reader = std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&l) {
                l2.lock().unwrap().push(v);
            }
        }
    });
    LoopChild {
        child,
        lines,
        reader: Some(reader),
    }
}

impl LoopChild {
    /// Wait for exit; returns (exit code, per-tick lines, summary).
    fn finish(mut self) -> (i32, Vec<Value>, Value) {
        let st = self.child.wait().unwrap();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let mut lines = self.lines.lock().unwrap().clone();
        let sum = lines
            .pop()
            .map(|v| v["result"].clone())
            .unwrap_or(Value::Null);
        (st.code().unwrap_or(-1), lines, sum)
    }
    fn ticks(&self) -> Vec<Value> {
        self.lines.lock().unwrap().clone()
    }
}

fn evidence(scenario: &str, line: String) {
    eprintln!("[{}] {scenario}: {line}", stamp());
    if let Ok(p) = std::env::var("GAIA_FAULT_EVIDENCE") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
        {
            let _ = writeln!(f, "[{}] {scenario}: {line}", stamp());
        }
    }
}

// ============================================================== scenarios

/// F1 hang: holds the port, never answers -> hung (D-L4), report-only.
#[test]
fn f1_hang_is_report_only() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-hang";
    let argv = e.argv(P_HANG, &["--mode", "hang"]);
    let mut kids = Kids(vec![]);
    let hang = e.spawn_dummy(P_HANG, &["--mode", "hang"]);
    let pid = hang.id();
    kids.0.push(hang);
    let d = e.tok_dir(svc);
    e.batch(&d, svc, P_HANG, &argv, 3, 900);
    let t0 = Instant::now();
    let mut a = e.loop_args(
        FAKE,
        svc,
        "state-hang",
        P_HANG,
        &argv,
        &["--interval-ms", "200", "--max-ticks", "3", "--token-dir"],
    );
    a.insert(
        a.iter().position(|x| x == "--token-dir").unwrap() + 1,
        d.display().to_string(),
    );
    let r = gaia(&a);
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let sum = result(&r);
    assert_eq!(sum["outcomes"]["hung"], 3, "{sum}");
    assert_eq!(sum["restarts"], 0);
    assert_eq!(sum["tokens_spent"], 0);
    assert_eq!(e.consumed(svc), 0, "a hung tick must not consume a token");
    assert_eq!(e.global(svc).len(), 0);
    let hung = e.fake.events(svc, "gaia.heal.hung");
    assert_eq!(hung.len(), 3);
    for h in &hung {
        assert_eq!(h["data"]["listener_pids"], json!([pid]), "{h}");
        assert_eq!(h["data"]["gaia_started_pids"], json!([]));
        assert_eq!(h["data"]["kill"], false);
        assert_eq!(h["data"]["restart"], false);
    }
    assert!(alive(pid), "Gaia must never kill the hung service");
    assert!(first_tok(&d).is_some(), "tokens kept");
    evidence(
        "F1 hang",
        format!(
            "3 ticks in {:?}: outcomes {} restarts 0, tokens consumed 0 (3 kept), hung events 3 with listener_pids [{pid}], probes {}, pid alive",
            t0.elapsed(),
            sum["outcomes"],
            hung[0]["data"]["probes"]
        ),
    );
}

/// F2 slow /health: under the read timeout is healthy; over it twice is
/// hung (report-only); slow once then fast is healthy on the re-probe.
#[test]
fn f2_slow_health() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-slow";
    let cases: [(&[&str], i32, &str); 3] = [
        (&["--mode", "slow", "--delay-ms", "800"], 0, "healthy"),
        (&["--mode", "slow", "--delay-ms", "1400"], 8, "hung"),
        (
            &["--mode", "slow", "--delay-ms", "1400", "--slow-count", "1"],
            0,
            "healthy",
        ),
    ];
    for (mode, want_code, want) in cases {
        let mut kids = Kids(vec![e.spawn_dummy(P_SLOW, mode)]);
        let argv = e.argv(P_SLOW, mode);
        let t0 = Instant::now();
        let r = e.tick(FAKE, svc, P_SLOW, &argv, &[]);
        let took = t0.elapsed();
        let res = result(&r);
        assert_eq!(r.code, want_code, "{mode:?}: {} {}", r.out, r.err);
        assert_eq!(res["outcome"], want, "{mode:?}: {res}");
        assert!(alive(kids.0[0].id()), "never killed");
        evidence(
            "F2 slow",
            format!(
                "{mode:?} -> exit {} outcome {} detail {:?} in {took:?}",
                r.code, res["outcome"], res["detail"]
            ),
        );
        kids.0[0].kill().ok();
        let _ = kids.0[0].wait();
        kids.0.clear();
        wait_closed(P_SLOW, 5);
    }
    assert_eq!(e.consumed(svc), 0);
    assert_eq!(e.global(svc).len(), 0);
    assert_eq!(e.fake.count(svc, "gaia.heal.hung"), 1);
}

/// F3 bad JSON: 8 broken or misleading /health bodies are never "healthy"
/// (the old substring rule read 6 of them as ok). With tokens, restarts
/// cannot take the held port: apply-failed x3 then capped; nothing killed.
#[test]
fn f3_bad_json() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-badjson";
    for kind in [
        "truncated",
        "nested",
        "text",
        "html",
        "spaced",
        "array",
        "empty",
        "okfalse",
    ] {
        let mode = ["--mode", "badjson", "--bad", kind];
        let mut kids = Kids(vec![e.spawn_dummy(P_BAD, &mode)]);
        let argv = e.argv(P_BAD, &mode);
        let r = e.tick(FAKE, svc, P_BAD, &argv, &[]);
        let res = result(&r);
        assert_eq!(
            r.code, 4,
            "{kind}: must not read as healthy: {} {}",
            r.out, r.err
        );
        let probe = step(&res, "probe").unwrap();
        assert_ne!(probe["detail"]["state"], "up", "{kind}: {probe}");
        let ap = step(&res, "approval").unwrap();
        assert_eq!(ap["detail"]["code"], "no-token");
        evidence(
            "F3 badjson",
            format!(
                "--bad {kind}: exit 4 (dry-run refusal, no token) probe state {} notes {}",
                probe["detail"]["state"], probe["detail"]["notes"]
            ),
        );
        kids.0[0].kill().ok();
        let _ = kids.0[0].wait();
        kids.0.clear();
        wait_closed(P_BAD, 5);
    }
    // With tokens: the held port makes every restart futile.
    let mode = ["--mode", "badjson", "--bad", "truncated"];
    let kids = Kids(vec![e.spawn_dummy(P_BAD, &mode)]);
    let holder = kids.0[0].id();
    let argv = e.argv(P_BAD, &mode);
    let d = e.tok_dir(svc);
    e.batch(&d, svc, P_BAD, &argv, 3, 900);
    let mut a = e.loop_args(
        FAKE,
        svc,
        "state-badjson",
        P_BAD,
        &argv,
        &["--interval-ms", "200", "--max-ticks", "5", "--token-dir"],
    );
    a.insert(
        a.iter().position(|x| x == "--token-dir").unwrap() + 1,
        d.display().to_string(),
    );
    let r = gaia(&a);
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let sum = result(&r);
    assert_eq!(sum["outcomes"]["apply-failed"], 3, "{sum}");
    assert_eq!(sum["outcomes"]["capped"], 2, "{sum}");
    assert_eq!(e.global(svc).len(), 3, "budget 3/h holds");
    assert_eq!(e.consumed(svc), 3);
    assert!(alive(holder), "the port holder is never killed");
    assert_eq!(gaia_live::heal::listener_pids(P_BAD), vec![holder]);
    let log =
        std::fs::read_to_string(e.dir.join("state-badjson/fi-badjson.log")).unwrap_or_default();
    let bind_fail = log.matches(&format!("bind 127.0.0.1:{P_BAD}")).count();
    assert_eq!(bind_fail, 3, "{log}");
    evidence(
        "F3 badjson",
        format!(
            "tokens 3, loop 5 ticks: outcomes {} -> global ledger 3, consumed 3, holder pid {holder} alive and sole listener, {bind_fail} spawned copies failed to bind (finding D-FI2: tokens + budget burnt on a held port)",
            sum["outcomes"]
        ),
    );
    drop(kids);
}

/// F4 flapping: two loops on the same service with different --state dirs
/// and their own token batches: still exactly 3 restarts in the hour.
#[test]
fn f4_flapping_two_loops_budget_holds() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-flap";
    let argv = e.argv(P_FLAP, &["--mode", "flap", "--up-ms", "700"]);
    let (da, db) = (e.tok_dir("fi-flap-a"), e.tok_dir("fi-flap-b"));
    e.batch(&da, svc, P_FLAP, &argv, 3, 900);
    e.batch(&db, svc, P_FLAP, &argv, 3, 900);
    let mk = |state: &str, d: &Path| {
        let mut a = e.loop_args(
            FAKE,
            svc,
            state,
            P_FLAP,
            &argv,
            &[
                "--interval-ms",
                "300",
                "--jitter-ms",
                "150",
                "--max-runtime-s",
                "22",
                "--token-dir",
            ],
        );
        a.insert(
            a.iter().position(|x| x == "--token-dir").unwrap() + 1,
            d.display().to_string(),
        );
        a
    };
    let t0 = Instant::now();
    let la = spawn_loop(&mk("state-flap-a", &da));
    let lb = spawn_loop(&mk("state-flap-b", &db));
    let (ca, _ta, sa) = la.finish();
    let (cb, _tb, sb) = lb.finish();
    sweep();
    assert_eq!((ca, cb), (0, 0));
    let total = sa["restarts"].as_u64().unwrap() + sb["restarts"].as_u64().unwrap();
    assert_eq!(total, 3, "a: {sa}\nb: {sb}");
    assert_eq!(e.global(svc).len(), 3);
    assert_eq!(e.fake.count(svc, "gaia.heal.applied"), 3);
    let capped = e.fake.events(svc, "gaia.heal.capped");
    assert!(!capped.is_empty());
    let consumed = e.consumed(svc);
    assert!((3..=6).contains(&consumed), "{consumed}");
    let burnt = capped
        .iter()
        .filter(|c| c["data"]["token_consumed"] == true)
        .count();
    assert_eq!(
        consumed,
        3 + burnt,
        "only applied or lock-race-capped ticks consume"
    );
    let ts: Vec<String> = e
        .global(svc)
        .iter()
        .map(|v| v["ts"].as_str().unwrap_or("?").to_string())
        .collect();
    evidence(
        "F4 flap",
        format!(
            "2 loops x 22 s, {:?}: restarts a {} + b {} = {total}; global ledger {ts:?}; capped {} (token burnt in lock race {burnt}); consumed {consumed}; outcomes a {} b {}",
            t0.elapsed(),
            sa["restarts"],
            sb["restarts"],
            capped.len(),
            sa["outcomes"],
            sb["outcomes"]
        ),
    );
}

/// F5 port stolen: a foreign process holds the target port. Answering 503:
/// restarts fail to bind, apply-failed x3 then capped, thief untouched.
/// Hanging thief: reported hung with the foreign pid, not Gaia-started.
#[test]
fn f5_port_stolen() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-stolen";
    let mut kids = Kids(vec![e.spawn_dummy(P_STOLEN, &["--mode", "wrong"])]);
    let thief = kids.0[0].id();
    let argv = e.argv(P_STOLEN, &[]);
    let d = e.tok_dir(svc);
    e.batch(&d, svc, P_STOLEN, &argv, 3, 900);
    let mut a = e.loop_args(
        FAKE,
        svc,
        "state-stolen",
        P_STOLEN,
        &argv,
        &["--interval-ms", "200", "--max-ticks", "4", "--token-dir"],
    );
    a.insert(
        a.iter().position(|x| x == "--token-dir").unwrap() + 1,
        d.display().to_string(),
    );
    let r = gaia(&a);
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let sum = result(&r);
    assert_eq!(sum["outcomes"]["apply-failed"], 3, "{sum}");
    assert_eq!(sum["outcomes"]["capped"], 1, "{sum}");
    assert_eq!(e.global(svc).len(), 3);
    assert!(alive(thief));
    assert_eq!(gaia_live::heal::listener_pids(P_STOLEN), vec![thief]);
    let failed = e.fake.events(svc, "gaia.heal.failed");
    assert_eq!(failed.len(), 3);
    evidence(
        "F5 stolen",
        format!(
            "thief pid {thief} (503) on {P_STOLEN}: outcomes {} global ledger 3, thief alive + sole listener, failed events 3 (health_verified {})",
            sum["outcomes"], failed[0]["data"]["health_verified"]
        ),
    );
    // Hanging thief on another target port.
    let svc2 = "fi-stolen-hang";
    kids.0
        .push(e.spawn_dummy(P_STOLEN_HANG, &["--mode", "hang"]));
    let thief2 = kids.0[1].id();
    let argv2 = e.argv(P_STOLEN_HANG, &[]);
    let r = e.tick(FAKE, svc2, P_STOLEN_HANG, &argv2, &[]);
    assert_eq!(r.code, 8, "{} {}", r.out, r.err);
    let h = e.fake.events(svc2, "gaia.heal.hung");
    assert_eq!(h.len(), 1);
    assert_eq!(h[0]["data"]["listener_pids"], json!([thief2]));
    assert_eq!(
        h[0]["data"]["gaia_started_pids"],
        json!([]),
        "foreign, not Gaia's"
    );
    assert!(alive(thief2));
    evidence(
        "F5 stolen",
        format!("hanging thief pid {thief2} on {P_STOLEN_HANG}: exit 8 hung, listener_pids [{thief2}], gaia_started_pids [] (foreign), alive"),
    );
    drop(kids);
}

/// F6 nerve down and back (real openmind-nerve): while it is down every
/// tick errors and nothing is restarted, the loop backs off; when it comes
/// back the next tick heals. A nerve that stays down stops the loop (7).
#[test]
fn f6_nerve_down_and_back() {
    let Some((_g, e)) = setup() else { return };
    let Ok(nerve_bin) = std::env::var("GAIA_NERVE_BIN") else {
        panic!("GAIA_NERVE_BIN must point at a release openmind-nerve");
    };
    let start_nerve = |log: &str| -> Child {
        let c = Command::new(&nerve_bin)
            .args(["serve", "--listen", REAL, "--log"])
            .arg(e.dir.join(log))
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_port(20591);
        wait_for("real nerve healthy", 30, || {
            gaia_live::nerve::healthy(REAL.parse().unwrap())
        });
        c
    };
    let svc = "fi-nerve";
    let argv = e.argv(P_NERVE, &[]);
    let mut kids = Kids(vec![]);
    let mut nerve = start_nerve("nerve-1.jsonl");
    kids.0.push(e.spawn_dummy(P_NERVE, &[]));
    let d = e.tok_dir(svc);
    e.batch(&d, svc, P_NERVE, &argv, 2, 900);
    let stop = e.dir.join("STOP-f6");
    let mut a = e.loop_args(
        REAL,
        svc,
        "state-nerve",
        P_NERVE,
        &argv,
        &[
            "--interval-ms",
            "300",
            "--max-errors",
            "40",
            "--max-runtime-s",
            "90",
            "--token-dir",
        ],
    );
    a.insert(
        a.iter().position(|x| x == "--token-dir").unwrap() + 1,
        d.display().to_string(),
    );
    a.insert(1, stop.display().to_string());
    a.insert(1, "--stop-file".into());
    let lc = spawn_loop(&a);
    wait_for("2 healthy ticks", 20, || {
        lc.ticks()
            .iter()
            .filter(|t| t["outcome"] == "healthy")
            .count()
            >= 2
    });
    // Nerve goes down, then the service crashes while it is down.
    nerve.kill().ok();
    let _ = nerve.wait();
    let down_at = Instant::now();
    kids.0[0].kill().ok();
    let _ = kids.0[0].wait();
    wait_closed(P_NERVE, 5);
    wait_for("3 tick errors while nerve down", 30, || {
        lc.ticks()
            .iter()
            .filter(|t| t["outcome"] == "error")
            .count()
            >= 3
    });
    assert!(!open(P_NERVE), "no restart while the nerve is down");
    assert_eq!(e.global(svc).len(), 0);
    let errs: Vec<Value> = lc
        .ticks()
        .into_iter()
        .filter(|t| t["outcome"] == "error")
        .collect();
    let down_for = down_at.elapsed();
    // Nerve comes back (fresh log file).
    let nerve2 = start_nerve("nerve-2.jsonl");
    kids.0.push(nerve2);
    let back = wait_for("restart after nerve back", 40, || open(P_NERVE));
    std::fs::write(&stop, b"").unwrap();
    let (code, ticks, sum) = lc.finish();
    assert_eq!(code, 0, "{sum}");
    assert_eq!(sum["restarts"], 1, "{sum}");
    assert_eq!(sum["tokens_spent"], 1);
    assert!(sum["errors_total"].as_u64().unwrap() >= 3);
    assert_eq!(e.global(svc).len(), 1);
    let log2 = std::fs::read_to_string(e.dir.join("nerve-2.jsonl")).unwrap_or_default();
    assert!(
        log2.contains("gaia.heal.applied"),
        "applied recorded on the returned nerve"
    );
    let log1 = std::fs::read_to_string(e.dir.join("nerve-1.jsonl")).unwrap_or_default();
    assert!(!log1.contains("gaia.heal.applied"));
    evidence(
        "F6 nerve",
        format!(
            "real nerve killed; {} error ticks in {down_for:?} (first: {:?}; consecutive {:?}); port {P_NERVE} stayed closed, global ledger 0; nerve back -> restarted after {back:?}; loop ticks {} restarts 1 errors_total {} sleeps_ms {}",
            errs.len(),
            errs[0]["error"],
            errs.iter().map(|t| t["consecutive_errors"].clone()).collect::<Vec<_>>(),
            ticks.len(),
            sum["errors_total"],
            sum["sleeps_ms"]
        ),
    );
    // Nerve stays down: max-errors stops the loop with exit 7.
    for c in &mut kids.0 {
        if c.id() != 0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    kids.0.clear();
    sweep();
    wait_closed(20591, 5);
    let mut a = e.loop_args(
        REAL,
        svc,
        "state-nerve",
        P_NERVE,
        &argv,
        &["--interval-ms", "100", "--max-errors", "3"],
    );
    a.insert(1, "--max-runtime-s".into());
    a.insert(2, "30".into());
    let t0 = Instant::now();
    let r = gaia(&a);
    let sum = result(&r);
    assert_eq!(r.code, 7, "{} {}", r.out, r.err);
    assert_eq!(sum["stop"], "max-errors");
    assert_eq!(sum["restarts"], 0);
    assert!(!open(P_NERVE));
    evidence(
        "F6 nerve",
        format!(
            "nerve stays down: loop exit 7 stop max-errors after {:?}, ticks {}, restarts 0, last_error {:?}",
            t0.elapsed(),
            sum["ticks"],
            sum["last_error"]
        ),
    );
}

/// F7 nerve partial faults (fake nerve): each fails closed; where a token is
/// already consumed or a restart already spawned, the budget still counts.
#[test]
fn f7_nerve_partial_faults() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-nfault";
    let argv = e.argv(P_NFAULT, &[]);
    let mut kids = Kids(vec![]);
    let mut n = 0;
    let mut fresh = |e: &Env| {
        n += 1;
        e.one_token(&format!("fi-nfault-{n}"), svc, P_NFAULT, &argv)
    };
    // (a) nerve /health 503.
    e.fake.faults().health_down = true;
    let tok = fresh(e);
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert!(r.err.contains("not healthy"), "{}", r.err);
    assert_eq!(e.consumed(svc), 0);
    assert!(!open(P_NFAULT));
    evidence(
        "F7 nerve-fault",
        format!(
            "(a) /health 503 -> exit 2 {:?}; consumed 0, nothing posted for the tick, not started",
            r.err.lines().next().unwrap_or("")
        ),
    );
    e.fake.reset();
    // (b) /events drops the probe event.
    e.fake.faults().drop_probe = true;
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert!(
        r.err.contains("did not return the probe event"),
        "{}",
        r.err
    );
    assert_eq!(e.consumed(svc), 0);
    assert!(!open(P_NFAULT));
    evidence(
        "F7 nerve-fault",
        format!(
            "(b) /events without the probe -> exit 2 {:?}; consumed 0",
            r.err.lines().next().unwrap_or("")
        ),
    );
    e.fake.reset();
    // (c) /state 500.
    e.fake.faults().state = (500, r#"{"error":"injected"}"#.into());
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert!(r.err.contains("D-L2"), "{}", r.err);
    assert_eq!(e.consumed(svc), 0);
    evidence(
        "F7 nerve-fault",
        format!(
            "(c) /state 500 -> exit 2 {:?}; consumed 0 (token kept)",
            r.err.lines().next().unwrap_or("")
        ),
    );
    e.fake.reset();
    // (d) /state fine for the pre-token check, 500 under the budget lock.
    e.fake.faults().state_ok_left = Some(1);
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert!(r.err.contains("D-L2"), "{}", r.err);
    assert_eq!(e.consumed(svc), 1, "token consumed before the re-read");
    assert_eq!(e.global(svc).len(), 0);
    assert!(!open(P_NFAULT), "no spawn when the locked re-read fails");
    evidence("F7 nerve-fault", "(d) /state 500 only under the lock -> exit 2, no spawn, global ledger 0, but token consumed 1 (finding D-FI3)".into());
    e.fake.reset();
    // The burnt token is refused as replayed afterwards.
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 4);
    assert_eq!(
        step(&result(&r), "approval").unwrap()["detail"]["code"],
        "replayed"
    );
    // (e) POST gaia.heal.approved fails: consumed, no spawn.
    e.fake.faults().post_fail_kind = Some("gaia.heal.approved".into());
    let tok = fresh(e);
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert_eq!(e.consumed(svc), 2);
    assert!(!open(P_NFAULT));
    assert_eq!(e.global(svc).len(), 0);
    evidence(
        "F7 nerve-fault",
        format!(
            "(e) POST gaia.heal.approved 500 -> exit 2 {:?}; consumed 2, no spawn",
            r.err.lines().next().unwrap_or("")
        ),
    );
    e.fake.reset();
    // (f) POST gaia.heal.applied fails: the restart happened and counts.
    e.fake.faults().post_fail_kind = Some("gaia.heal.applied".into());
    let tok = fresh(e);
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    let res_f = result(&r);
    assert_eq!(res_f["outcome"], "restarted", "{res_f}");
    assert!(res_f["nerve_error"].is_string(), "{res_f}");
    assert!(r.err.contains("not recorded on the nerve"), "{}", r.err);
    assert!(open(P_NFAULT), "the restart itself happened");
    assert_eq!(e.global(svc).len(), 1, "and it is in the global ledger");
    evidence(
        "F7 nerve-fault",
        format!(
            "(f) POST gaia.heal.applied 500 -> exit 2 (Ok report, outcome restarted, nerve_error set) {:?}; service up, global ledger 1",
            r.err.lines().next().unwrap_or("")
        ),
    );
    e.fake.reset();
    sweep();
    wait_closed(P_NFAULT, 5);
    // The next tick sees that restart in its budget.
    let tok = fresh(e);
    let r = e.tick(
        FAKE,
        svc,
        P_NFAULT,
        &argv,
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let res = result(&r);
    let cap = step(&res, "cap").unwrap();
    assert_eq!(cap["detail"]["gaia_restarts_last_hour_before"], 1, "{cap}");
    assert_eq!(e.global(svc).len(), 2);
    evidence("F7 nerve-fault", "next tick counts the unreported restart: cap step gaia_restarts_last_hour_before 1 -> restart 2/3".into());
    // (g) D-FI3 bug fix (slot 199): the same fault under `--loop`. Before
    //     the fix that tick was an Err, so the loop's `restarts` stayed 0 and
    //     `--max-restarts 1` did not stop it; now the restart counts, its
    //     token is retired as spent, and the tick is still an error tick.
    let svc_u = "fi-unrec";
    let argv_u = e.argv(P_UNREC, &[]);
    let du = e.tok_dir(svc_u);
    e.batch(&du, svc_u, P_UNREC, &argv_u, 2, 900);
    e.fake.faults().post_fail_kind = Some("gaia.heal.applied".into());
    let mut a = e.loop_args(
        FAKE,
        svc_u,
        "state-unrec",
        P_UNREC,
        &argv_u,
        &[
            "--interval-ms",
            "200",
            "--max-ticks",
            "4",
            "--max-restarts",
            "1",
            "--token-dir",
        ],
    );
    a.insert(
        a.iter().position(|x| x == "--token-dir").unwrap() + 1,
        du.display().to_string(),
    );
    let r = gaia(&a);
    e.fake.reset();
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let sum = result(&r);
    assert_eq!(sum["stop"], "max-restarts", "{sum}");
    assert_eq!(sum["ticks"], 1, "{sum}");
    assert_eq!(sum["restarts"], 1, "{sum}");
    assert_eq!(sum["errors_total"], 1, "{sum}");
    assert_eq!(sum["tokens_spent"], 1, "{sum}");
    assert!(sum["last_error"]
        .as_str()
        .unwrap_or("")
        .contains("not recorded on the nerve"));
    let tick1: Value = r
        .out
        .lines()
        .find_map(|l| {
            serde_json::from_str::<Value>(l)
                .ok()
                .filter(|v| v["tick"] == 1)
        })
        .expect("tick 1 line");
    assert_eq!(tick1["exit"], 2, "{tick1}");
    assert_eq!(tick1["spawned"], true, "{tick1}");
    assert_eq!(tick1["token_moved"], "spent", "{tick1}");
    assert!(tick1["nerve_error"].is_string(), "{tick1}");
    assert!(open(P_UNREC), "the restart itself happened");
    assert_eq!(e.global(svc_u).len(), 1);
    assert_eq!(e.consumed(svc_u), 1);
    evidence(
        "F7 nerve-fault",
        format!(
            "(g) --loop --max-restarts 1 with POST gaia.heal.applied 500: stop {} after {} tick, restarts {}, errors_total {}, tokens_spent {}, tick 1 exit {} spawned {} token_moved {}; global ledger 1, consumed 1 (D-FI3 counter bug fixed, slot 199)",
            sum["stop"], sum["ticks"], sum["restarts"], sum["errors_total"], sum["tokens_spent"],
            tick1["exit"], tick1["spawned"], tick1["token_moved"]
        ),
    );
    sweep();
    wait_closed(P_UNREC, 5);
    kids.0.clear();
    drop(kids);
    sweep();
}

/// Unix time of 2026-10-25 00:00:00 UTC (02:00 CEST).
const OCT25_0000Z: u64 = 1_792_886_400;

fn at(h: u64, m: u64) -> u64 {
    OCT25_0000Z + h * 3600 + m * 60
}

/// F8 DST: in-process ticks on a fake wall clock across the 2026-10-25
/// 03:00 CEST -> 02:00 CET switch. The window is 3600 real seconds, the
/// repeated local 02:xx hour neither frees nor blocks a restart, ledger
/// times carry the right offset, token expiry is judged on the same clock.
#[test]
fn f8_dst_fake_clock() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-dst";
    let argv = e.argv(P_DST, &["--mode", "flap", "--up-ms", "500"]);
    let cfg = HealCfg {
        target: TestTarget {
            name: svc.into(),
            port: P_DST,
            health_path: "/health".into(),
            argv: argv.clone(),
        },
        allowed: P_DST..=P_DST,
        nerve: FAKE.parse().unwrap(),
        state_dir: e.dir.join("state-dst"),
        hades_config: PathBuf::from(&e.config),
        token_file: None,
        wait: Duration::from_millis(2500),
    };
    // Mint on the fake clock (admin side, in-process).
    let mut batch = 0;
    let mut mint = |now: u64, count: u64, ttl: u64| -> Vec<PathBuf> {
        batch += 1;
        let d = e.tok_dir(&format!("fi-dst-{batch}"));
        let mut a = s(&["--token-dir"]);
        a.push(d.display().to_string());
        a.extend(s(&[
            "--count",
            &count.to_string(),
            "--ttl-s",
            &ttl.to_string(),
        ]));
        a.extend(e.target_args(svc, P_DST, &argv));
        let ar: Vec<&str> = a.iter().map(String::as_str).collect();
        let req = gaia::tokens::request_at(&ar, now).unwrap();
        let rp = req["request"].as_str().unwrap().to_string();
        gaia::tokens::mint_at(&["--request", &rp, "--seed", &e.seed], now).unwrap();
        let mut v: Vec<PathBuf> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|x| x.path())
            .filter(|p| p.extension().is_some_and(|x| x == "tok"))
            .collect();
        v.sort();
        v
    };
    let tick = |now: u64, tok: Option<&PathBuf>| {
        wait_closed(P_DST, 5);
        let mut c = cfg.clone();
        c.token_file = tok.cloned();
        let t = live_heal::heal_tick_at(&c, Some(now)).unwrap();
        let _ = gaia_live::heal::reap();
        t
    };
    // One token per restart, minted 5 min before on the fake clock (ttl 900 =
    // the gate's default max_ttl_secs).
    let toks: Vec<PathBuf> = [at(0, 25), at(0, 45), at(1, 5)]
        .iter()
        .map(|t| mint(*t, 1, 900).remove(0))
        .collect();
    let plan: [(u64, u64, Option<usize>, u8); 4] = [
        (0, 30, Some(0), 0), // 02:30 CEST
        (0, 50, Some(1), 0), // 02:50 CEST
        (1, 10, Some(2), 0), // 02:10 CET (local clock went back)
        (1, 20, None, 5),    // 02:20 CET: 3 in the last 3600 s -> capped
    ];
    let mut lines = Vec::new();
    for (h, m, ti, want) in plan {
        let now = at(h, m);
        let t = tick(now, ti.map(|i| &toks[i]));
        let local = zagreb_time::from_unix(now as i64);
        assert_eq!(t.exit, want, "{local}: {:?}", t.report.get("detail"));
        lines.push(format!("{local} exit {}", t.exit));
        if want == 5 {
            let cap = t
                .report
                .get("steps")
                .map(gaia_core::Json::compact)
                .unwrap_or_default();
            assert!(cap.contains("\"retry_after_s\":600"), "{cap}");
        }
    }
    // 01:30Z: 00:30Z aged out exactly (3600 s); an expired token (minted
    // 01:00Z = 02:00 CET, ttl 900 -> exp 02:15 CET) is refused on the fake clock.
    let old = mint(at(1, 0), 1, 900);
    let t = tick(at(1, 30), Some(&old[0]));
    assert_eq!(t.exit, 4);
    assert_eq!(t.approval_code.as_deref(), Some("expired"));
    lines.push(format!(
        "{} expired token (exp {}) exit 4 code expired",
        zagreb_time::from_unix(at(1, 30) as i64),
        zagreb_time::from_unix(at(1, 15) as i64)
    ));
    // A 3600 s token (allowed by `gaia tokens`, D-L3) is refused by a gate on
    // its default max_ttl_secs 900 (finding D-FI5); not consumed.
    let long = mint(at(1, 29), 1, 3600);
    let t = tick(at(1, 30), Some(&long[0]));
    assert_eq!(t.exit, 4);
    assert_eq!(t.approval_code.as_deref(), Some("lifetime"));
    lines.push(format!(
        "{} ttl-3600 token exit 4 code lifetime (gate max_ttl_secs 900)",
        zagreb_time::from_unix(at(1, 30) as i64)
    ));
    let fresh = mint(at(1, 29), 1, 600);
    let t = tick(at(1, 30), Some(&fresh[0]));
    assert_eq!(t.exit, 0, "{:?}", t.report.get("detail"));
    lines.push(format!(
        "{} exit 0 (restart, window 00:50Z..01:30Z)",
        zagreb_time::from_unix(at(1, 30) as i64)
    ));
    let t = tick(at(1, 31), None);
    assert_eq!(t.exit, 5);
    assert!(t
        .report
        .get("steps")
        .map(gaia_core::Json::compact)
        .unwrap_or_default()
        .contains("\"retry_after_s\":1140"));
    lines.push(format!(
        "{} exit 5 capped retry 1140 s",
        zagreb_time::from_unix(at(1, 31) as i64)
    ));
    let ts: Vec<String> = e
        .global(svc)
        .iter()
        .map(|v| v["ts"].as_str().unwrap_or("?").to_string())
        .collect();
    assert_eq!(
        ts,
        vec![
            "2026-10-25T02:30:00+02:00",
            "2026-10-25T02:50:00+02:00",
            "2026-10-25T02:10:00+01:00",
            "2026-10-25T02:30:00+01:00",
        ]
    );
    let consumed: Vec<String> = jsonl(&e.dir.join("gate/ledger.jsonl"))
        .iter()
        .filter(|v| v["event"] == "consumed" && v["target"] == svc)
        .map(|v| v["ts"].as_str().unwrap_or("?").to_string())
        .collect();
    assert_eq!(consumed.len(), 4);
    assert_eq!(
        zagreb_time::from_unix((OCT25_0000Z + 3599) as i64),
        "2026-10-25T02:59:59+02:00"
    );
    assert_eq!(
        zagreb_time::from_unix((OCT25_0000Z + 3600) as i64),
        "2026-10-25T02:00:00+01:00"
    );
    evidence(
        "F8 dst",
        format!(
            "fake clock ticks {lines:?}; global ledger ts {ts:?} (02:30 twice, +02:00 and +01:00); hades consumed ts {consumed:?}"
        ),
    );
    sweep();
}

/// F9 ledgers and gate: a corrupt global or state ledger, a disabled gate
/// and a missing gate config all fail closed with no spawn.
#[test]
fn f9_ledger_and_gate_fail_closed() {
    let Some((_g, e)) = setup() else { return };
    let svc = "fi-ledger";
    let argv = e.argv(P_LEDGER, &[]);
    let gate2 = e.dir.join("gate2");
    let config2 = write_gate(&gate2, &e.dir.join("gate/admin.pub"));
    let c2 = config2.display().to_string();
    let tick2 = |state: &str, tok: &Path, config: &str| {
        let mut a = e.heal_args(FAKE, state, config);
        a.extend(s(&["--token-file", tok.to_str().unwrap()]));
        a.extend(e.target_args(svc, P_LEDGER, &argv));
        gaia(&a)
    };
    let tok = e.one_token("fi-ledger-1", svc, P_LEDGER, &argv);
    // (a) corrupt global ledger.
    std::fs::write(
        gate2.join("gaia-restarts.jsonl"),
        b"{\"service\":\"fi-ledger\",\"epoch_s\":1\n",
    )
    .unwrap();
    let r = tick2("state-ledger-a", &tok, &c2);
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert!(r.err.contains("ledger"), "{}", r.err);
    assert!(!open(P_LEDGER));
    assert_eq!(
        ledger_count(&gate2.join("ledger.jsonl"), svc, "consumed"),
        0
    );
    evidence(
        "F9 ledger",
        format!(
            "(a) torn global ledger line -> exit 2 {:?}; not started, consumed 0",
            r.err.lines().next().unwrap_or("")
        ),
    );
    std::fs::remove_file(gate2.join("gaia-restarts.jsonl")).unwrap();
    // (b) corrupt state ledger.
    std::fs::create_dir_all(e.dir.join("state-ledger-b")).unwrap();
    std::fs::write(e.dir.join("state-ledger-b/restarts.jsonl"), b"not json\n").unwrap();
    let r = tick2("state-ledger-b", &tok, &c2);
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert!(!open(P_LEDGER));
    assert_eq!(
        ledger_count(&gate2.join("ledger.jsonl"), svc, "consumed"),
        0
    );
    evidence(
        "F9 ledger",
        format!(
            "(b) corrupt --state ledger -> exit 2 {:?}; not started, consumed 0",
            r.err.lines().next().unwrap_or("")
        ),
    );
    // (c) disabled gate.
    std::fs::write(gate2.join("DISABLED"), b"").unwrap();
    let r = tick2("state-ledger-c", &tok, &c2);
    assert_eq!(r.code, 4, "{} {}", r.out, r.err);
    let code = step(&result(&r), "approval").unwrap()["detail"]["code"].clone();
    assert!(!open(P_LEDGER));
    std::fs::remove_file(gate2.join("DISABLED")).unwrap();
    evidence(
        "F9 ledger",
        format!("(c) gate DISABLED file -> exit 4 code {code}; not started"),
    );
    // (d) missing gate config.
    let r = tick2(
        "state-ledger-d",
        &tok,
        &e.dir.join("nope.toml").display().to_string(),
    );
    assert_eq!(r.code, 4, "{} {}", r.out, r.err);
    assert!(!open(P_LEDGER));
    evidence(
        "F9 ledger",
        format!(
            "(d) missing hades config -> exit 4 {}",
            result(&r)["detail"]
        ),
    );
    // The same token still works on an intact gate: nothing was burnt.
    let r = tick2("state-ledger-e", &tok, &c2);
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    assert_eq!(
        ledger_count(&gate2.join("ledger.jsonl"), svc, "consumed"),
        1
    );
    evidence(
        "F9 ledger",
        "(e) same token on the intact gate -> exit 0 restarted (none of (a)-(d) consumed it)"
            .into(),
    );
    sweep();
}

/// Last: teardown checks (runs alone with --test-threads 1, name sorts last).
#[test]
fn z_teardown_clean() {
    let Some((_g, e)) = setup() else { return };
    sweep();
    assert!(our_dummies().is_empty(), "{:?}", our_dummies());
    for p in TARGET_PORTS {
        assert!(!open(p), "port {p} still open");
    }
    assert!(!open(20591), "real nerve still up");
    let n = e.fake.log.lock().unwrap().len();
    let gets = *e.fake.state_gets.lock().unwrap();
    // No token text ever reached the nerve.
    let all = serde_json::to_string(&*e.fake.log.lock().unwrap()).unwrap();
    assert!(!all.contains("hades1."), "token text reached the nerve");
    evidence(
        "teardown",
        format!("0 dummies, 20592-20599 + 20690-20692 + 20591 closed (20590 is this test's thread); fake nerve {n} events, {gets} /state GETs; no token text in events"),
    );
    let done = DONE.load(std::sync::atomic::Ordering::SeqCst);
    if done == SCENARIOS {
        let _ = std::fs::remove_dir_all(&e.dir);
    } else {
        eprintln!(
            "keeping {} ({done}/{SCENARIOS} scenarios finished cleanly)",
            e.dir.display()
        );
    }
}
