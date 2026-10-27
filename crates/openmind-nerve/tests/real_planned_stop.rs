// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 310 (F2 of slot 271): the nerve tags planned stops.
//!
//! Real binary (`serve --mode event`, release with `--release`) on the box,
//! ports 21800-21809, gated with `GALION_REAL=1`. The journal is the real VPS
//! capture of the 2026-10-27 console restarts (17:25:04 operator, 17:25:06
//! cortex dec-874, 17:28:29 cutover +01:00) and real hermes-rs lines (19:52 CET:
//! operator restart, then the new invocation crashed with exit 2), fed through a FIFO (`NERVE_EVT_JOURNALCTL=replay:`, same
//! parser as the live `journalctl -f -o json` child) with timestamps moved to
//! "now" (order kept, gaps compressed). `systemctl` is a stand-in in the test's
//! temp dir (the box has no systemd): `show` prints one block per asked unit
//! from `show-<unit>.txt` (the test bumps MainPID per restart), `list-units`
//! prints `units.txt`.
//!
//! Expect (new build): console `unit.failed`/`unit.exit` 0, `unit.planned_stop`
//! 6 (info), `unit.stopping` 3, console `/state` restarts_1h 0 with
//! planned_restarts_1h >= 1 and status healthy; hermes-rs crash control keeps
//! `unit.failed` high and the crash wins over its stop job: `/state`
//! restarts_1h >= 1, planned_restarts_1h 0 (degrading).
//!
//! Optional negative control: `GALION_NERVE_BASE_BIN=<openmind-nerve 9d1a4f3>`
//! (the live build) shows the bug: console `unit.failed` high and restarts_1h >= 1.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const PORT: u16 = 21800;
const PORT_BASE: u16 = 21805;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/planned-stop")
        .join(name)
}

fn http(port: u16, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
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

struct Nerve(Child);
impl Drop for Nerve {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-s310-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::create_dir_all(d.join("bin")).unwrap();
    d
}

fn fake_systemctl(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("bin/systemctl");
    let d = dir.display();
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\nif [ \"$1\" = show ]; then\n  shift 4\n  first=1\n  for u in \"$@\"; do\n    [ $first = 1 ] || echo\n    first=0\n    if [ -f '{d}/show-'\"$u\"'.txt' ]; then cat '{d}/show-'\"$u\"'.txt'; else printf 'Id=%s\\nLoadState=not-found\\nActiveState=inactive\\nMainPID=0\\n' \"$u\"; fi\n  done\n  exit 0\nfi\ncat '{d}/units.txt'\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        dir.join("units.txt"),
        "console.service loaded active running console\nhermes-rs.service loaded active running hermes\n",
    )
    .unwrap();
}

fn set_pid(dir: &Path, unit: &str, pid: u32) {
    let body = format!(
        "Id={unit}\nLoadState=loaded\nActiveState=active\nSubState=running\nResult=success\nUnitFileState=enabled\nMainPID={pid}\nNRestarts=0\nMemoryCurrent=2142208\nMemoryMax=infinity\nCPUUsageNSec=530780000\nActiveEnterTimestampMonotonic={}\n",
        pid as u64 * 1000
    );
    let t = dir.join(format!("show-{unit}.txt.new"));
    std::fs::write(&t, body).unwrap();
    std::fs::rename(t, dir.join(format!("show-{unit}.txt"))).unwrap();
}

fn start(bin: &str, port: u16, dir: &Path) -> Nerve {
    let fifo = dir.join("journal.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    std::fs::write(
        dir.join("components.json"),
        r#"[{"name":"console","unit":"console.service","gaia":"HEPHAESTUS","system":"console"},
            {"name":"hermes-rs","unit":"hermes-rs.service","gaia":"ARTEMIS","system":"hermes"}]"#,
    )
    .unwrap();
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let child = Command::new(bin)
        .args([
            "serve",
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--log",
            &dir.join("events.jsonl").display().to_string(),
        ])
        .env_clear()
        .env("PATH", path)
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .env("NERVE_EVT_DEDUPE", "1")
        .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nerve");
    let n = Nerve(child);
    let t = Instant::now();
    while !(TcpStream::connect(("127.0.0.1", port)).is_ok() && http(port, "/health").0 == 200) {
        assert!(t.elapsed() < Duration::from_secs(30), "nerve health");
        std::thread::sleep(Duration::from_millis(100));
    }
    n
}

