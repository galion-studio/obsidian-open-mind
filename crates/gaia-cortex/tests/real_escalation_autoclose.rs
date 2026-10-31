// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 417 cortex-escalations-diag: why the live `escalations_open`
//! only grew (1 at 05:47 -> 11 at 15:37 +01:00 on 2026-10-28) and the
//! staged fix, end to end. The REAL release nerve feeds the REAL release
//! cortex the REAL VPS journal of the 2026-10-28 06:24-06:31 +01:00 slot-370
//! test-unit crash loop (`fixtures/noloop370-journal-20261028.jsonl`, the
//! same 227 lines that produced 10 of the 11 live open escalations), at 8x
//! speed, then a control unit crashes 3x and recovers, then one unrelated
//! burst arrives after `escalation_max_age_s`.
//!
//! Phases (fresh nerve + cortex each, live cortex env minus live actions):
//!   off   live config (no autoclose, no max age): every escalation stays
//!         open although the control incident resolved = the live growth
//!   on    CORTEX_ESCALATION_AUTOCLOSE=1 + CORTEX_ESCALATION_MAX_AGE_S=MAX_AGE:
//!         the control escalations close with its incident; everything
//!         else closes as `expired` on the first burst after MAX_AGE;
//!         `escalations_open` = 0 and one close line per escalation
//!
//! Box only, loopback only, slot 417 ports 22870-22879. Nerve dry-run
//! (reflex off, no systemd), cortex dry-run, Hermes handoff ON into a temp
//! inbox (no dispatcher, no paid call).
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve>`.
//! Optional `GALION_CORTEX_BIN` (default: the cargo-built cortex).
//! `cargo test --release -p gaia-cortex --test real_escalation_autoclose -- --nocapture --test-threads 1`

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 22870;
const CORTEX_PORT: u16 = 22871;
const DEAD_PORT: u16 = 22879;
const FIXTURE: &str = include_str!("fixtures/noloop370-journal-20261028.jsonl");
const SPEED: u64 = 8;
const CONTROL: &str = "p417ctl-20261028.service";
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
        "[417 {}] {}",
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

fn cortex_cmd(cortex_bin: &Path, dir: &Path, extra: &[(&str, &str)]) -> Command {
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
        // 2026-10-31: the default action mode is notify (never hands off); this
        // replays the pre-notify live config, which handed off.
        .env("CORTEX_ACTION_MODE", "auto")
        .env("CORTEX_HERMES_PER_HOUR", "6")
        .env("CORTEX_HERMES_INBOX", dir.join("inbox"));
    for (k, v) in extra {
        c.env(k, v);
    }
    c
}

fn start(nerve_bin: &Path, cortex_bin: &Path, tag: &str, extra: &[(&str, &str)]) -> Stack {
    let dir = std::env::temp_dir().join(format!("ea417-{tag}-{}", std::process::id()));
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
        format!(r#"[{{"name":"p417-dead","port":{DEAD_PORT},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#),
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
        cortex_cmd(cortex_bin, &dir, extra)
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
        let inv = format!("417ctl{i:026}");
        let t = now_unix() * 1_000_000;
        let line = |id: &str, extra: Value| {
            let mut v = json!({"MESSAGE_ID": id, "UNIT": CONTROL, "INVOCATION_ID": inv,
                               "MESSAGE": format!("{CONTROL}: slot 417 control"), "_TRANSPORT": "journal"});
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

const MAX_AGE: u64 = 90;
const RECOVERY_WAIT: u64 = 60;
const TICK_UNIT: &str = "p417tick-20261028.service";

/// The control unit comes back: its own "cleared" signal.
fn control_recovers(s: &mut Stack) {
    let t = now_unix() * 1_000_000;
    s.raw(
        json!({"MESSAGE_ID": ID_STARTED, "UNIT": CONTROL, "INVOCATION_ID": format!("417ctlok{:024}", 0),
               "MESSAGE": format!("Started {CONTROL}."), "_TRANSPORT": "journal",
               "JOB_TYPE": "start", "JOB_RESULT": "done"}),
        t,
    );
}

/// One unrelated, harmless burst (a unit start) so the cortex decides again.
fn tick(s: &mut Stack, n: u64) {
    let t = now_unix() * 1_000_000;
    s.raw(
        json!({"MESSAGE_ID": ID_STARTED, "UNIT": TICK_UNIT, "INVOCATION_ID": format!("417tick{n:025}"),
               "MESSAGE": format!("Started {TICK_UNIT}."), "_TRANSPORT": "journal",
               "JOB_TYPE": "start", "JOB_RESULT": "done"}),
        t,
    );
}

struct Outcome {
    total: u64,
    open_after_recovery: usize,
    open_end: usize,
    closed_total: u64,
    why: std::collections::BTreeMap<String, usize>,
    close_lines: usize,
    control_open_after_recovery: usize,
    health_open: Value,
    stats: Value,
}

fn subj(v: &Value) -> String {
    v["subject"].as_str().unwrap_or("").to_string()
}

fn run_phase(nerve: &Path, cortex: &Path, tag: &str, extra: &[(&str, &str)]) -> Outcome {
    let mut s = start(nerve, cortex, tag, extra);
    let t = Instant::now();
    replay_noloop(&mut s);
    std::thread::sleep(Duration::from_secs(6));
    control_crashes(&mut s);
    std::thread::sleep(Duration::from_secs(6));
    control_recovers(&mut s);
    // the recovery reaches the cortex through the nerve's debounce: give
    // it up to RECOVERY_WAIT s to close the control escalations
    let w = Instant::now();
    let control_open = || {
        get(CORTEX_PORT, "/escalations")["open"]
            .as_array()
            .map_or(usize::MAX, |a| {
                a.iter().filter(|v| subj(v).contains(CONTROL)).count()
            })
    };
    while control_open() != 0 && w.elapsed() < Duration::from_secs(RECOVERY_WAIT) {
        std::thread::sleep(Duration::from_millis(500));
    }
    log(format!(
        "{tag}: control escalations open {} after {:.1} s of waiting for the recovery",
        control_open(),
        w.elapsed().as_secs_f64()
    ));
    let esc = get(CORTEX_PORT, "/escalations");
    let open_after_recovery = esc["open"].as_array().map_or(0, Vec::len);
    let control_open_after_recovery = esc["open"].as_array().map_or(0, |a| {
        a.iter().filter(|v| subj(v).contains(CONTROL)).count()
    });
    log(format!(
        "{tag} ({:.0} s): after control recovery open={open_after_recovery} (control {control_open_after_recovery}) total={} closed_total={}",
        t.elapsed().as_secs_f64(),
        esc["total"],
        esc["closed_total"]
    ));
    // wait past MAX_AGE from the newest escalation, then one burst
    let last_esc = s
        .escalations()
        .iter()
        .filter(|v| v.get("id").is_some())
        .count();
    assert!(last_esc > 0, "{tag}: the replay escalates");
    std::thread::sleep(Duration::from_secs(MAX_AGE + 5));
    tick(&mut s, 1);
    std::thread::sleep(Duration::from_secs(5));
    let esc = get(CORTEX_PORT, "/escalations");
    let health = get(CORTEX_PORT, "/health");
    let stats = get(CORTEX_PORT, "/stats");
    let lines = s.escalations();
    let mut why = std::collections::BTreeMap::new();
    for l in lines.iter().filter(|l| l.get("close").is_some()) {
        *why.entry(l["why"].as_str().unwrap_or("?").to_string())
            .or_insert(0) += 1;
    }
    let o = Outcome {
        total: esc["total"].as_u64().unwrap_or(0),
        open_after_recovery,
        open_end: esc["open"].as_array().map_or(0, Vec::len),
        closed_total: esc["closed_total"].as_u64().unwrap_or(0),
        close_lines: lines.iter().filter(|l| l.get("close").is_some()).count(),
        why,
        control_open_after_recovery,
        health_open: health["escalations_open"].clone(),
        stats,
    };
    // keep the raw run files for the evidence doc (box /tmp, not committed)
    let keep = std::env::temp_dir().join(format!("417-evidence/{tag}"));
    let _ = std::fs::create_dir_all(&keep);
    for f in ["escalations.jsonl", "decisions.jsonl", "alerts.jsonl"] {
        let _ = std::fs::copy(s.dir.join("cortex-data").join(f), keep.join(f));
    }
    let rules: Vec<String> = lines
        .iter()
        .filter(|l| l.get("id").is_some())
        .map(|l| {
            format!(
                "{}={}",
                subj(l).trim_start_matches("unit:"),
                l["rule"].as_str().unwrap_or("")
            )
        })
        .collect();
    log(format!(
        "{tag} ({:.0} s): escalations total={} {:?}; end open={} /health escalations_open={} closed_total={} close lines={} by why {:?}; stats closed_incident={} closed_expired={} incidents_open={}; hermes inbox tickets={}",
        t.elapsed().as_secs_f64(),
        o.total,
        rules,
        o.open_end,
        o.health_open,
        o.closed_total,
        o.close_lines,
        o.why,
        o.stats["escalations_closed_incident"],
        o.stats["escalations_closed_expired"],
        health["incidents_open"],
        s.inbox().len(),
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
            "port {p} busy (slot 417 22870-22879)"
        );
    }
    let cortex = std::env::var("GALION_CORTEX_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_gaia-cortex")));
    assert!(cortex.is_file(), "cortex {} missing", cortex.display());
    Some((nerve, cortex))
}

#[test]
fn real_escalation_autoclose_end_to_end() {
    let Some((nerve, cortex)) = gate() else {
        return;
    };
    let age = MAX_AGE.to_string();

    // ------------------------------------------------------------ off (live config)
    let off = run_phase(&nerve, &cortex, "off", &[]);
    assert!(off.total >= 6, "the crash loop escalates");
    assert_eq!(off.closed_total, 0);
    assert_eq!(off.close_lines, 0);
    assert!(
        off.control_open_after_recovery >= 1,
        "live: a recovered unit's escalation stays open"
    );
    assert_eq!(off.open_end as u64, off.total, "live: nothing ever closes");
    assert_eq!(off.health_open, json!(off.total));

    // ------------------------------------------------------------ on (staged fix)
    let on = run_phase(
        &nerve,
        &cortex,
        "on",
        &[
            ("CORTEX_ESCALATION_AUTOCLOSE", "1"),
            ("CORTEX_ESCALATION_MAX_AGE_S", age.as_str()),
        ],
    );
    assert_eq!(on.total, off.total, "same escalations as the live config");
    assert_eq!(
        on.control_open_after_recovery, 0,
        "the control unit's escalations close with its incident"
    );
    assert!(on.open_after_recovery < off.open_after_recovery);
    assert!(
        on.why.get("incident-cleared").copied().unwrap_or(0) >= 1,
        "{:?}",
        on.why
    );
    assert_eq!(on.open_end, 0, "nothing left open after MAX_AGE");
    assert_eq!(on.health_open, json!(0));
    assert_eq!(on.closed_total, on.total);
    assert_eq!(on.close_lines as u64, on.total, "one close line each");
    assert_eq!(
        on.stats["escalations_closed_incident"]
            .as_u64()
            .unwrap_or(0)
            + on.stats["escalations_closed_expired"].as_u64().unwrap_or(0),
        on.total
    );
}
