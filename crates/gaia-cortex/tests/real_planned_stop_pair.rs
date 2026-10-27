// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 328 planned-stop-pair-check: the PAIR end to end. The REAL
//! release nerve of slot 310 (`swarm/nerve-planned-stop-tag` 12956b45: tags a
//! planned stop as `unit.planned_stop` / `data.planned=true`) feeds the REAL
//! release cortex of slot 309 (this crate, `swarm/cortex-r3-planned-stop`
//! bdde4a07: rule R5b-planned-stop). Journal = the REAL VPS lines of
//! console.service 2026-10-27 17:25:04 / 17:25:06 / 17:28:29 +01:00
//! (fixtures/console-planned-restart-20261027.jsonl) through the nerve's FIFO
//! journal replay. Box only, loopback only, slot 328 ports 21980-21989. The
//! nerve is dry-run (`NERVE_DRY_RUN=1`); nothing touches systemd.
//!
//! The two branches cannot be merged textually (11 files of openmind-nerve and
//! galion-common conflict: the live nerve lineage 9d1a4f3 and the cortex
//! lineage 73583ab carry different nerve code), so the pair is tested the way
//! it runs live: two separately built binaries talking over HTTP.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve 12956b45>`.
//! Optional `GALION_NERVE_BASE_BIN=<release openmind-nerve 9d1a4f3>` (the live
//! nerve) adds the split-burst control: without the nerve tag the same split
//! burst is R3 (slot 309 open item 2). Optional
//! `GALION_CORTEX_BASE_BIN=<release gaia-cortex 73583ab>` (the live cortex)
//! adds: tagged nerve + live cortex = no R3 / no restart either.
//! `cargo test --release -p gaia-cortex --test real_planned_stop_pair -- --nocapture --test-threads 1`
//!
//! Phases (fresh nerve + cortex each, live coalesce 1500 ms, repeat poll 2 s, nerve dedupe 600 s):
//!   pair       tagged nerve, the 3 live planned restarts -> nerve unit.planned_stop (no unit.failed),
//!              cortex R5b-planned-stop, 0 R3/R2, 0 restart requests, circuit closed
//!   split      Stopping line, 12 s gap (> the cortex 10 s hold), then exit 143 + failed alone
//!              -> tagged nerve: still R5b / no restart; base nerve: R3 + 1 restart request
//!   crash      exit status 1 + failed, no stop job -> nerve unit.failed, cortex R3 + 1 restart
//!   kill9      code=killed status=KILL + failed signal -> R3 + 1 restart
//!   restart_then_crash  planned restart (stop job, 143, Started new invocation), the NEW
//!              invocation crashes with exit 1 one second later (the hermes-rs 19:52 shape)
//!              -> crash wins: R3 + 1 restart request

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 21980;
const CORTEX_PORT: u16 = 21981;
const DEAD_PORT: u16 = 21989;

const ID_STOPPING: &str = "de5b426a63be47a7b6ac3eaac82e2f6f";
const ID_STOPPED: &str = "9d1aaa27d60140bd96365438aad20286";
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
        "[328 {}] {}",
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
    let dir = std::env::temp_dir().join(format!("pair-328-{tag}-{}", std::process::id()));
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
        format!(r#"[{{"name":"p328-dead","port":{DEAD_PORT},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#),
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

fn no_crash_rule(d: &[Value]) -> bool {
    !rules_of(d)
        .iter()
        .any(|r| r.starts_with("R3") || r.starts_with("R2"))
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

/// Nerve ring events of one unit: kind -> (count, severity) of the newest.
fn ring(unit: &str) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    for e in get(NERVE_PORT, "/evt?since=0&n=1000")
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        if e["unit"] != unit {
            continue;
        }
        let k = e["kind"].as_str().unwrap_or("").to_string();
        m.insert(k, json!({"count": e["count"], "sev": e["severity"]}));
    }
    m
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

/// The live 17:25:04 / 17:25:06 / 17:28:29 sequence with its real gaps; the
/// 203 s gap to the cutover restart shortened to 14 s (as slot 309).
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

fn line(id: &str, inv: &str, extra: Value) -> Value {
    let mut v = json!({"MESSAGE_ID": id, "UNIT": CONSOLE, "INVOCATION_ID": inv,
                       "MESSAGE": "console.service: slot 328", "_TRANSPORT": "journal"});
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

fn now_us() -> u64 {
    now_unix() * 1_000_000
}

fn exit_failed(j: &mut Journal, inv: &str, code: &str, status: &str, result: &str) {
    let t = now_us();
    j.raw(
        line(
            ID_PROCESS_EXIT,
            inv,
            json!({"EXIT_CODE": code, "EXIT_STATUS": status}),
        ),
        t,
    );
    j.raw(
        line(ID_FAILURE_RESULT, inv, json!({"UNIT_RESULT": result})),
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
            .expect("GALION_REAL=1 needs GALION_NERVE_BIN=<release openmind-nerve 12956b45>"),
    );
    assert!(
        nerve.is_file(),
        "GALION_NERVE_BIN {} missing",
        nerve.display()
    );
    let base = std::env::var("GALION_NERVE_BASE_BIN")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (slot 328 range 21980-21989)"
        );
    }
    Some((
        nerve,
        PathBuf::from(env!("CARGO_BIN_EXE_gaia-cortex")),
        base,
    ))
}