struct Stream {
    rx: Receiver<Value>,
    all: Vec<Value>,
}
impl Stream {
    fn open(port: u16) -> Self {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET /events/stream?since=0 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut rd = BufReader::new(s);
            let mut l = String::new();
            loop {
                l.clear();
                match rd.read_line(&mut l) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        if let Ok(v) = serde_json::from_str::<Value>(l.trim()) {
                            if tx.send(v).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        Stream { rx, all: vec![] }
    }
    fn drain(&mut self, for_ms: u64) {
        let end = Instant::now() + Duration::from_millis(for_ms);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(left) {
                Ok(v) => self.all.push(v),
                Err(_) => break,
            }
        }
    }
    fn wait(&mut self, what: &str, secs: u64, f: impl Fn(&Value) -> bool) {
        if self.all.iter().any(&f) {
            return;
        }
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let left = end
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("timeout waiting for {what}"));
            let v = self
                .rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("timeout waiting for {what}"));
            self.all.push(v.clone());
            if f(&v) {
                return;
            }
        }
    }
    fn count(&self, unit: &str, kind: &str, sev: Option<&str>) -> usize {
        self.all
            .iter()
            .filter(|v| v["unit"] == unit && v["kind"] == kind)
            .filter(|v| sev.is_none_or(|s| v["severity"] == s))
            .count()
    }
}

/// Fixture line with its timestamp moved to now (microseconds).
fn now_line(l: &str) -> String {
    let mut v: Value = serde_json::from_str(l).unwrap();
    let us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    v["__REALTIME_TIMESTAMP"] = Value::from(us.to_string());
    v.to_string()
}

fn comp(port: u16, name: &str) -> Value {
    let st = get_json(port, "/state");
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == name).cloned())
        .unwrap_or_else(|| panic!("no component {name} in /state"))
}

