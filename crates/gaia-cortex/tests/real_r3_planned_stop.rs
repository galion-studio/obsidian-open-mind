// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 309 (slot 271 F3, D-271-2 option A): the REAL release `gaia-cortex`
//! against the REAL release event-mode nerve built from the live source
//! (`openmind-nerve` 9d1a4f3 = /opt/galion/rust/bin/openmind-nerve-9d1a4f3),
//! fed the REAL VPS journal of console.service 2026-10-27 17:25:04 /
//! 17:25:06 / 17:28:29 +01:00 (tests/fixtures/console-planned-restart-20261027.jsonl,
//! systemd lines only, read-only `journalctl -o json` export). Box only,
//! loopback only, slot 309 ports 21790-21799. The nerve is dry-run
//! (`NERVE_DRY_RUN=1`) and reads its journal from a FIFO; nothing touches
//! systemd.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve 9d1a4f3>`.
//! Optional `GALION_CORTEX_BASE_BIN=<release gaia-cortex 73583ab>` adds the
//! BEFORE phase (the live cortex build): it must reproduce live dec-874.
//! `cargo test --release -p gaia-cortex --test real_r3_planned_stop -- --nocapture --test-threads 1`
//!
//! Phases (a fresh nerve + cortex each, live coalesce 1500 ms, repeat poll 2 s):
//!   before   base cortex, the 3 planned restarts -> R3-known-crash + nerve restart request (dec-874 reproduced)
//!   after    this branch, same journal -> 0 R3*, R5b-planned-stop, 0 restart requests, no circuit
//!   crash    exit status 1 + failed, no job lines -> R3-known-crash, 1 restart request
//!   kill9    code=killed status=KILL + failed signal -> R3-known-crash
//!   mixed    planned restart, then within the nerve dedupe window a real crash (exit 1)
//!            folded into the same events -> the crash still gets R3 via the repeat watch

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 21790;
const CORTEX_PORT: u16 = 21791;
const DEAD_PORT: u16 = 21799;

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
        "[309 {}] {}",
        zagreb_time::from_unix(now_unix() as i64),
        msg.as_ref()
    );
}

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let t = Duration::from_secs(15);
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), t) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(t));
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if s.write_all(head.as_bytes()).is_err() || s.write_all(body.as_bytes()).is_err() {
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
    let (c, b) = http(port, "GET", path, "");
    if c != 200 {
        return Value::Null;
    }
    serde_json::from_str(&b).unwrap_or(Value::Null)
}

