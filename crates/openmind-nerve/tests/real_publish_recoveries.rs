// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 246 (D-237-2): the nerve publishes recoveries of unit faults.
//!
//! Real binary (`serve --mode event`, release when run with `--release`) on
//! the box, ports 21160-21169, gated with `GALION_REAL=1`. Journal lines go
//! through a FIFO (`NERVE_EVT_JOURNALCTL=replay:`, same parser as the live
//! `journalctl -f -o json` child); `systemctl list-units` is a stand-in
//! written into the test's temp dir that prints a unit table the test edits
//! (the box has no systemd). One NDJSON subscriber on `/events/stream`
//! records every event.
//!
//! Scenarios:
//! A. postfix (untracked infra unit): 4 failures -> exactly 1 `incident.opened`;
//!    `Started` -> `unit.started` + `unit.state` + `incident.resolved` (with cause).
//! B. ssh crash loop: start/exit inside the hold never resolves; 1 open only.
//! C. postfix faults again inside the hub dedupe window -> a new incident, resolved again.
//! D. cron recovers without a journal line: the inventory row turns `active`.
//! E. transient / sandbox unit failures never open a unit incident.
//!
//! Optional: `GALION_NERVE_BASE_BIN=<openmind-nerve bb287cc>` replays A
//! against the live build and shows the bug (no recovery published).

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const PORT: u16 = 21160;
const PORT_BASE: u16 = 21165;
const HOLD_S: u64 = 2;

