// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 431 cortex-escalations-persist (D-417-3), end to end with the
//! REAL release cortex binary on the box: open escalations survive SIGTERM
//! and SIGKILL restarts, an ack / a close sticks, a torn last line (crash
//! mid-append) is tolerated, ids stay unique across restarts
//! (`esc-<epoch>-<seq>`), autoclose stays OFF by default. Then the REAL live
//! `escalations.jsonl` of 2026-10-28 (37 legacy `esc-<N>` lines, slot 387
//! copy) is loaded by a fresh start: default = all 37 reopen; with the staged
//! drop-in 93 env (autoclose + 6 h max age) the first burst closes all 37.
//!
//! Box only, loopback only, slot 431 ports 23010-23019. No nerve (dry-run
//! actions stay local), Hermes handoff off, no paid call.
//!
//! Gated: `GALION_REAL=1`. Optional `GALION_CORTEX_BIN` (default: the
//! cargo-built cortex).
//! `GALION_REAL=1 cargo test --release -p gaia-cortex --test real_escalation_persist -- --nocapture --test-threads 1`

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 23010;
const LIVE: &str = include_str!("fixtures/escalations-live-20261028.jsonl");

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[431 {}] {}",
        zagreb_time::from_unix(now_unix() as i64),
        msg.as_ref()
    );
}

fn http(method: &str, path: &str, body: &str) -> (u16, String) {
    let t = Duration::from_secs(15);
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), t) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(t));
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
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