/// Stopping line, then (after the cortex hold) exit 143 + failed of the same
/// invocation with no Stopped / Started line.
fn split_burst(s: &mut Stack) {
    let inv = "328split0000000000000000000000aa";
    s.j.raw(
        line(ID_STOPPING, inv, json!({"JOB_TYPE": "stop"})),
        now_us(),
    );
    settle(12);
    exit_failed(&mut s.j, inv, "exited", "143", "exit-code");
}

#[test]
fn planned_stop_pair_end_to_end() {
    let Some((nerve, cortex, base_nerve)) = gate() else {
        return;
    };
    let mut summary = serde_json::Map::new();

    // ------------------------------------------------------------ pair
    {
        let mut s = start(&nerve, &cortex, "pair");
        replay_live(&mut s.j);
        settle(10);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        let cs = get(CORTEX_PORT, "/stats");
        let inc = get(CORTEX_PORT, "/incidents");
        let r = ring(CONSOLE);
        log(format!(
            "PAIR live replay: console decisions {:?} reasons {:?}; nerve restart requests {na}; nerve ring {}; cortex actions_requested {}; circuit_open {}",
            rules_of(&d),
            d.iter().map(|x| x["reason"].clone()).collect::<Vec<_>>(),
            Value::Object(r.clone()),
            cs["actions_requested"],
            inc["units"][CONSOLE]["circuit_open"]
        ));
        assert!(
            r.contains_key("unit.planned_stop"),
            "nerve must tag planned stops (is GALION_NERVE_BIN 12956b45?): {r:?}"
        );
        for k in ["unit.failed", "unit.exit"] {
            assert!(
                !r.get(k).is_some_and(|e| e["sev"] == "high"),
                "nerve must not emit {k} high for a planned restart: {r:?}"
            );
        }
        assert!(no_crash_rule(&d), "no R3/R2: {d:?}");
        assert!(
            rules_of(&d).iter().any(|r| r == "R5b-planned-stop"),
            "cortex must give R5b on the nerve tag: {d:?}"
        );
        assert_eq!(na, 0, "no restart request");
        assert_eq!(n(&cs, "actions_requested"), 0, "{cs}");
        assert_ne!(inc["units"][CONSOLE]["circuit_open"], true, "{inc}");
        summary.insert(
            "pair".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na, "nerve_ring": r}),
        );
    }

    // ------------------------------------------------------------ pair with the live cortex
    if let Some(live) = std::env::var("GALION_CORTEX_BASE_BIN")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        let mut s = start(&nerve, &live, "pair-live-cortex");
        replay_live(&mut s.j);
        settle(10);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        log(format!(
            "PAIR with live cortex {} (no R5b rule): {:?}; nerve restart requests {na}",
            live.display(),
            rules_of(&d)
        ));
        assert!(
            no_crash_rule(&d),
            "tagged nerve + live cortex: no R3/R2: {d:?}"
        );
        assert_eq!(na, 0);
        summary.insert(
            "pair_live_cortex".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na}),
        );
    } else {
        log("PAIR with live cortex skipped (no GALION_CORTEX_BASE_BIN)");
    }

    // ------------------------------------------------------------ split
    {
        let mut s = start(&nerve, &cortex, "split");
        split_burst(&mut s);
        settle(8);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        let r = ring(CONSOLE);
        log(format!(
            "SPLIT tagged nerve: {:?} reasons {:?}; nerve restart requests {na}; ring {}",
            rules_of(&d),
            d.iter().map(|x| x["reason"].clone()).collect::<Vec<_>>(),
            Value::Object(r.clone())
        ));
        assert!(no_crash_rule(&d), "split burst with tag: no R3: {d:?}");
        assert_eq!(na, 0);
        summary.insert(
            "split_tagged".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na, "nerve_ring": r}),
        );
    }
    if let Some(base) = &base_nerve {
        let mut s = start(base, &cortex, "split-base");
        split_burst(&mut s);
        wait_until("split base: decision", 20, || !decisions(SUBJ).is_empty());
        settle(4);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        log(format!(
            "SPLIT base nerve {}: {:?}; nerve restart requests {na}; ring {}",
            base.display(),
            rules_of(&d),
            Value::Object(ring(CONSOLE))
        ));
        assert_eq!(rules_of(&d)[0], "R3-known-crash", "{d:?}");
        assert_eq!(na, 1);
        summary.insert(
            "split_base_nerve".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na}),
        );
    } else {
        log("SPLIT base control skipped (no GALION_NERVE_BASE_BIN)");
    }

    // ------------------------------------------------------------ crash controls
    for (tag, code, status, result) in [
        ("crash", "exited", "1", "exit-code"),
        ("kill9", "killed", "KILL", "signal"),
    ] {
        let mut s = start(&nerve, &cortex, tag);
        exit_failed(
            &mut s.j,
            "328crash000000000000000000000bb",
            code,
            status,
            result,
        );
        wait_until(&format!("{tag}: decision"), 20, || {
            !decisions(SUBJ).is_empty()
        });
        settle(4);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        let r = ring(CONSOLE);
        log(format!(
            "CONTROL {tag} (EXIT_CODE={code} EXIT_STATUS={status}): {:?}; nerve restart requests {na}; ring {}",
            rules_of(&d),
            Value::Object(r.clone())
        ));
        assert!(!r.contains_key("unit.planned_stop"), "{r:?}");
        assert!(
            r.get("unit.failed").is_some_and(|e| e["sev"] == "high"),
            "{r:?}"
        );
        assert_eq!(rules_of(&d)[0], "R3-known-crash", "{d:?}");
        assert_eq!(na, 1);
        summary.insert(
            format!("control_{tag}"),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na}),
        );
    }

    // ------------------------------------------------------------ restart then crash
    {
        let mut s = start(&nerve, &cortex, "restart-crash");
        let (a, b) = (
            "328rcA0000000000000000000000000a",
            "328rcB0000000000000000000000000b",
        );
        let t = now_us();
        s.j.raw(line(ID_STOPPING, a, json!({"JOB_TYPE": "restart"})), t);
        s.j.raw(
            line(
                ID_PROCESS_EXIT,
                a,
                json!({"EXIT_CODE": "exited", "EXIT_STATUS": "143"}),
            ),
            t + 60_000,
        );
        s.j.raw(
            line(ID_FAILURE_RESULT, a, json!({"UNIT_RESULT": "exit-code"})),
            t + 60_500,
        );
        s.j.raw(
            line(
                ID_STOPPED,
                a,
                json!({"JOB_TYPE": "restart", "JOB_RESULT": "done"}),
            ),
            t + 60_600,
        );
        s.j.raw(
            line(
                ID_STARTED,
                b,
                json!({"JOB_TYPE": "start", "JOB_RESULT": "done"}),
            ),
            t + 80_000,
        );
        settle(1);
        exit_failed(&mut s.j, b, "exited", "1", "exit-code");
        wait_until("restart-crash: R3", 25, || {
            decisions(SUBJ)
                .iter()
                .any(|d| d["rule"] == "R3-known-crash")
        });
        settle(4);
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(CONSOLE);
        let r = ring(CONSOLE);
        log(format!(
            "RESTART THEN CRASH (new invocation exit 1): {:?}; nerve restart requests {na}; ring {}",
            rules_of(&d),
            Value::Object(r.clone())
        ));
        assert!(
            r.get("unit.failed").is_some_and(|e| e["sev"] == "high"),
            "crash wins in the nerve: {r:?}"
        );
        assert_eq!(na, 1, "{d:?}");
        summary.insert(
            "restart_then_crash".into(),
            json!({"rules": rules_of(&d), "nerve_restart_requests": na, "nerve_ring": r}),
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