const FAILED: &str = "d9b373ed55a64feb8242e02dbe79a49c";
const RESTART: &str = "5eb03494b6584870a536b337290809b3";
const EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";
const STARTED: &str = "39f53479d3a045ac8e11786248231fbf";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
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
    let d = std::env::temp_dir().join(format!("nerve-s246-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::create_dir_all(d.join("bin")).unwrap();
    d
}

/// Stand-in `systemctl`: prints `units.txt` (the test edits it).
fn fake_systemctl(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("bin/systemctl");
    std::fs::write(
        &p,
        format!("#!/bin/sh\ncat '{}'\n", dir.join("units.txt").display()),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn set_units(dir: &Path, rows: &[(&str, &str)]) {
    let mut s = String::new();
    for (u, a) in rows {
        let sub = match *a {
            "active" => "running",
            "failed" => "failed",
            _ => "dead",
        };
        s.push_str(&format!("{u} loaded {a} {sub} test unit\n"));
    }
    let tmp = dir.join("units.txt.new");
    std::fs::write(&tmp, s).unwrap();
    std::fs::rename(tmp, dir.join("units.txt")).unwrap();
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
        format!(
            r#"[{{"name":"s246-dead","port":{},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#,
            port + 4
        ),
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
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .env("NERVE_EVT_RECOVER_HOLD", HOLD_S.to_string())
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

/// NDJSON subscriber: a reader thread forwards every event line.
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
    /// Wait for the first event (after index `from`) matching `f`.
    fn wait(&mut self, what: &str, secs: u64, from: usize, f: impl Fn(&Value) -> bool) -> Value {
        if let Some(v) = self.all[from..].iter().find(|v| f(v)) {
            return v.clone();
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
                return v;
            }
        }
    }
    fn count(&self, kind: &str, cause: &str) -> usize {
        self.all.iter().filter(|v| is(v, kind, cause)).count()
    }
}

fn is(v: &Value, kind: &str, cause: &str) -> bool {
    v["kind"] == kind && v["data"]["cause"] == cause
}

fn jl(j: &mut impl Write, id: &str, unit: &str, extra: &str) {
    writeln!(
        j,
        r#"{{"MESSAGE_ID":"{id}","UNIT":"{unit}","MESSAGE":"{unit}: s246","_PID":"1"{extra}}}"#
    )
    .unwrap();
    j.flush().unwrap();
}

fn open_ids(port: u16) -> Vec<String> {
    get_json(port, "/incidents")["open"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str().map(str::to_string))
        .collect()
}

fn clock() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S %z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[test]
fn real_t246_unit_recoveries_published() {
    if !real() {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    let t0 = Instant::now();
    eprintln!("start {}", clock());
    let dir = tmp("new");
    fake_systemctl(&dir);
    let mut units = vec![
        ("hermes-rs.service", "active"),
        ("postfix.service", "active"),
        ("ssh.service", "active"),
        ("cron.service", "active"),
    ];
    let set =
        |dir: &Path, units: &mut Vec<(&'static str, &'static str)>, u: &str, a: &'static str| {
            for r in units.iter_mut() {
                if r.0 == u {
                    r.1 = a;
                }
            }
            set_units(dir, units);
        };
    set_units(&dir, &units);
    let n = start(BIN, PORT, &dir);
    let mut j = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("journal.fifo"))
        .unwrap();
    let mut s = Stream::open(PORT);
    s.wait("nerve.started", 30, 0, |v| v["kind"] == "nerve.started");
    let st = get_json(PORT, "/stats");
    assert_eq!(st["config"]["recover_hold_s"], HOLD_S, "{st}");
    assert_eq!(st["units"]["opened_total"], 0, "{st}");

    // A. postfix: 4 failures -> one open
    let pf = "unit:postfix.service";
    let a0 = s.all.len();
    set(&dir, &mut units, "postfix.service", "failed");
    let ta = Instant::now();
    jl(
        &mut j,
        FAILED,
        "postfix.service",
        r#","UNIT_RESULT":"exit-code""#,
    );
    jl(&mut j, RESTART, "postfix.service", r#","N_RESTARTS":"1""#);
    jl(&mut j, EXIT, "postfix.service", r#","EXIT_STATUS":"1""#);
    jl(&mut j, EXIT, "postfix.service", r#","EXIT_STATUS":"1""#);
    let o = s.wait("A opened", 15, a0, |v| is(v, "incident.opened", pf));
    eprintln!(
        "A opened seq {} in {} ms key {} sev {}",
        o["seq"],
        ta.elapsed().as_millis(),
        o["key"],
        o["severity"]
    );
    s.drain(1500);
    assert_eq!(s.count("incident.opened", pf), 1, "no duplicate opens");
    assert!(
        open_ids(PORT).contains(&pf.to_string()),
        "A open in /incidents"
    );
    let u = get_json(PORT, "/stats")["units"].clone();
    assert_eq!(
        (u["opened_total"].as_u64(), u["repeat_faults"].as_u64()),
        (Some(1), Some(3)),
        "{u}"
    );
    // recovery
    set(&dir, &mut units, "postfix.service", "active");
    let a1 = s.all.len();
    let tr = Instant::now();
    jl(
        &mut j,
        STARTED,
        "postfix.service",
        r#","JOB_RESULT":"done""#,
    );
    let st_ev = s.wait("A unit.started", 10, a1, |v| {
        v["kind"] == "unit.started" && v["unit"] == "postfix.service"
    });
    let r = s.wait("A resolved", 20, a1, |v| is(v, "incident.resolved", pf));
    let rec_ms = tr.elapsed().as_millis();
    eprintln!(
        "A unit.started seq {} group {}; resolved seq {} after {rec_ms} ms (hold {HOLD_S}s) by {} faults {} open_s {} key {}",
        st_ev["seq"], st_ev["group"], r["seq"], r["data"]["resolved_by"], r["data"]["faults"], r["data"]["open_s"], r["key"]
    );
    assert!(
        rec_ms as u64 >= HOLD_S * 1000 - 200,
        "recovery waits for the hold"
    );
    assert_eq!(r["data"]["id"], pf);
    assert_eq!(r["data"]["resolved_by"], "journal:started");
    let ra_key = r["key"].clone();
    assert_eq!(r["data"]["faults"], 4);
    let us = s.wait("A unit.state", 2, a1, |v| {
        v["kind"] == "unit.state" && v["unit"] == "postfix.service"
    });
    assert_eq!(
        (us["data"]["to"].as_str(), us["data"]["recovered"].as_bool()),
        (Some("active"), Some(true))
    );
    assert!(
        us["seq"].as_u64() < r["seq"].as_u64(),
        "unit.state before incident.resolved"
    );
    assert!(
        !open_ids(PORT).contains(&pf.to_string()),
        "A no longer open"
    );
    let inc = get_json(PORT, "/incidents");
    assert!(
        inc["resolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == pf && i["state"] == "resolved"),
        "{inc}"
    );

    // B. ssh crash loop: never resolves inside the hold, one open only
    let sh = "unit:ssh.service";
    let b0 = s.all.len();
    set(&dir, &mut units, "ssh.service", "failed");
    jl(&mut j, EXIT, "ssh.service", r#","EXIT_STATUS":"255""#);
    s.wait("B opened", 15, b0, |v| is(v, "incident.opened", sh));
    for _ in 0..3 {
        set(&dir, &mut units, "ssh.service", "active");
        jl(&mut j, STARTED, "ssh.service", "");
        s.drain(600);
        set(&dir, &mut units, "ssh.service", "failed");
        jl(&mut j, EXIT, "ssh.service", r#","EXIT_STATUS":"255""#);
        s.drain(400);
    }
    s.drain(HOLD_S * 1000 + 1000);
    assert_eq!(
        s.count("incident.resolved", sh),
        0,
        "crash loop never resolves"
    );
    assert_eq!(s.count("incident.opened", sh), 1, "crash loop opens once");
    let u = get_json(PORT, "/stats")["units"].clone();
    let cancelled = u["recoveries_cancelled"].as_u64().unwrap();
    assert!(cancelled >= 3, "{u}");
    let started_ssh = s
        .all
        .iter()
        .filter(|v| v["kind"] == "unit.started" && v["unit"] == "ssh.service")
        .count();
    set(&dir, &mut units, "ssh.service", "active");
    let b1 = s.all.len();
    let tb = Instant::now();
    jl(&mut j, STARTED, "ssh.service", "");
    let r = s.wait("B resolved", 20, b1, |v| is(v, "incident.resolved", sh));
    eprintln!(
        "B crash loop: 3 start/exit pairs, recoveries cancelled {cancelled}, unit.started published {started_ssh} (hub dedupe 600 s), 0 resolved during loop; stable start -> resolved after {} ms, faults {}",
        tb.elapsed().as_millis(),
        r["data"]["faults"]
    );
    assert_eq!(r["data"]["faults"], 4);

    // C. postfix again inside the dedupe window: a new incident (new key)
    let c0 = s.all.len();
    set(&dir, &mut units, "postfix.service", "failed");
    jl(
        &mut j,
        FAILED,
        "postfix.service",
        r#","UNIT_RESULT":"exit-code""#,
    );
    let o2 = s.wait("C opened", 15, c0, |v| is(v, "incident.opened", pf));
    set(&dir, &mut units, "postfix.service", "active");
    jl(&mut j, STARTED, "postfix.service", "");
    let r2 = s.wait("C resolved", 20, c0, |v| is(v, "incident.resolved", pf));
    eprintln!(
        "C reopen: opened key {} -> resolved key {} (first episode {} / {})",
        o2["key"], r2["key"], o["key"], ra_key
    );
    assert_ne!(o2["key"], o["key"]);
    assert_eq!(s.count("incident.opened", pf), 2);
    assert_eq!(s.count("incident.resolved", pf), 2);

    // D. cron: no journal recovery line; the inventory row turns active
    let cr = "unit:cron.service";
    let d0 = s.all.len();
    set(&dir, &mut units, "cron.service", "failed");
    jl(
        &mut j,
        FAILED,
        "cron.service",
        r#","UNIT_RESULT":"exit-code""#,
    );
    s.wait("D opened", 15, d0, |v| is(v, "incident.opened", cr));
    s.drain(7000); // the deferred cycle + inventory after the failure sees "failed"
    assert_eq!(
        s.count("incident.resolved", cr),
        0,
        "failed row keeps it open"
    );
    set(&dir, &mut units, "cron.service", "active");
    let td = Instant::now();
    // any stack event schedules the next cycle + inventory refresh
    jl(&mut j, STARTED, "hermes-rs.service", "");
    let r = s.wait("D resolved", 30, d0, |v| is(v, "incident.resolved", cr));
    eprintln!(
        "D inventory: resolved by {} after {} ms (next event cycle + hold)",
        r["data"]["resolved_by"],
        td.elapsed().as_millis()
    );
    assert_eq!(r["data"]["resolved_by"], "inventory:active");

    // E. throwaway units never open a unit incident
    jl(
        &mut j,
        FAILED,
        "run-u4242.service",
        r#","UNIT_RESULT":"exit-code""#,
    );
    jl(
        &mut j,
        FAILED,
        "sbx246-test.service",
        r#","UNIT_RESULT":"exit-code""#,
    );
    s.drain(1500);
    let unit_opens: Vec<&Value> = s
        .all
        .iter()
        .filter(|v| v["source"] == "units" && v["kind"] == "incident.opened")
        .collect();
    assert_eq!(unit_opens.len(), 4, "postfix x2, ssh, cron only");

    // Whole stream: every unit incident alternates open -> resolved, never two opens in a row.
    let mut state: std::collections::BTreeMap<String, bool> = Default::default();
    for v in s.all.iter().filter(|v| v["source"] == "units") {
        let c = v["data"]["cause"].as_str().unwrap_or("").to_string();
        match v["kind"].as_str() {
            Some("incident.opened") => {
                assert!(
                    !state.get(&c).copied().unwrap_or(false),
                    "duplicate open {c}"
                );
                state.insert(c, true);
            }
            Some("incident.resolved") => {
                assert!(
                    state.get(&c).copied().unwrap_or(false),
                    "resolve without open {c}"
                );
                state.insert(c, false);
            }
            _ => {}
        }
    }
    assert!(
        state.values().all(|o| !o),
        "all unit incidents resolved: {state:?}"
    );
    let u = get_json(PORT, "/stats")["units"].clone();
    let engine_opened = s
        .all
        .iter()
        .filter(|v| v["source"] == "engine" && v["kind"] == "incident.opened")
        .map(|v| v["key"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    eprintln!(
        "summary: stream events {}, units opened {} resolved {} repeat_faults {} cancelled {}; engine opens {:?}; escalations {}",
        s.all.len(),
        u["opened_total"],
        u["resolved_total"],
        u["repeat_faults"],
        u["recoveries_cancelled"],
        engine_opened,
        get_json(PORT, "/stats")["escalations_total"]
    );
    for k in &engine_opened {
        assert_eq!(
            k.matches(':').count(),
            2,
            "engine open key carries the open time: {k}"
        );
    }
    drop(s);
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);

    // Optional: the live build (bb287cc) with scenario A.
    if let Ok(base) = std::env::var("GALION_NERVE_BASE_BIN") {
        let dir = tmp("base");
        fake_systemctl(&dir);
        set_units(&dir, &[("postfix.service", "failed")]);
        let n = start(&base, PORT_BASE, &dir);
        let mut j = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.join("journal.fifo"))
            .unwrap();
        let mut s = Stream::open(PORT_BASE);
        s.wait("base nerve.started", 30, 0, |v| {
            v["kind"] == "nerve.started"
        });
        jl(
            &mut j,
            FAILED,
            "postfix.service",
            r#","UNIT_RESULT":"exit-code""#,
        );
        jl(&mut j, EXIT, "postfix.service", r#","EXIT_STATUS":"1""#);
        s.drain(1500);
        set_units(&dir, &[("postfix.service", "active")]);
        jl(&mut j, STARTED, "postfix.service", "");
        s.drain(HOLD_S * 1000 + 8000);
        let failed = s
            .all
            .iter()
            .filter(|v| v["unit"] == "postfix.service" && v["kind"] != "unit.started")
            .count();
        let started = s
            .all
            .iter()
            .filter(|v| v["kind"] == "unit.started" && v["unit"] == "postfix.service")
            .count();
        let resolved = s
            .all
            .iter()
            .filter(|v| v["kind"] == "incident.resolved")
            .count();
        eprintln!(
            "BASE {base}: postfix failure events {failed}, unit.started {started}, incident.resolved {resolved} -> recovery never published"
        );
        assert!(failed >= 1);
        assert_eq!((started, resolved), (0, 0), "base build reproduces D-237-2");
        drop(s);
        drop(n);
        let _ = std::fs::remove_dir_all(&dir);
    }
    eprintln!("end {} ({} ms)", clock(), t0.elapsed().as_millis());
}

/// Real VPS data: the `gaia-hermes-handoff.path` trigger-limit failure of
/// 2026-10-21 23:06:25 / 23:08:13 +02:00 and its fix (`Started` 23:08:38),
/// 82 journal entries read-only from the VPS (`journalctl -o json`, PID 1,
/// the .path and its .service; host/boot/machine ids and cursors dropped).
/// The live nerve bb287cc published `unit.failed` + `unit.start_failed` for
/// the path unit and never its recovery (its journal filter lists services
/// only). Port 21162.
#[test]
fn real_t246_vps_journal_replay() {
    if !real() {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    let lines: Vec<String> = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vps-journal-handoff-20261021.ndjson"
    ))
    .unwrap()
    .lines()
    .map(str::to_string)
    .collect();
    assert_eq!(lines.len(), 82);
    let first_fault = lines
        .iter()
        .position(|l| l.contains(FAILED) && l.contains(".path"))
        .unwrap();
    let dir = tmp("vps");
    fake_systemctl(&dir);
    set_units(&dir, &[("gaia-hermes-handoff.service", "inactive")]);
    let port = PORT + 2;
    let n = start(BIN, port, &dir);
    let mut j = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("journal.fifo"))
        .unwrap();
    let mut s = Stream::open(port);
    s.wait("nerve.started", 30, 0, |v| v["kind"] == "nerve.started");
    let cause = "unit:gaia-hermes-handoff.path";
    for l in &lines[..=first_fault] {
        writeln!(j, "{l}").unwrap();
    }
    j.flush().unwrap();
    let o = s.wait("vps opened", 15, 0, |v| is(v, "incident.opened", cause));
    eprintln!(
        "VPS replay: opened seq {} sev {} kind {} (first fault in the VPS journal: 2026-10-21 23:06:25 +02:00)",
        o["seq"], o["severity"], o["data"]["kind"]
    );
    let k = s.all.len();
    for l in &lines[first_fault + 1..] {
        writeln!(j, "{l}").unwrap();
    }
    j.flush().unwrap();
    let r = s.wait("vps resolved", 20, k, |v| is(v, "incident.resolved", cause));
    s.drain(1000);
    let fails = s
        .all
        .iter()
        .filter(|v| {
            v["unit"] == "gaia-hermes-handoff.path"
                && v["kind"]
                    .as_str()
                    .is_some_and(|x| x.starts_with("unit.") && x.contains("fail"))
        })
        .count();
    eprintln!(
        "VPS replay: failure events published {fails}; resolved seq {} by {} faults {}; opens {} resolves {}; /incidents open {:?}",
        r["seq"],
        r["data"]["resolved_by"],
        r["data"]["faults"],
        s.count("incident.opened", cause),
        s.count("incident.resolved", cause),
        open_ids(port)
    );
    assert_eq!(s.count("incident.opened", cause), 1);
    assert_eq!(s.count("incident.resolved", cause), 1);
    assert_eq!(r["data"]["resolved_by"], "journal:started");
    assert_eq!(r["data"]["faults"], 3, "failed, failed, start_failed");
    assert!(!open_ids(port).contains(&cause.to_string()));
    drop(s);
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
}