fn n(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) -> Duration {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout ({secs} s) waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    t.elapsed()
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Journal(std::fs::File);

const FIXTURE: &str = include_str!("fixtures/console-planned-restart-20261027.jsonl");
const CONSOLE: &str = "console.service";
const SUBJ: &str = "unit:console.service";

impl Journal {
    fn raw(&mut self, mut v: Value, ts_us: u64) {
        v["__REALTIME_TIMESTAMP"] = json!(ts_us.to_string());
        v["_PID"] = json!("1");
        writeln!(self.0, "{v}").unwrap();
        self.0.flush().unwrap();
    }
}

struct Stack {
    _nerve: Proc,
    _cortex: Proc,
    j: Journal,
    dir: PathBuf,
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(nerve_bin: &Path, cortex_bin: &Path, tag: &str) -> Stack {
    let dir = std::env::temp_dir().join(format!("cortex-309-{tag}-{}", std::process::id()));
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
        format!(r#"[{{"name":"f309-dead","port":{DEAD_PORT},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#),
    )
    .unwrap();
    let nerve = Command::new(nerve_bin)
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
        .expect("spawn nerve");
    let nerve = Proc(nerve);
    wait_until("nerve /health", 30, || {
        http(NERVE_PORT, "GET", "/health", "").0 == 200
    });
    let j = Journal(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    let cfg = dir.join("cortex.json");
    // live /etc/gaia-cortex/config.json values that matter here
    std::fs::write(
        &cfg,
        json!({"restarts_per_unit_hour": 3, "decision_cooldown_s": 120}).to_string(),
    )
    .unwrap();
    let cortex = Proc(
        Command::new(cortex_bin)
            .arg("serve")
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("CORTEX_CONFIG", &cfg)
            .env("CORTEX_LISTEN", format!("127.0.0.1:{CORTEX_PORT}"))
            .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
            .env("CORTEX_DATA_DIR", dir.join("cortex-data"))
            .env("CORTEX_COALESCE_MS", "1500")
            .env("CORTEX_REPEAT_POLL_S", "2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("cortex.log")).unwrap())
            .spawn()
            .expect("spawn cortex"),
    );
    wait_until("cortex /health", 20, || {
        http(CORTEX_PORT, "GET", "/health", "").0 == 200
    });
    wait_until("cortex subscribed", 20, || {
        n(&get(NERVE_PORT, "/stats"), "streams") == 1
    });
    std::thread::sleep(Duration::from_secs(3));
    Stack {
        _nerve: nerve,
        _cortex: cortex,
        j,
        dir,
    }
}

fn decisions(subject: &str) -> Vec<Value> {
    let mut v: Vec<Value> = get(CORTEX_PORT, "/decisions?n=500")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["subject"] == subject && d["rule"] != "R10-remediation")
        .collect();
    v.sort_by_key(|d| {
        d["id"]
            .as_str()
            .and_then(|s| s.strip_prefix("dec-"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    });
    v
}

fn rules_of(d: &[Value]) -> Vec<String> {
    d.iter()
        .map(|x| x["rule"].as_str().unwrap_or("").to_string())
        .collect()
}

fn nerve_restarts_for(unit: &str) -> usize {
    get(NERVE_PORT, "/actions")
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| x["unit"] == unit && x["action"] == "restart")
                .count()
        })
        .unwrap_or(0)
}

fn ring_count(kind: &str, unit: &str) -> Value {
    get(NERVE_PORT, "/evt?since=0&n=1000")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|e| e["kind"] == kind && e["unit"] == unit)
        .map(|e| e["count"].clone())
        .unwrap_or(Value::Null)
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

/// Write the live 17:25:04 / 17:25:06 / 17:28:29 sequence with its real
/// gaps, the 203 s gap to the cutover restart shortened to 14 s (still past
/// the cortex's 10 s planned hold; the nerve's 600 s dedupe folds it either way).
fn replay_live(j: &mut Journal) {
    let lines = fixture();
    assert_eq!(lines.len(), 18, "fixture: 3 restarts x 6 systemd lines");
    let t0 = ts_of(&lines[0]);
    let start = Instant::now();
    let now_us = now_unix() * 1_000_000;
    for v in lines {
        let mut off = ts_of(&v) - t0;
        if off > 60_000_000 {
            off = 14_000_000 + (off % 1_000_000);
        }
        let due = Duration::from_micros(off);
        let el = start.elapsed();
        if due > el {
            std::thread::sleep(due - el);
        }
        j.raw(v, now_us + off);
    }
}

fn console_line(id: &str, extra: Value) -> Value {
    let mut v = json!({"MESSAGE_ID": id, "UNIT": CONSOLE, "MESSAGE": "console.service: slot 309", "_TRANSPORT": "journal"});
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

fn crash(j: &mut Journal, code: &str, status: &str, result: &str) {
    let t = now_unix() * 1_000_000;
    j.raw(
        console_line(
            ID_PROCESS_EXIT,
            json!({"EXIT_CODE": code, "EXIT_STATUS": status}),
        ),
        t,
    );
    j.raw(
        console_line(ID_FAILURE_RESULT, json!({"UNIT_RESULT": result})),
        t + 200,
    );
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

fn gate() -> Option<(PathBuf, PathBuf, Option<PathBuf>)> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (and GALION_NERVE_BIN)");
        return None;
    }
    let nerve = PathBuf::from(
        std::env::var("GALION_NERVE_BIN")
            .expect("GALION_REAL=1 needs GALION_NERVE_BIN=<release openmind-nerve 9d1a4f3>"),
    );
    assert!(
        nerve.is_file(),
        "GALION_NERVE_BIN {} missing",
        nerve.display()
    );
    let base = std::env::var("GALION_CORTEX_BASE_BIN")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (slot 309 range 21790-21799)"
        );
    }
    Some((
        nerve,
        PathBuf::from(env!("CARGO_BIN_EXE_gaia-cortex")),
        base,
    ))
}