fn wait_pid(port: u16, name: &str, pid: u32, secs: u64) -> Value {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let c = comp(port, name);
        if c["pid"] == pid {
            return c;
        }
        assert!(
            Instant::now() < end,
            "{name} pid {pid} never in /state: {c}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn clock() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S %z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[derive(Debug)]
struct Out {
    console_failed_high: usize,
    console_exit_high: usize,
    console_planned_info: usize,
    console_stopping: usize,
    console_restarts_1h: u64,
    console_planned_restarts_1h: Option<u64>,
    console_status: String,
    hermes_failed_high: usize,
    hermes_planned: usize,
    hermes_stopping: usize,
    hermes_planned_restarts_1h: u64,
    hermes_restarts_1h: u64,
    hermes_status: String,
    console_escalated: usize,
}

fn scenario(bin: &str, port: u16, name: &str) -> Out {
    let t0 = Instant::now();
    let dir = tmp(name);
    fake_systemctl(&dir);
    set_pid(&dir, "console.service", 100);
    set_pid(&dir, "hermes-rs.service", 200);
    let _n = start(bin, port, &dir);
    let mut j = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("journal.fifo"))
        .unwrap();
    let mut s = Stream::open(port);
    s.wait("nerve.started", 30, |v| v["kind"] == "nerve.started");
    wait_pid(port, "console", 100, 30);
    // console: three real stop-job restarts, MainPID bumps after each Started
    let lines = std::fs::read_to_string(fixture("vps-console-restarts-20261027.jsonl")).unwrap();
    let mut pid = 100;
    for l in lines.lines() {
        writeln!(j, "{}", now_line(l)).unwrap();
        j.flush().unwrap();
        if l.contains("39f53479d3a045ac8e11786248231fbf") {
            pid += 1;
            set_pid(&dir, "console.service", pid);
            // one probe cycle per restart (cycle min gap 5 s, dedupe 1 s)
            wait_pid(port, "console", pid, 30);
        }
    }
    s.drain(1500);
    let c = comp(port, "console");
    eprintln!("[{name}] console after 3 planned restarts: {c}");
    // hermes-rs: real VPS 2026-10-27 19:52 CET lines: operator restart (stop job,
    // clean "Deactivated"), then the new invocation crashed (exit 2) in the same
    // second. The crash must win: one counted restart, none planned.
    let all = std::fs::read_to_string(fixture("vps-lifecycle-7d-20261027.jsonl")).unwrap();
    let crash: Vec<&str> = all
        .lines()
        .filter(|l| l.contains("\"hermes-rs.service\"") && l.contains("\"17931267"))
        .take(8)
        .collect();
    assert!(crash.len() == 8, "crash fixture lines {}", crash.len());
    for l in &crash {
        writeln!(j, "{}", now_line(l)).unwrap();
    }
    j.flush().unwrap();
    set_pid(&dir, "hermes-rs.service", 201);
    let h = wait_pid(port, "hermes-rs", 201, 30);
    s.drain(1500);
    let escal = get_json(port, "/stats");
    let console_escalated = escal
        .to_string()
        .matches("unit.failed console.service")
        .count();
    eprintln!("[{name}] hermes-rs after crash control: {h}");
    eprintln!(
        "[{name}] {} events, {} ms, {}",
        s.all.len(),
        t0.elapsed().as_millis(),
        clock()
    );
    let _ = std::fs::remove_dir_all(&dir);
    Out {
        console_failed_high: s.count("console.service", "unit.failed", Some("high")),
        console_exit_high: s.count("console.service", "unit.exit", Some("high")),
        console_planned_info: s.count("console.service", "unit.planned_stop", Some("info")),
        console_stopping: s.count("console.service", "unit.stopping", None),
        console_restarts_1h: c["restarts_1h"].as_u64().unwrap_or(99),
        console_planned_restarts_1h: c["planned_restarts_1h"].as_u64(),
        console_status: c["status"].as_str().unwrap_or("").into(),
        hermes_failed_high: s.count("hermes-rs.service", "unit.failed", Some("high")),
        hermes_planned: s.count("hermes-rs.service", "unit.planned_stop", None),
        hermes_stopping: s.count("hermes-rs.service", "unit.stopping", None),
        hermes_planned_restarts_1h: h["planned_restarts_1h"].as_u64().unwrap_or(99),
        hermes_restarts_1h: h["restarts_1h"].as_u64().unwrap_or(0),
        hermes_status: h["status"].as_str().unwrap_or("").into(),
        console_escalated,
    }
}

#[test]
fn real_t310_planned_stop_tag() {
    if !real() {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    eprintln!("start {}", clock());
    let o = scenario(BIN, PORT, "new");
    eprintln!("NEW {o:?}");
    assert_eq!(o.console_failed_high, 0, "{o:?}");
    assert_eq!(o.console_exit_high, 0, "{o:?}");
    assert_eq!(o.console_planned_info, 6, "{o:?}");
    assert_eq!(o.console_stopping, 3, "{o:?}");
    assert_eq!(o.console_restarts_1h, 0, "{o:?}");
    assert!(o.console_planned_restarts_1h.unwrap_or(0) >= 1, "{o:?}");
    assert_eq!(o.console_status, "healthy", "{o:?}");
    assert_eq!(o.console_escalated, 0, "{o:?}");
    // crash control
    assert_eq!(o.hermes_failed_high, 1, "{o:?}");
    assert_eq!(o.hermes_planned, 0, "{o:?}");
    assert_eq!(o.hermes_stopping, 1, "{o:?}");
    assert_eq!(o.hermes_planned_restarts_1h, 0, "crash wins: {o:?}");
    assert!(o.hermes_restarts_1h >= 1, "{o:?}");
    assert_eq!(o.hermes_status, "degrading", "{o:?}");
    if let Ok(base) = std::env::var("GALION_NERVE_BASE_BIN") {
        let b = scenario(&base, PORT_BASE, "base");
        eprintln!("BASE {b:?}");
        assert!(b.console_failed_high >= 3, "base shows the bug: {b:?}");
        assert!(
            b.console_restarts_1h >= 1,
            "base counts planned restarts: {b:?}"
        );
        assert_ne!(b.console_status, "healthy", "{b:?}");
    }
    eprintln!("end {}", clock());
}
