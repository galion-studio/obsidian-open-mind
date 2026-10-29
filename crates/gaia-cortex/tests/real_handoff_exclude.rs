// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 387 cortex-handoff-exclude: the REAL release nerve feeds the
//! REAL release cortex (this crate) the REAL VPS journal of the 2026-10-28
//! 06:24-06:31 +01:00 slot-370 test-unit crash loop
//! (`fixtures/noloop370-journal-20261028.jsonl`, 227 systemd lines for
//! `noloop370-t1..t7-20261028062409.service`, journalctl -o json read-only),
//! replayed through the nerve's FIFO journal input at 8x speed, followed by
//! three crashes of a control unit that is NOT a test unit.
//!
//! Phases (fresh nerve + cortex each, Hermes handoff ON, inbox in a temp dir):
//!   off      no exclude list = the live config: test units are handed to
//!            Hermes and use up the 6/h budget, so the control unit is
//!            rate-limited (reproduces the live 06:24 incident)
//!   on       CORTEX_HERMES_EXCLUDE_UNITS=noloop*,sbxtest*: zero test-unit
//!            tickets, escalations still logged with handoff=excluded, the
//!            cortex still opens its circuits, the control unit IS handed over
//!   refuse   CORTEX_HERMES_EXCLUDE_UNITS=hermes* (or *): the cortex refuses
//!            to start (exit != 0), and `gaia-cortex config` shows the list
//!
//! Box only, loopback only, slot 387 ports 22570-22579. The nerve is dry-run
//! (`NERVE_DRY_RUN=1`, reflex off); nothing touches systemd; the cortex is
//! dry-run; no Hermes call is made (the cortex only writes inbox files and
//! no dispatcher runs).
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve>`.
//! Optional `GALION_CORTEX_BIN` (default: the cargo-built cortex) for the VPS run.
//! `cargo test --release -p gaia-cortex --test real_handoff_exclude -- --nocapture --test-threads 1`

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Port block start. Default is the writing slot's original range; merge
/// slots (e.g. 341 cortex-live-merge-4) move the whole block at build time
/// with `REAL_CORTEX_PORT_BASE=<port>` so merged re-runs stay inside their
/// own slot range. Offsets inside the block are unchanged (+0 nerve,
/// +1 cortex, +9 dead component).
const fn port_block(default: u16) -> u16 {
    match option_env!("REAL_CORTEX_PORT_BASE") {
        None => default,
        Some(s) => {
            let b = s.as_bytes();
            let mut i = 0;
            let mut v: u32 = 0;
            assert!(!b.is_empty(), "REAL_CORTEX_PORT_BASE empty");
            while i < b.len() {
                assert!(b[i].is_ascii_digit(), "REAL_CORTEX_PORT_BASE not a number");
                v = v * 10 + (b[i] - b'0') as u32;
                assert!(v <= 65_526, "REAL_CORTEX_PORT_BASE too large");
                i += 1;
            }
            v as u16
        }
    }
}

const BLOCK: u16 = port_block(22570);
const NERVE_PORT: u16 = BLOCK;
const CORTEX_PORT: u16 = BLOCK + 1;
const DEAD_PORT: u16 = BLOCK + 9;
const FIXTURE: &str = include_str!("fixtures/noloop370-journal-20261028.jsonl");
const SPEED: u64 = 8;
const CONTROL: &str = "p387ctl-20261028.service";
const ID_STARTED: &str = "39f53479d3a045ac8e11786248231fbf";
const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds for journal timestamps; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[387 {}] {}",
        zagreb_time::from_unix(now_unix() as i64),
        msg.as_ref()
    );
}

fn http_get(port: u16, path: &str) -> (u16, String) {
    let t = Duration::from_secs(15);
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), t) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(t));
    let head = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if s.write_all(head.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).to_string();
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let b = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, b)
}

fn get(port: u16, path: &str) -> Value {
    let (c, b) = http_get(port, path);
    if c != 200 {
        return Value::Null;
    }
    serde_json::from_str(&b).unwrap_or(Value::Null)
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout ({secs} s) waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Stack {
    _nerve: Proc,
    _cortex: Proc,
    j: std::fs::File,
    dir: PathBuf,
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Stack {
    fn raw(&mut self, mut v: Value, ts_us: u64) {
        v["__REALTIME_TIMESTAMP"] = json!(ts_us.to_string());
        v["_PID"] = json!("1");
        writeln!(self.j, "{v}").unwrap();
        self.j.flush().unwrap();
    }
    fn inbox(&self) -> Vec<Value> {
        let Ok(rd) = std::fs::read_dir(self.dir.join("inbox")) else {
            return vec![];
        };
        rd.flatten()
            .filter(|f| f.file_name().to_string_lossy().ends_with(".json"))
            .map(|f| serde_json::from_str(&std::fs::read_to_string(f.path()).unwrap()).unwrap())
            .collect()
    }
    fn escalations(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("cortex-data/escalations.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

fn cortex_cmd(cortex_bin: &Path, dir: &Path, exclude: Option<&str>) -> Command {
    let mut c = Command::new(cortex_bin);
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("CORTEX_LISTEN", format!("127.0.0.1:{CORTEX_PORT}"))
        .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
        .env("CORTEX_DATA_DIR", dir.join("cortex-data"))
        .env("CORTEX_COALESCE_MS", "1500")
        .env("CORTEX_REPEAT_POLL_S", "2")
        // live gaia-cortex.service env (read-only 2026-10-28 13:27 +01:00), minus live actions
        .env("CORTEX_NERVE_STREAM", "1")
        .env("CORTEX_PULL_EVERY_S", "0")
        .env("CORTEX_HERMES_HANDOFF", "1")
        .env("CORTEX_HERMES_PER_HOUR", "6")
        .env("CORTEX_HERMES_INBOX", dir.join("inbox"));
    if let Some(x) = exclude {
        c.env("CORTEX_HERMES_EXCLUDE_UNITS", x);
    }
    c
}

fn start(nerve_bin: &Path, cortex_bin: &Path, tag: &str, exclude: Option<&str>) -> Stack {
    let dir = std::env::temp_dir().join(format!("hx387-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("nerve-data")).unwrap();
    let fifo = dir.join("journal.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"p387-dead","port":{DEAD_PORT},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#),
    )
    .unwrap();
    let nerve = Proc(
        Command::new(nerve_bin)
            .args([
                "serve",
                "--mode",
                "event",
                "--listen",
                &format!("127.0.0.1:{NERVE_PORT}"),
                "--log",
                &dir.join("nerve-events.jsonl").display().to_string(),
            ])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("NERVE_DATA_DIR", dir.join("nerve-data"))
            .env("NERVE_COMPONENTS", dir.join("components.json"))
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_EVT_PIDFD", "0")
            .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
            .env("NERVE_EVT_DEDUPE", "600")
            .env("NERVE_EVT_DEBOUNCE", "1")
            .env("NERVE_EVT_MAX_WAIT", "4")
            .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
            .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("nerve.log")).unwrap())
            .spawn()
            .expect("spawn nerve"),
    );
    wait_until("nerve /health", 30, || {
        http_get(NERVE_PORT, "/health").0 == 200
    });
    let j = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    let cortex = Proc(
        cortex_cmd(cortex_bin, &dir, exclude)
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("cortex.log")).unwrap())
            .spawn()
            .expect("spawn cortex"),
    );
    wait_until("cortex /health", 20, || {
        http_get(CORTEX_PORT, "/health").0 == 200
    });
    wait_until("cortex subscribed", 20, || {
        get(NERVE_PORT, "/stats")["streams"].as_u64() == Some(1)
    });
    std::thread::sleep(Duration::from_secs(2));
    Stack {
        _nerve: nerve,
        _cortex: cortex,
        j,
        dir,
    }
}

fn fixture() -> Vec<Value> {
    FIXTURE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn ts_of(v: &Value) -> u64 {
    v["__REALTIME_TIMESTAMP"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap()
}

/// The live crash loop with its real gaps divided by [`SPEED`].
fn replay_noloop(s: &mut Stack) {
    let lines = fixture();
    assert_eq!(lines.len(), 227, "fixture: 227 systemd lines");
    let t0 = ts_of(&lines[0]);
    let start = Instant::now();
    let base = now_unix() * 1_000_000;
    for v in lines {
        let off = (ts_of(&v) - t0) / SPEED;
        let due = Duration::from_micros(off);
        let el = start.elapsed();
        if due > el {
            std::thread::sleep(due - el);
        }
        s.raw(v, base + off);
    }
}

/// A non-test unit (unknown to the allowlist -> R4 escalation) crashing 3x.
fn control_crashes(s: &mut Stack) {
    for i in 0..3u64 {
        let inv = format!("387ctl{i:026}");
        let t = now_unix() * 1_000_000;
        let line = |id: &str, extra: Value| {
            let mut v = json!({"MESSAGE_ID": id, "UNIT": CONTROL, "INVOCATION_ID": inv,
                               "MESSAGE": format!("{CONTROL}: slot 387 control"), "_TRANSPORT": "journal"});
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            v
        };
        s.raw(
            line(
                ID_STARTED,
                json!({"JOB_TYPE": "start", "JOB_RESULT": "done"}),
            ),
            t,
        );
        s.raw(
            line(
                ID_PROCESS_EXIT,
                json!({"EXIT_CODE": "exited", "EXIT_STATUS": "1"}),
            ),
            t + 100,
        );
        s.raw(
            line(ID_FAILURE_RESULT, json!({"UNIT_RESULT": "exit-code"})),
            t + 200,
        );
        std::thread::sleep(Duration::from_secs(3));
    }
}

fn is_test(s: &str) -> bool {
    s.contains("noloop370")
}

struct Outcome {
    tickets_test: usize,
    tickets_control: usize,
    esc_test: usize,
    esc_test_excluded: usize,
    esc_control: Vec<String>,
    stats: Value,
    circuits_test: usize,
}

fn run_phase(nerve: &Path, cortex: &Path, tag: &str, exclude: Option<&str>) -> Outcome {
    let mut s = start(nerve, cortex, tag, exclude);
    let t = Instant::now();
    replay_noloop(&mut s);
    std::thread::sleep(Duration::from_secs(6));
    control_crashes(&mut s);
    std::thread::sleep(Duration::from_secs(8));
    let inbox = s.inbox();
    let esc = s.escalations();
    let stats = get(CORTEX_PORT, "/stats");
    let inc = get(CORTEX_PORT, "/incidents");
    let circuits_test = inc["units"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, v)| is_test(k) && v["circuit_open"].as_bool() == Some(true))
                .count()
        })
        .unwrap_or(0);
    let subj = |v: &Value| v["subject"].as_str().unwrap_or("").to_string();
    log(format!(
        "{tag}: inbox subjects {:?}",
        inbox
            .iter()
            .map(|v| format!("{}={}", subj(v), v["rule"].as_str().unwrap_or("")))
            .collect::<Vec<_>>()
    ));
    let o = Outcome {
        tickets_test: inbox.iter().filter(|v| is_test(&subj(v))).count(),
        tickets_control: inbox.iter().filter(|v| subj(v).contains(CONTROL)).count(),
        esc_test: esc.iter().filter(|v| is_test(&subj(v))).count(),
        esc_test_excluded: esc
            .iter()
            .filter(|v| is_test(&subj(v)) && v["handoff"] == "excluded")
            .count(),
        esc_control: esc
            .iter()
            .filter(|v| subj(v).contains(CONTROL))
            .map(|v| {
                format!(
                    "{}={}",
                    v["rule"].as_str().unwrap_or(""),
                    v["handoff"].as_str().unwrap_or("")
                )
            })
            .collect(),
        stats,
        circuits_test,
    };
    log(format!(
        "{tag} ({:.0} s): inbox test={} control={}; escalations test={} (excluded {}) control={:?}; \
         stats handoffs={} excluded={} duplicate={} rate_limited={} escalations={} alerts={}; test circuits open={}; \
         hermes_handoff={}",
        t.elapsed().as_secs_f64(),
        o.tickets_test,
        o.tickets_control,
        o.esc_test,
        o.esc_test_excluded,
        o.esc_control,
        o.stats["handoffs"],
        o.stats["handoffs_excluded"],
        o.stats["handoffs_duplicate"],
        o.stats["handoffs_rate_limited"],
        o.stats["escalations"],
        o.stats["alerts"],
        o.circuits_test,
        o.stats["hermes_handoff"],
    ));
    o
}

fn gate() -> Option<(PathBuf, PathBuf)> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (and GALION_NERVE_BIN)");
        return None;
    }
    let nerve = PathBuf::from(
        std::env::var("GALION_NERVE_BIN").expect("GALION_REAL=1 needs GALION_NERVE_BIN"),
    );
    assert!(
        nerve.is_file(),
        "GALION_NERVE_BIN {} missing",
        nerve.display()
    );
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (block {BLOCK}-{}; default slot 387 22570-22579)",
            BLOCK + 9
        );
    }
    // GALION_CORTEX_BIN: run the same test on the VPS (static musl test
    // binary copied to a throwaway folder, run as nobody) against a copied
    // cortex binary; default = the binary cargo built for this test.
    let cortex = std::env::var("GALION_CORTEX_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_gaia-cortex")));
    assert!(cortex.is_file(), "cortex {} missing", cortex.display());
    Some((nerve, cortex))
}

#[test]
fn real_handoff_exclude_end_to_end() {
    let Some((nerve, cortex)) = gate() else {
        return;
    };

    // ------------------------------------------------------------ off (live config)
    let off = run_phase(&nerve, &cortex, "off", None);
    assert!(off.esc_test >= 6, "the crash loop escalates");
    assert!(
        off.tickets_test >= 1,
        "live config hands test units to Hermes"
    );
    assert_eq!(off.esc_test_excluded, 0);
    // the test units ate the 6/h budget: the real control unit was refused
    assert!(!off.esc_control.is_empty());
    assert!(
        off.esc_control.iter().all(|e| e.ends_with("=rate-limited")),
        "{:?}",
        off.esc_control
    );
    assert_eq!(off.tickets_control, 0);

    // ------------------------------------------------------------ on (staged list)
    let on = run_phase(&nerve, &cortex, "on", Some("noloop*,sbxtest*"));
    assert_eq!(on.tickets_test, 0, "no test-unit ticket in the inbox");
    assert!(on.esc_test >= 6, "escalations are still logged");
    assert_eq!(
        on.esc_test_excluded, on.esc_test,
        "every test escalation says excluded"
    );
    assert_eq!(
        on.esc_test, off.esc_test,
        "same escalations as the live config"
    );
    assert!(
        on.circuits_test >= 1,
        "breakers still open for the test units"
    );
    assert_eq!(
        on.stats["handoffs_excluded"].as_u64(),
        Some(on.esc_test as u64)
    );
    assert_eq!(
        on.stats["hermes_handoff"]["exclude_units"],
        json!(["noloop*", "sbxtest*"])
    );
    // R4 (unknown unit) then R2 (circuit open): two rules, two tickets, as live
    assert_eq!(
        on.tickets_control, 2,
        "a real unit is still handed to Hermes"
    );
    assert!(on
        .esc_control
        .iter()
        .all(|e| e.ends_with("=handed-to-hermes")));
    assert_eq!(on.stats["handoffs_rate_limited"].as_u64(), Some(0));

    // ------------------------------------------------------------ refuse
    let dir = std::env::temp_dir().join(format!("hx387-refuse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for bad in ["hermes*", "*", "galion-*", "*test*"] {
        let t = Instant::now();
        let out = cortex_cmd(&cortex, &dir, Some(bad))
            .arg("serve")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        log(format!(
            "refuse {bad:?}: exit {:?} in {} ms: {}",
            out.status.code(),
            t.elapsed().as_millis(),
            err.trim()
        ));
        assert!(!out.status.success(), "{bad} must not start");
        assert!(err.contains("exclude_units"), "{err}");
        assert!(
            TcpListener::bind(("127.0.0.1", CORTEX_PORT)).is_ok(),
            "nothing listening after refusal"
        );
    }
    let out = cortex_cmd(&cortex, &dir, Some("sbxtest*,noloop*,swarmtest-*"))
        .arg("config")
        .output()
        .unwrap();
    assert!(out.status.success());
    let cfg: Value = serde_json::from_slice(&out.stdout).expect("config prints JSON");
    log(format!("config hermes = {}", cfg["hermes"]));
    assert_eq!(
        cfg["hermes"]["exclude_units"],
        json!(["noloop*", "sbxtest*", "swarmtest-*"])
    );
    let _ = std::fs::remove_dir_all(&dir);
}