#[test]
fn planned_restarts_are_not_crashes_live_journal() {
    let Some((nerve, ours, base)) = gate() else {
        return;
    };
    let mut summary = serde_json::Map::new();

    // ------------------------------------------------------------ before
    if let Some(base) = &base {
        let mut s = start(&nerve, base, "before");
        replay_live(&mut s.j);
        settle(10);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        log(format!(
            "BEFORE (base {}): console decisions {:?}; nerve restart requests {na}; ring unit.failed count {}",
            base.display(),
            rules_of(&d),
            ring_count("unit.failed", CONSOLE)
        ));
        assert!(
            rules_of(&d).iter().any(|r| r == "R3-known-crash"),
            "base must reproduce live dec-874: {d:?}"
        );
        assert!(na >= 1);
        summary.insert(
            "before".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na}),
        );
    } else {
        log("BEFORE skipped (no GALION_CORTEX_BASE_BIN)");
    }

    // ------------------------------------------------------------ after
    {
        let mut s = start(&nerve, &ours, "after");
        replay_live(&mut s.j);
        settle(10);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        let cs = get(CORTEX_PORT, "/stats");
        let inc = get(CORTEX_PORT, "/incidents");
        log(format!(
            "AFTER: console decisions {:?} reasons {:?}; nerve restart requests {na}; ring unit.failed count {} unit.exit count {} unit.stopped count {}; repeat_watch {}; circuit_open {}",
            rules_of(&d),
            d.iter().map(|x| x["reason"].clone()).collect::<Vec<_>>(),
            ring_count("unit.failed", CONSOLE),
            ring_count("unit.exit", CONSOLE),
            ring_count("unit.stopped", CONSOLE),
            cs["repeat_watch"],
            inc["units"][CONSOLE]["circuit_open"]
        ));
        assert!(
            !rules_of(&d)
                .iter()
                .any(|r| r.starts_with("R3") || r.starts_with("R2")),
            "no R3/R2 on planned restarts: {d:?}"
        );
        assert!(
            rules_of(&d).iter().any(|r| r == "R5b-planned-stop"),
            "{d:?}"
        );
        assert_eq!(na, 0, "no restart request");
        assert_eq!(n(&cs, "actions_requested"), 0, "{cs}");
        assert_ne!(inc["units"][CONSOLE]["circuit_open"], true, "{inc}");
        summary.insert(
            "after".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na,
                   "ring_failed_count": ring_count("unit.failed", CONSOLE),
                   "repeat_watch": cs["repeat_watch"]}),
        );
    }

    // ------------------------------------------------------------ controls
    for (tag, code, status, result) in [
        ("crash", "exited", "1", "exit-code"),
        ("kill9", "killed", "KILL", "signal"),
    ] {
        let mut s = start(&nerve, &ours, tag);
        crash(&mut s.j, code, status, result);
        wait_until(&format!("{tag}: decision"), 20, || {
            !decisions(SUBJ).is_empty()
        });
        settle(4);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        log(format!(
            "CONTROL {tag} (EXIT_CODE={code} EXIT_STATUS={status}): {:?}; nerve restart requests {na}",
            rules_of(&d)
        ));
        assert_eq!(rules_of(&d)[0], "R3-known-crash", "{d:?}");
        assert_eq!(na, 1);
        summary.insert(
            format!("control_{tag}"),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na}),
        );
    }

    // ------------------------------------------------------------ mixed
    {
        let mut s = start(&nerve, &ours, "mixed");
        let lines = fixture();
        let now_us = now_unix() * 1_000_000;
        for v in lines.into_iter().take(6) {
            s.j.raw(v, now_us);
        }
        wait_until("mixed: planned decision", 20, || {
            !decisions(SUBJ).is_empty()
        });
        settle(3);
        // a real crash 6 s later: the nerve folds exit+failed into the
        // planned restart's events (dedupe 600 s); only the repeat watch
        // carries it (exit status 1 -> crash evidence).
        crash(&mut s.j, "exited", "1", "exit-code");
        wait_until("mixed: crash decision", 25, || {
            decisions(SUBJ)
                .iter()
                .any(|d| d["rule"] == "R3-known-crash")
        });
        settle(3);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        log(format!(
            "MIXED planned then folded crash: {:?}; nerve restart requests {na}; ring unit.failed count {}",
            rules_of(&d),
            ring_count("unit.failed", CONSOLE)
        ));
        assert_eq!(rules_of(&d)[0], "R5b-planned-stop", "{d:?}");
        assert_eq!(na, 1);
        summary.insert(
            "mixed".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na}),
        );
    }

    log(format!(
        "SUMMARY {}",
        serde_json::to_string(&Value::Object(summary)).unwrap()
    ));
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} freed"
        );
    }
}