fn get(path: &str) -> Value {
    let (c, b) = http("GET", path, "");
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

struct Cortex {
    child: Child,
}

impl Cortex {
    fn start(bin: &Path, data: &Path, extra: &[(&str, &str)]) -> Cortex {
        let mut c = Command::new(bin);
        c.arg("serve")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("CORTEX_LISTEN", format!("127.0.0.1:{PORT}"))
            .env("CORTEX_DATA_DIR", data)
            .env("CORTEX_COALESCE_MS", "300")
            .env("CORTEX_NERVE_STREAM", "0")
            .env("CORTEX_PULL_EVERY_S", "0")
            .env("CORTEX_REPEAT_POLL_S", "0")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        for (k, v) in extra {
            c.env(k, v);
        }
        let child = c.spawn().expect("spawn cortex");
        let t = Instant::now();
        wait_until("cortex /health 200", 20, || {
            http("GET", "/health", "").0 == 200
        });
        log(format!(
            "cortex pid {} up in {} ms",
            child.id(),
            t.elapsed().as_millis()
        ));
        Cortex { child }
    }

    /// SIGTERM (graceful) or SIGKILL (crash); returns the stderr reload line.
    fn stop(mut self, kill: bool) -> String {
        let sig = if kill { "-KILL" } else { "-TERM" };
        let _ = Command::new("kill")
            .args([sig, &self.child.id().to_string()])
            .status();
        let _ = self.child.wait();
        let mut err = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        wait_until("port free", 10, || {
            TcpListener::bind(("127.0.0.1", PORT)).is_ok()
        });
        err.lines()
            .find(|l| l.contains("escalations reloaded"))
            .unwrap_or("")
            .to_string()
    }
}

impl Drop for Cortex {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn post_events(evs: &[Value]) {
    let body: String = evs.iter().map(|e| format!("{e}\n")).collect();
    let (c, b) = http("POST", "/v1/events", &body);
    assert_eq!(c, 202, "ingest: {b}");
}

fn unit_event(kind: &str, sev: &str, unit: &str) -> Value {
    json!({"source": "journal", "kind": kind, "severity": sev, "ts": zagreb_time::now_rfc3339(),
           "unit": unit, "data": {"unit": unit}})
}

fn open_ids() -> Vec<String> {
    get("/escalations")["open"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| v["id"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn epoch(id: &str) -> u64 {
    match gaia_cortex::sink::parse_esc_id(id) {
        Some(gaia_cortex::sink::EscId::Unique { start, .. }) => start,
        o => panic!("new id shape expected: {id} {o:?}"),
    }
}

fn fail_units(units: &[&str]) {
    // One burst per unit: 3+ failing units in one burst fold into a single
    // R0 stack-wide escalation.
    for u in units {
        let before = open_ids().len();
        post_events(&[unit_event("unit.failed", "high", u)]);
        wait_until("an escalation for the failed unit", 20, || {
            open_ids().len() > before
        });
    }
}

fn gate() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return None;
    }
    assert!(
        TcpListener::bind(("127.0.0.1", PORT)).is_ok(),
        "port {PORT} busy (slot 431 23010-23019)"
    );
    let bin = std::env::var("GALION_CORTEX_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_gaia-cortex")));
    assert!(bin.is_file(), "cortex {} missing", bin.display());
    Some(bin)
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-431-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn lines_with(p: &Path, key: &str) -> usize {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains(&format!("\"{key}\"")))
        .count()
}

#[test]
fn real_escalation_persist_end_to_end() {
    let Some(bin) = gate() else {
        return;
    };
    log(format!("cortex binary {}", bin.display()));

    // ---- A: restarts keep the open queue; ids unique; autoclose off ----
    let d = tmpdir("a");
    let file = d.join("escalations.jsonl");
    let cx = Cortex::start(&bin, &d, &[]);
    let units = ["p431-a.service", "p431-b.service", "p431-c.service"];
    fail_units(&units);
    let ids1 = open_ids();
    let e1 = epoch(&ids1[0]);
    log(format!("run 1 open {} {:?}", ids1.len(), ids1));
    let _ = cx.stop(false);

    let cx = Cortex::start(&bin, &d, &[]);
    let ids2 = open_ids();
    let stats = get("/stats");
    let health = get("/health");
    log(format!(
        "after SIGTERM restart: open {} reloaded {} epoch {} health.escalations_open {}",
        ids2.len(),
        stats["escalations_reloaded"],
        stats["escalation_epoch"],
        health["escalations_open"]
    ));
    assert_eq!(ids2, ids1, "open survives SIGTERM restart");
    assert_eq!(health["escalations_open"], ids1.len());
    assert!(stats["escalation_epoch"].as_u64().unwrap() > e1);
    // Their incidents resolve: default config closes nothing.
    let evs: Vec<Value> = units
        .iter()
        .map(|u| unit_event("unit.started", "info", u))
        .collect();
    post_events(&evs);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(open_ids(), ids1, "autoclose OFF by default");
    assert_eq!(get("/escalations")["closed_total"], 0);
    // Ack one from process 1 in process 2.
    let (c, b) = http("POST", &format!("/escalations/ack?id={}", ids1[0]), "");
    assert_eq!(c, 200, "ack: {b}");
    fail_units(&["p431-d.service"]);
    let ids3 = open_ids();
    log(format!("run 2 after ack + 1 new: {ids3:?}"));
    let line = cx.stop(true);
    log(format!("SIGKILL; run-2 stderr: {line}"));

    let cx = Cortex::start(&bin, &d, &[]);
    assert_eq!(open_ids(), ids3, "open survives SIGKILL, ack stays");
    assert!(!open_ids().contains(&ids1[0]));
    // Run 3 writes no escalation, so the next start may reuse its epoch; the
    // guarantee is only: past every epoch already IN the log.
    let e_logged = epoch(ids3.last().unwrap());
    let _ = cx.stop(true);

    // Crash mid-append: torn last line.
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    f.write_all(br#"{"decision":"dec-9","id":"esc-1-9","incident":"inc-torn","sub"#)
        .unwrap();
    drop(f);
    let cx = Cortex::start(&bin, &d, &[]);
    let st = get("/stats");
    log(format!(
        "torn line: open {} skipped {}",
        open_ids().len(),
        st["escalations_reload_skipped"]
    ));
    assert_eq!(open_ids(), ids3);
    assert_eq!(st["escalations_reload_skipped"], 1);
    assert!(st["escalation_epoch"].as_u64().unwrap() > e_logged);
    fail_units(&["p431-e.service"]);
    let ids4 = open_ids();
    let _ = cx.stop(false);
    let cx = Cortex::start(&bin, &d, &[]);
    assert_eq!(open_ids(), ids4, "record after the torn line is readable");
    let mut all = ids4.clone();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), ids4.len(), "unique ids across 5 processes");
    let epochs: std::collections::BTreeSet<u64> = ids4.iter().map(|i| epoch(i)).collect();
    log(format!(
        "final open {} ids {:?} epochs {:?} log lines {} (ack {})",
        ids4.len(),
        ids4,
        epochs,
        std::fs::read_to_string(&file).unwrap().lines().count(),
        lines_with(&file, "ack")
    ));
    drop(cx);
    let _ = std::fs::remove_dir_all(&d);

    // ---- B: the real live file (legacy ids) through a fresh start ----
    let d = tmpdir("b");
    let file = d.join("escalations.jsonl");
    std::fs::write(&file, LIVE).unwrap();
    let cx = Cortex::start(&bin, &d, &[]);
    let n_default = open_ids().len();
    let line = cx.stop(false);
    log(format!(
        "live file, default config: open {n_default}; stderr: {line}"
    ));
    assert_eq!(n_default, 37);
    let cx = Cortex::start(
        &bin,
        &d,
        &[
            ("CORTEX_ESCALATION_AUTOCLOSE", "1"),
            ("CORTEX_ESCALATION_MAX_AGE_S", "21600"),
        ],
    );
    assert_eq!(open_ids().len(), 37);
    // First burst (any event) runs the age check.
    post_events(&[unit_event("unit.started", "info", "p431-burst.service")]);
    wait_until("legacy backlog expired", 20, || open_ids().is_empty());
    let st = get("/stats");
    log(format!(
        "drop-in 93 env: open 0, closed_expired {}, close lines {}",
        st["escalations_closed_expired"],
        lines_with(&file, "close")
    ));
    assert_eq!(st["escalations_closed_expired"], 37);
    assert_eq!(lines_with(&file, "close"), 37);
    let _ = cx.stop(false);
    let cx = Cortex::start(&bin, &d, &[]);
    assert!(open_ids().is_empty(), "expired legacy items stay closed");
    assert_eq!(
        get("/escalations")["closed_recent"]
            .as_array()
            .map_or(0, Vec::len),
        20
    );
    drop(cx);
    let _ = std::fs::remove_dir_all(&d);
    log("PASS");
}
