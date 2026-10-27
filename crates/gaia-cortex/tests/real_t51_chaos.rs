// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex T5.1: chaos scenarios against the REAL release `gaia-cortex`
//! binary and the REAL event-mode nerve (`openmind-nerve serve --mode
//! event`, branch `swarm/nerve-event-driven` bb287cc), box only, loopback
//! only, slot 227 ports 20970-20979.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve>`.
//! Run with `cargo test --release -p gaia-cortex --test real_t51_chaos -- --nocapture`.
//!
//! Scenarios (one shared nerve + cortex, phases in order):
//!   S1 unit kill          journal: tuner.service SIGKILL exit + failed -> one incident, R3 dry-run restart answered by the nerve
//!   S2 restart loop       same unit keeps failing: nerve dedupe (default 600 s) folds it; with a 1 s window the cortex opens its circuit
//!   S3 protected pieces   ssh / cloudflared / postfix / backup / /opt/galion/openmind -> alert only, never an action request
//!   S4 grouping           4 stack units fail in one burst -> ONE R0-stack-wide decision, no restarts
//!   S5 malformed burst    garbage journal lines into the nerve, garbage / oversized / nested / injection bodies into the cortex,
//!                         a connection flood; both stay up, nothing acts on garbage
//!   S6 slow upstream      a second cortex through a latency proxy (1 s ok, 4 s > 3 s client timeout)
//!   S7 hung upstream      SIGSTOP the nerve while the cortex has to act; SIGCONT; stream resumes
//!   S8 nerve restart      SIGKILL the nerve mid-incident, new nerve on the same port: the cortex must see the new nerve's events
//!   S9 LLM advisor        error-severity ambiguous incident: advisor skipped (disabled, not compiled), 0 calls, $0, no non-loopback socket;
//!                         CORTEX_LLM_ENABLED=1 refused at start (llm-advisor builds: starts, 0 calls without a key)
//!
//! Nothing here touches systemd, the VPS or the network beyond 127.0.0.1.

use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Port base: slot 227 range 20970-20979 by default; `GALION_T51_PORT_BASE`
/// moves the whole block (slot 236 re-runs it on 21060-21069).
fn port(off: u16) -> u16 {
    std::env::var("GALION_T51_PORT_BASE")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(20970)
        + off
}
fn nerve_port() -> u16 {
    port(0)
}
fn cortex_port() -> u16 {
    port(1)
}
fn proxy_port() -> u16 {
    port(2)
}
fn cortex_b_port() -> u16 {
    port(3)
}
fn dead_port() -> u16 {
    port(9)
}

const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds for journal timestamps; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn zagreb() -> String {
    zagreb_time::from_unix(now_unix() as i64)
}

fn log(msg: impl AsRef<str>) {
    println!("[t51 {}] {}", zagreb(), msg.as_ref());
}

// ------------------------------------------------------------------ http

fn http_raw(port: u16, method: &str, path: &str, body: &[u8], timeout: Duration) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), timeout) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if s.write_all(head.as_bytes()).is_err() {
        return (0, String::new());
    }
    let _ = s.write_all(body);
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

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    http_raw(port, method, path, body.as_bytes(), Duration::from_secs(15))
}

fn get(port: u16, path: &str) -> Value {
    let (c, b) = http(port, "GET", path, "");
    if c != 200 {
        return Value::Null;
    }
    serde_json::from_str(&b).unwrap_or(Value::Null)
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

fn n(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

// ------------------------------------------------------------------ processes

struct Proc {
    child: Child,
    name: &'static str,
}
impl Proc {
    fn pid(&self) -> u32 {
        self.child.id()
    }
    fn signal(&self, sig: &str) {
        let ok = Command::new("kill")
            .args([sig, &self.pid().to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "kill {sig} {}", self.name);
    }
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
    fn stop(&mut self) {
        let _ = Command::new("kill")
            .args(["-CONT", &self.pid().to_string()])
            .stderr(Stdio::null())
            .status();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Proc {
    fn drop(&mut self) {
        self.stop();
    }
}

fn work_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cortex-t51-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start_nerve(bin: &Path, dir: &Path, fifo: &Path, dedupe_s: u64, tag: &str) -> Proc {
    let data = dir.join(format!("nerve-data-{tag}"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"t51-dead","port":{},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#, dead_port()),
    )
    .unwrap();
    let stderr = std::fs::File::create(dir.join(format!("nerve-{tag}.log"))).unwrap();
    let child = Command::new(bin)
        .args([
            "serve",
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{}", nerve_port()),
            "--log",
            &dir.join(format!("nerve-events-{tag}.jsonl"))
                .display()
                .to_string(),
        ])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_DATA_DIR", &data)
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
        .env("NERVE_EVT_DEDUPE", dedupe_s.to_string())
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("spawn nerve");
    let p = Proc {
        child,
        name: "nerve",
    };
    wait_until("nerve /health", 30, || {
        http(nerve_port(), "GET", "/health", "").0 == 200
    });
    p
}

fn start_cortex(dir: &Path, port: u16, nerve_port: u16, tag: &str) -> Proc {
    let data = dir.join(format!("cortex-data-{tag}"));
    let stderr = std::fs::File::create(dir.join(format!("cortex-{tag}.log"))).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("CORTEX_LISTEN", format!("127.0.0.1:{port}"))
        .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{nerve_port}"))
        .env("CORTEX_DATA_DIR", &data)
        .env("CORTEX_COALESCE_MS", "500")
        // T5.1 keeps the pre-slot-235 transport (no ring re-read) so its S2a
        // still records finding F1; real_flap_dedupe.rs is the before/after.
        .env("CORTEX_REPEAT_POLL_S", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("spawn cortex");
    let p = Proc {
        child,
        name: "cortex",
    };
    wait_until("cortex /health", 20, || {
        http(port, "GET", "/health", "").0 == 200
    });
    p
}

// ------------------------------------------------------------------ journal FIFO

struct Journal(std::fs::File);
impl Journal {
    fn open(fifo: &Path) -> Journal {
        Journal(std::fs::OpenOptions::new().write(true).open(fifo).unwrap())
    }
    fn raw(&mut self, line: &str) {
        writeln!(self.0, "{line}").unwrap();
        self.0.flush().unwrap();
    }
    fn entry(&mut self, id: &str, unit: &str, extra: Value) {
        let mut v = json!({
            "MESSAGE_ID": id, "UNIT": unit, "MESSAGE": format!("{unit}: t51 chaos"), "_PID": "1",
            "__REALTIME_TIMESTAMP": (now_unix() * 1_000_000).to_string(),
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        self.raw(&v.to_string());
    }
    /// A unit killed by SIGKILL: main process exit + failure result.
    fn kill(&mut self, unit: &str) {
        self.entry(
            ID_PROCESS_EXIT,
            unit,
            json!({"EXIT_CODE": "killed", "EXIT_STATUS": "9"}),
        );
        self.entry(ID_FAILURE_RESULT, unit, json!({"UNIT_RESULT": "signal"}));
    }
    fn failed(&mut self, unit: &str) {
        self.entry(ID_FAILURE_RESULT, unit, json!({"UNIT_RESULT": "exit-code"}));
    }
}

fn mkfifo(p: &Path) {
    assert!(Command::new("mkfifo").arg(p).status().unwrap().success());
}

// ------------------------------------------------------------------ cortex views

/// Decisions on one subject, newest first, without the R10 echoes of our
/// own action requests (the nerve publishes every `action.result`).
fn decisions_for(port: u16, subject: &str) -> Vec<Value> {
    get(port, "/decisions?n=500")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["subject"] == subject && d["rule"] != "R10-remediation")
        .collect()
}

fn nerve_actions_for(unit: &str) -> Vec<Value> {
    get(nerve_port(), "/actions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a["unit"] == unit)
        .collect()
}

fn nerve_streams() -> u64 {
    n(&get(nerve_port(), "/stats"), "streams")
}

// ------------------------------------------------------------------ sockets

/// Sockets of `pid` whose remote end is not loopback / unspecified (tcp,
/// tcp6, udp, udp6 of the process's network namespace, matched by inode).
fn non_loopback_sockets(pid: u32) -> Vec<String> {
    let mut inodes = HashSet::new();
    if let Ok(rd) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        for e in rd.flatten() {
            if let Ok(t) = std::fs::read_link(e.path()) {
                let t = t.display().to_string();
                if let Some(i) = t.strip_prefix("socket:[").and_then(|x| x.strip_suffix(']')) {
                    inodes.insert(i.to_string());
                }
            }
        }
    }
    let mut bad = vec![];
    for (f, v6) in [
        ("tcp", false),
        ("tcp6", true),
        ("udp", false),
        ("udp6", true),
    ] {
        let Ok(t) = std::fs::read_to_string(format!("/proc/{pid}/net/{f}")) else {
            continue;
        };
        for l in t.lines().skip(1) {
            let c: Vec<&str> = l.split_whitespace().collect();
            if c.len() < 10 || !inodes.contains(c[9]) {
                continue;
            }
            let rem = c[2].split(':').next().unwrap_or("");
            if !loopback_hex(rem, v6) {
                bad.push(format!("{f} {} -> {}", c[1], c[2]));
            }
        }
    }
    bad
}

fn loopback_hex(h: &str, v6: bool) -> bool {
    let bytes: Vec<u8> = (0..h.len() / 2)
        .filter_map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok())
        .collect();
    // /proc prints each 32-bit word in host (little-endian) order.
    let ip: Vec<u8> = bytes
        .chunks(4)
        .flat_map(|w| w.iter().rev().copied())
        .collect();
    if ip.iter().all(|b| *b == 0) {
        return true;
    }
    if !v6 {
        return ip.first() == Some(&127);
    }
    let lo6 = ip[..15].iter().all(|b| *b == 0) && ip[15] == 1;
    let mapped = ip[..10].iter().all(|b| *b == 0) && ip[10] == 0xff && ip[11] == 0xff;
    lo6 || (mapped && ip[12] == 127)
}

// ------------------------------------------------------------------ latency proxy

/// Std-only TCP proxy that sleeps `delay_ms` before forwarding each chunk
/// in either direction (a "latency toxic"; idea from Toxiproxy, no code copied).
fn latency_proxy(listen: u16, upstream: u16, delay_ms: Arc<AtomicU64>, stop: Arc<AtomicBool>) {
    let l = TcpListener::bind(("127.0.0.1", listen)).expect("proxy bind");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((down, _)) => {
                    let _ = down.set_nonblocking(false);
                    let Ok(up) = TcpStream::connect(("127.0.0.1", upstream)) else {
                        let _ = down.shutdown(Shutdown::Both);
                        continue;
                    };
                    for (mut a, mut b) in [
                        (down.try_clone().unwrap(), up.try_clone().unwrap()),
                        (up, down),
                    ] {
                        let d = delay_ms.clone();
                        std::thread::spawn(move || {
                            let mut buf = [0u8; 16 * 1024];
                            loop {
                                match a.read(&mut buf) {
                                    Ok(0) | Err(_) => break,
                                    Ok(k) => {
                                        std::thread::sleep(Duration::from_millis(
                                            d.load(Ordering::SeqCst),
                                        ));
                                        if b.write_all(&buf[..k]).is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            let _ = b.shutdown(Shutdown::Write);
                        });
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    });
}

// ------------------------------------------------------------------ the scenario run

fn gate() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (and GALION_NERVE_BIN)");
        return None;
    }
    let bin = std::env::var("GALION_NERVE_BIN")
        .expect("GALION_REAL=1 needs GALION_NERVE_BIN=<release openmind-nerve from swarm/nerve-event-driven>");
    let bin = PathBuf::from(bin);
    assert!(bin.is_file(), "GALION_NERVE_BIN {} missing", bin.display());
    Some(bin)
}

fn assert_llm_off(port: u16, who: &str) {
    let s = get(port, "/stats");
    assert_eq!(n(&s, "llm_calls"), 0, "{who}: {s}");
    assert_eq!(s["llm"]["enabled"], false, "{who}: {s}");
    assert_eq!(
        s["llm_spend_usd"].as_f64().unwrap_or(0.0),
        0.0,
        "{who}: {s}"
    );
    assert_eq!(n(&s, "actions_executed"), 0, "{who}: {s}");
}

#[test]
fn chaos_t51_real_cortex_and_nerve() {
    let Some(nerve_bin) = gate() else { return };
    for p in [nerve_port(), cortex_port(), proxy_port(), cortex_b_port()] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (slot 227 range 20970-20979)"
        );
    }
    let dir = work_dir("chaos");
    let fifo = dir.join("journal.fifo");
    mkfifo(&fifo);
    let mut summary = serde_json::Map::new();

    // ---- start: nerve A (default 600 s dedupe, as on the VPS) + cortex
    let mut nerve = start_nerve(&nerve_bin, &dir, &fifo, 600, "a");
    let mut j = Journal::open(&fifo);
    let mut cortex = start_cortex(&dir, cortex_port(), nerve_port(), "a");
    wait_until("cortex subscribed to the nerve stream", 20, || {
        nerve_streams() == 1
    });
    let h = get(cortex_port(), "/health");
    assert_eq!(h["mode"], "dry-run", "{h}");
    assert_eq!(h["llm_enabled"], false, "{h}");
    log(format!(
        "started nerve pid {} cortex pid {} (dry-run, llm off)",
        nerve.pid(),
        cortex.pid()
    ));
    // let the nerve warm-up (dead component incident) settle into the cortex
    std::thread::sleep(Duration::from_secs(3));

    // ---- S1 unit kill
    let t = Instant::now();
    j.kill("tuner.service");
    let el = wait_until("S1 decision on tuner", 15, || {
        !decisions_for(cortex_port(), "unit:tuner.service").is_empty()
    });
    std::thread::sleep(Duration::from_millis(1200));
    let d = decisions_for(cortex_port(), "unit:tuner.service");
    assert_eq!(d.len(), 1, "one incident -> one decision: {d:?}");
    let d0 = &d[0];
    assert_eq!(d0["rule"], "R3-known-crash", "{d0}");
    assert_eq!(
        d0["events"], 2,
        "exit + failed grouped into one subject: {d0}"
    );
    assert_eq!(d0["dry_run"], true);
    assert_eq!(d0["action"]["by"], "nerve", "{d0}");
    assert_eq!(d0["action"]["status"], "simulated", "{d0}");
    let na = nerve_actions_for("tuner.service");
    assert_eq!(na.len(), 1, "{na:?}");
    assert_eq!(na[0]["result"], "dry-run");
    assert_eq!(na[0]["source"], "gaia-cortex");
    log(format!(
        "S1 unit kill: {} {} by {} in {} ms; nerve would_run {:?}",
        d0["rule"],
        d0["action"]["status"],
        d0["action"]["by"],
        el.as_millis(),
        na[0]["would_run"]
    ));
    summary.insert(
        "S1".into(),
        json!({"rule": d0["rule"], "events": d0["events"], "status": d0["action"]["status"], "ms": t.elapsed().as_millis() as u64, "incident": d0["incident"]}),
    );

    // ---- S2a restart loop under the default nerve dedupe (600 s)
    let dedup0 = n(&get(nerve_port(), "/stats"), "deduped");
    for _ in 0..5 {
        j.failed("tuner.service");
        std::thread::sleep(Duration::from_millis(700));
    }
    std::thread::sleep(Duration::from_secs(2));
    let dedup1 = n(&get(nerve_port(), "/stats"), "deduped");
    let d = decisions_for(cortex_port(), "unit:tuner.service");
    assert!(
        dedup1 >= dedup0 + 5,
        "nerve folded the loop: {dedup0} -> {dedup1}"
    );
    assert_eq!(
        d.len(),
        1,
        "FINDING F1 (current behaviour): the nerve folds repeats for 600 s, the cortex never sees failures 2..6"
    );
    log(format!(
        "S2a restart loop, nerve dedupe 600 s: 5 more failures -> nerve deduped +{}, cortex decisions still {} (circuit never opens)",
        dedup1 - dedup0,
        d.len()
    ));
    summary.insert(
        "S2a".into(),
        json!({"failures_written": 5, "nerve_deduped": dedup1 - dedup0, "cortex_decisions": d.len()}),
    );

    // ---- S3 protected pieces: one burst each, alert only, no action request
    let acts_before = n(&get(cortex_port(), "/stats"), "actions_requested");
    let mut s3 = vec![];
    for u in [
        "ssh.service",
        "cloudflared.service",
        "postfix.service",
        "galion-backup.service",
        "hades-gate.service",
    ] {
        j.kill(u);
        let subj = format!("unit:{u}");
        wait_until(&format!("S3 decision {u}"), 15, || {
            !decisions_for(cortex_port(), &subj).is_empty()
        });
        let d = decisions_for(cortex_port(), &subj);
        assert_eq!(d[0]["rule"], "R1-protected", "{u}: {}", d[0]);
        assert_eq!(d[0]["alert"], true, "{u}");
        assert!(d[0]["action"].is_null(), "{u}: never an action: {}", d[0]);
        assert!(nerve_actions_for(u).is_empty(), "{u}");
        s3.push(json!({"unit": u, "rule": d[0]["rule"], "reason": d[0]["reason"]}));
        std::thread::sleep(Duration::from_millis(800));
    }
    // a path event naming the protected working copy
    let (c, _) = http(
        nerve_port(),
        "POST",
        "/event",
        r#"{"source":"t51","kind":"file.changed","severity":"high","data":{"path":"/opt/galion/openmind/.git/HEAD","unit":"galion-one.service"}}"#,
    );
    assert_eq!(c, 200);
    wait_until("S3 /opt/galion/openmind decision", 15, || {
        decisions_for(cortex_port(), "unit:galion-one.service")
            .iter()
            .any(|d| d["rule"] == "R1-protected")
    });
    assert!(nerve_actions_for("galion-one.service").is_empty());
    let acts_after = n(&get(cortex_port(), "/stats"), "actions_requested");
    assert_eq!(acts_before, acts_after, "protected pieces never act");
    // how long does the S1 incident stay open? (the correlator resolves after 2 quiet batches)
    let s1_inc = summary["S1"]["incident"].clone();
    let inc = get(cortex_port(), "/incidents");
    let s1_open = inc["open"]
        .as_array()
        .is_some_and(|a| a.iter().any(|i| i["id"] == s1_inc));
    let s1_resolved = inc["resolved"]
        .as_array()
        .is_some_and(|a| a.iter().any(|i| i["id"] == s1_inc));
    log(format!("S3: S1 incident {s1_inc} open {s1_open} resolved {s1_resolved} after 6 unrelated bursts (no new tuner event)"));
    summary.insert(
        "S1_incident_after_S3".into(),
        json!({"open": s1_open, "resolved": s1_resolved}),
    );
    log(format!(
        "S3 protected: {} subjects alert-only, 0 action requests",
        s3.len() + 1
    ));
    summary.insert("S3".into(), json!(s3));

    // ---- S4 grouping: 4 stack units in one burst -> one stack-wide decision
    let dec0 = n(&get(cortex_port(), "/stats"), "decisions");
    let acts0 = n(&get(cortex_port(), "/stats"), "actions_requested");
    for u in [
        "hermes-rs.service",
        "console.service",
        "link-watch-rs.service",
        "openmind-memory.service",
    ] {
        j.kill(u);
    }
    wait_until("S4 stack-wide decision", 15, || {
        get(cortex_port(), "/decisions?n=50")
            .as_array()
            .is_some_and(|a| a.iter().any(|d| d["rule"] == "R0-stack-wide"))
    });
    std::thread::sleep(Duration::from_millis(1500));
    let st = get(cortex_port(), "/stats");
    let sw: Vec<Value> = get(cortex_port(), "/decisions?n=50")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["rule"] == "R0-stack-wide")
        .collect();
    assert_eq!(sw.len(), 1, "one stack-wide decision: {sw:?}");
    assert_eq!(
        n(&st, "actions_requested"),
        acts0,
        "no restarts on stack-wide"
    );
    assert_eq!(sw[0]["escalate"], true);
    log(format!(
        "S4 grouping: 4 units/8 events -> decisions +{}, R0-stack-wide x1 (incident {}), 0 restarts",
        n(&st, "decisions") - dec0,
        sw[0]["incident"]
    ));
    summary.insert(
        "S4".into(),
        json!({"units": 4, "decisions_added": n(&st, "decisions") - dec0, "stack_wide": 1, "incident": sw[0]["incident"]}),
    );

    // ---- S5 malformed event burst
    let c0 = get(cortex_port(), "/stats");
    let ns0 = get(nerve_port(), "/stats");
    // into the nerve's journal source
    for i in 0..500 {
        j.raw(&format!("garbage {i} \u{1} {{{{ not json"));
    }
    j.raw(&format!(
        r#"{{"MESSAGE_ID":"{ID_FAILURE_RESULT}","UNIT":"","MESSAGE":"empty unit"}}"#
    ));
    j.raw(&format!(
        r#"{{"MESSAGE_ID":"{ID_FAILURE_RESULT}","MESSAGE":{}}}"#,
        "1".repeat(64)
    ));
    j.raw(&"x".repeat(1 << 20));
    j.raw(&format!("{}{}", "[".repeat(10_000), "]".repeat(10_000)));
    // into the cortex push ingest
    let garbage: String = (0..1000).map(|i| format!("not json {i}\n")).collect();
    let (gc, gb) = http(cortex_port(), "POST", "/v1/events", &garbage);
    let nested = format!("{}{}", "[".repeat(50_000), "]".repeat(50_000));
    let (nc, _) = http(cortex_port(), "POST", "/v1/events", &nested);
    let big = vec![b'{'; 2 * 1024 * 1024];
    let (bc, _) = http_raw(
        cortex_port(),
        "POST",
        "/v1/events",
        &big,
        Duration::from_secs(15),
    );
    let (uc, _) = http_raw(
        cortex_port(),
        "POST",
        "/v1/events",
        &[0xff, 0xfe, 0x00, 0x7b, 0x22],
        Duration::from_secs(5),
    );
    let arr: Vec<Value> = (0..1500)
        .map(|i| json!({"source":"t51","kind":"unit.failed","severity":"high","unit":"tuner.service","seq": 900_000 + i}))
        .collect();
    let (ac, ab) = http(
        cortex_port(),
        "POST",
        "/v1/events",
        &Value::Array(arr).to_string(),
    );
    // injection attempts: units that are not exactly on the allowlist
    let inj = [
        "tuner.service; rm -rf /",
        "tuner.service\nExecStart=/bin/sh",
        "../../../../etc/passwd",
        "tuner.service --no-block cloudflared.service",
        "$(reboot).service",
    ];
    // one burst each, so every unit goes through the per-unit rules (not R0)
    let mut ic = 0;
    let mut ib = String::new();
    for (i, u) in inj.iter().enumerate() {
        let body = json!({"source":"t51-inject","kind":"unit.failed","severity":"critical","unit":u,"seq": 800_000 + i}).to_string();
        let (c, b) = http(cortex_port(), "POST", "/v1/events", &body);
        ic = c;
        ib = b;
        std::thread::sleep(Duration::from_millis(900));
    }
    // raw non-HTTP bytes and a connection flood (slowloris)
    if let Ok(mut s) = TcpStream::connect(("127.0.0.1", cortex_port())) {
        let _ = s.write_all(&[0u8, 0xff, 0x13, 0x37, b'\r', b'\n', b'\r', b'\n']);
    }
    let flood: Vec<TcpStream> = (0..40)
        .filter_map(|_| TcpStream::connect(("127.0.0.1", cortex_port())).ok())
        .collect();
    let during = http(cortex_port(), "GET", "/health", "").0;
    let t_flood = Instant::now();
    let rec = wait_until("cortex /health after the flood", 20, || {
        http(cortex_port(), "GET", "/health", "").0 == 200
    });
    drop(flood);
    let _ = t_flood;
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        nerve.alive() && cortex.alive(),
        "both survive the malformed burst"
    );
    assert_eq!(http(nerve_port(), "GET", "/health", "").0, 200);
    let c1 = get(cortex_port(), "/stats");
    let ns1 = get(nerve_port(), "/stats");
    // no injection unit got an action request or a restart outcome
    let decs = get(cortex_port(), "/decisions?n=500");
    let mut inj_decisions = 0;
    for d in decs.as_array().unwrap() {
        let u = d["unit"].as_str().unwrap_or("");
        if [";", "\n", "/", "$(", " "]
            .iter()
            .any(|bad| u.contains(bad))
        {
            inj_decisions += 1;
            assert!(d["action"].is_null(), "injection unit {u:?} acted: {d}");
        }
    }
    let inj_rules: Vec<Value> = decs
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| {
            d["subject"]
                .as_str()
                .is_some_and(|s| [";", "\n", "/", "$(", " "].iter().any(|b| s.contains(b)))
        })
        .map(|d| json!([d["unit"], d["rule"], d["outcome"]]))
        .collect();
    log(format!("S5 injection decisions: {inj_rules:?}"));
    let tuner: Vec<Value> = decisions_for(cortex_port(), "unit:tuner.service")
        .iter()
        .map(|d| json!([d["rule"], d["events"], d["outcome"]]))
        .collect();
    log(format!(
        "S5 tuner decisions after the 1500-array (newest first): {tuner:?}"
    ));
    let restart_units: Vec<String> = get(nerve_port(), "/actions")
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["unit"].as_str().map(str::to_string))
        .collect();
    for u in &restart_units {
        assert!(
            openmind_nerve::reflex::ALLOW.contains(&u.as_str()),
            "only exact allowlisted units reach the nerve: {u:?}"
        );
    }
    let gv: Value = serde_json::from_str(&gb).unwrap_or(Value::Null);
    let av: Value = serde_json::from_str(&ab).unwrap_or(Value::Null);
    let iv: Value = serde_json::from_str(&ib).unwrap_or(Value::Null);
    log(format!(
        "S5 malformed: nerve journal 504 bad lines + 1 MiB line + 20k-deep nest -> nerve alive, published +{}; \
         cortex garbage x1000 -> {gc} {gv}; nested 50k -> {nc}; 2 MiB -> {bc}; bad utf8 -> {uc}; 1500-array -> {ac} {av}; \
         injection x5 (one burst each) -> last {ic} {iv} ({inj_decisions} decisions, all without action); flood 40 conns -> /health {during} during, 200 after {} ms; \
         cortex events_invalid +{}, actions +{}",
        n(&ns1, "seq").saturating_sub(n(&ns0, "seq")),
        rec.as_millis(),
        n(&c1, "events_invalid") - n(&c0, "events_invalid"),
        n(&c1, "actions_requested") - n(&c0, "actions_requested"),
    ));
    assert_eq!(gc, 400, "all-garbage body is a 400");
    assert_eq!(gv["invalid"], 1000, "{gv}");
    assert_ne!(nc, 200, "50k-deep nesting rejected, not accepted");
    assert_eq!(bc, 413, "2 MiB body over the 1 MiB cap");
    assert_eq!(ac, 202);
    assert_eq!(
        (av["accepted"].as_u64(), av["invalid"].as_u64()),
        (Some(1000), Some(500)),
        "over-limit items reported: {av}"
    );
    summary.insert(
        "S5".into(),
        json!({"garbage": [gc, gv], "nested": nc, "big": bc, "utf8": uc, "array1500": [ac, av], "inject": [ic, iv, inj_decisions],
               "flood_health_during": during, "flood_recover_ms": rec.as_millis() as u64,
               "cortex_invalid_added": n(&c1, "events_invalid") - n(&c0, "events_invalid"),
               "cortex_actions_added": n(&c1, "actions_requested") - n(&c0, "actions_requested"),
               "nerve_published_added": n(&ns1, "seq").saturating_sub(n(&ns0, "seq"))}),
    );

    // ---- S6 slow upstream: cortex B through a latency proxy
    let delay = Arc::new(AtomicU64::new(1000));
    let stop = Arc::new(AtomicBool::new(false));
    latency_proxy(proxy_port(), nerve_port(), delay.clone(), stop.clone());
    let mut cortex_b = start_cortex(&dir, cortex_b_port(), proxy_port(), "b");
    wait_until("cortex B subscribed via proxy", 30, || nerve_streams() == 2);
    let t = Instant::now();
    j.kill("galion-voice.service");
    wait_until("S6 B decision (1 s latency)", 30, || {
        !decisions_for(cortex_b_port(), "unit:galion-voice.service").is_empty()
    });
    let slow_ok = t.elapsed();
    let db = decisions_for(cortex_b_port(), "unit:galion-voice.service");
    assert_eq!(db[0]["rule"], "R3-known-crash", "{}", db[0]);
    assert_eq!(
        db[0]["action"]["by"], "nerve",
        "1 s each way < 3 s timeout: {}",
        db[0]
    );
    delay.store(4000, Ordering::SeqCst);
    let t = Instant::now();
    let (pc, _) = http(
        cortex_b_port(),
        "POST",
        "/v1/events",
        r#"{"source":"t51-push","kind":"unit.failed","severity":"high","unit":"galion-health.service","seq":700001}"#,
    );
    assert_eq!(pc, 202);
    wait_until("S6 B decision (4 s latency)", 40, || {
        !decisions_for(cortex_b_port(), "unit:galion-health.service").is_empty()
    });
    let slow_to = t.elapsed();
    let db2 = decisions_for(cortex_b_port(), "unit:galion-health.service");
    log(format!(
        "S6 slow upstream: 1 s latency -> {} by {} after {} ms; 4 s latency -> {} by {} ({}) after {} ms",
        db[0]["action"]["status"], db[0]["action"]["by"], slow_ok.as_millis(),
        db2[0]["action"]["status"], db2[0]["action"]["by"], db2[0]["action"]["reason"], slow_to.as_millis()
    ));
    assert_eq!(
        db2[0]["action"]["by"], "unknown",
        "request reached the proxy, reply lost -> outcome unknown: {}",
        db2[0]
    );
    assert_llm_off(cortex_b_port(), "cortex B");
    summary.insert(
        "S6".into(),
        json!({"latency_1s": {"status": db[0]["action"]["status"], "by": db[0]["action"]["by"], "ms": slow_ok.as_millis() as u64},
               "latency_4s": {"status": db2[0]["action"]["status"], "by": db2[0]["action"]["by"], "reason": db2[0]["action"]["reason"], "ms": slow_to.as_millis() as u64}}),
    );
    stop.store(true, Ordering::SeqCst);
    cortex_b.stop();
    delay.store(0, Ordering::SeqCst);

    // ---- S7 hung upstream: SIGSTOP the nerve while the cortex must act
    nerve.signal("-STOP");
    let t = Instant::now();
    let (pc, _) = http(
        cortex_port(),
        "POST",
        "/v1/events",
        r#"{"source":"t51-push","kind":"unit.failed","severity":"high","unit":"galion-desk.service","seq":700101}"#,
    );
    assert_eq!(pc, 202, "push ingest answers while the nerve is hung");
    std::thread::sleep(Duration::from_millis(900)); // inside the decision (POST /actions blocked)
    let th = Instant::now();
    let (hc, hb) = http(cortex_port(), "GET", "/health", "");
    let hv: Value = serde_json::from_str(&hb).unwrap_or(Value::Null);
    let health_ms = th.elapsed().as_millis() as u64;
    let ts = Instant::now();
    let sc = http(cortex_port(), "GET", "/stats", "").0;
    let stats_ms = ts.elapsed().as_millis() as u64;
    wait_until("S7 decision while the nerve is hung", 40, || {
        !decisions_for(cortex_port(), "unit:galion-desk.service").is_empty()
    });
    let hung_ms = t.elapsed().as_millis() as u64;
    let dh = decisions_for(cortex_port(), "unit:galion-desk.service");
    nerve.signal("-CONT");
    // the nerve wakes up and may still process the request the cortex gave up on
    std::thread::sleep(Duration::from_secs(3));
    let late = nerve_actions_for("galion-desk.service");
    log(format!(
        "S7 hung nerve: decision {} {} by {} after {hung_ms} ms; during the hang /health {hc} in {health_ms} ms, /stats {sc} in {stats_ms} ms; \
         after SIGCONT the nerve logged {} late action(s) for the request the cortex marked unknown: {:?}",
        dh[0]["rule"], dh[0]["action"]["status"], dh[0]["action"]["by"], late.len(),
        late.iter().map(|a| a["result"].clone()).collect::<Vec<_>>()
    ));
    assert_eq!(
        dh[0]["action"]["by"], "unknown",
        "request sent, reply lost: {}",
        dh[0]
    );
    assert_eq!(dh[0]["escalate"], true, "{}", dh[0]);
    assert_eq!(hc, 200);
    assert_eq!(
        hv["busy"], true,
        "health answers from the snapshot while deciding: {hv}"
    );
    assert!(
        health_ms < 1000,
        "/health must not wait for the decider: {health_ms} ms"
    );
    assert!(
        stats_ms < 1000,
        "/stats must not wait for the decider either (slot 236): {stats_ms} ms"
    );
    assert_eq!(n(&get(cortex_port(), "/stats"), "actions_unknown"), 1);
    summary.insert(
        "S7".into(),
        json!({"decision_ms": hung_ms, "status": dh[0]["action"]["status"], "by": dh[0]["action"]["by"],
               "health_ms_during": health_ms, "health_busy": hv["busy"], "stats_ms_during": stats_ms, "late_nerve_actions": late.len()}),
    );
    // the stream survives the stop/cont (same TCP connection)
    j.failed("galion-one.service");
    wait_until("stream alive after SIGCONT", 20, || {
        decisions_for(cortex_port(), "unit:galion-one.service")
            .iter()
            .any(|d| d["rule"] != "R1-protected")
    });

    // ---- S8 nerve restart mid-incident
    let open_before: Vec<Value> = get(cortex_port(), "/incidents")["open"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|i| i["cause"].clone())
        .collect();
    let seq_before = n(&get(nerve_port(), "/stats"), "seq");
    let desk_base = decisions_for(cortex_port(), "unit:galion-desk.service").len();
    // incident in flight: a kill the cortex is still coalescing
    j.kill("galion-health.service");
    std::thread::sleep(Duration::from_millis(150));
    nerve.signal("-KILL");
    let _ = nerve.child.wait();
    drop(j);
    log(format!(
        "S8: nerve A SIGKILLed at seq {seq_before} mid-incident (cortex open incidents {open_before:?})"
    ));
    std::thread::sleep(Duration::from_secs(2));
    nerve = start_nerve(&nerve_bin, &dir, &fifo, 1, "b");
    j = Journal::open(&fifo);
    let t = Instant::now();
    wait_until("cortex re-subscribed to the new nerve", 90, || {
        nerve_streams() == 1
    });
    let resub_ms = t.elapsed().as_millis() as u64;
    std::thread::sleep(Duration::from_secs(1));
    let health_dec = decisions_for(cortex_port(), "unit:galion-health.service");
    // restart loop on the new nerve (1 s dedupe), failures 1.3 s apart
    let mut seen = vec![];
    for i in 0..4 {
        j.failed("galion-desk.service");
        let want = desk_base + i + 1;
        let t = Instant::now();
        let ok = (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            decisions_for(cortex_port(), "unit:galion-desk.service").len() >= want
        });
        seen.push(json!({"failure": i + 1, "decided": ok, "ms": t.elapsed().as_millis() as u64}));
        std::thread::sleep(Duration::from_millis(1300));
    }
    let nseq = n(&get(nerve_port(), "/stats"), "seq");
    let dd = decisions_for(cortex_port(), "unit:galion-desk.service");
    let after: Vec<&Value> = dd.iter().take(dd.len() - desk_base).collect();
    let rules: Vec<String> = after
        .iter()
        .rev()
        .map(|d| {
            format!(
                "{}:{}",
                d["rule"].as_str().unwrap_or(""),
                d["action"]["status"].as_str().unwrap_or("-")
            )
        })
        .collect();
    let st = get(cortex_port(), "/stats");
    log(format!(
        "S8 nerve restart: re-subscribed after {resub_ms} ms; new nerve seq {nseq} (old {seq_before}); \
         in-flight galion-health decisions {:?}; galion-desk after restart (oldest first) {rules:?}; per failure {seen:?}; suppressed {}",
        health_dec.iter().map(|d| format!("{}:{}:{}", d["rule"].as_str().unwrap_or(""), d["action"]["status"].as_str().unwrap_or("-"), d["action"]["by"].as_str().unwrap_or("-"))).collect::<Vec<_>>(),
        n(&st, "decisions_suppressed")
    ));
    summary.insert(
        "S8".into(),
        json!({"old_seq": seq_before, "new_seq": nseq, "resubscribe_ms": resub_ms, "open_before": open_before,
               "in_flight_decisions": health_dec.len(), "galion_desk_after_restart": rules, "per_failure": seen}),
    );
    assert!(
        !after.is_empty(),
        "FINDING F2: after a nerve restart the cortex must see the new nerve's events (its seq restarts at 1)"
    );
    assert_eq!(n(&st, "nerve_restarts"), 1, "{st}");
    assert!(
        after.iter().any(|d| d["rule"] == "R2-circuit-open"),
        "restart loop opens the cortex circuit: {rules:?}"
    );
    let desk_acts = dd.iter().filter(|d| !d["action"].is_null()).count();
    assert!(
        desk_acts <= 3,
        "cortex cap (2/unit/h since slot 235): {desk_acts}"
    );
    let esc = get(cortex_port(), "/escalations");
    assert!(
        esc["open"].as_array().is_some_and(|a| a
            .iter()
            .any(|e| e["subject"] == "unit:galion-desk.service" && e["rule"] == "R2-circuit-open")),
        "{esc}"
    );

    // ---- S9 LLM advisor stays off and capped
    let s0 = get(cortex_port(), "/stats");
    let (c, _) = http(
        cortex_port(),
        "POST",
        "/v1/events",
        r#"{"source":"t51-push","kind":"disk.crossed","severity":"critical","seq":700201,"data":{"path":"/var/lib/t51","pct":99}}"#,
    );
    assert_eq!(c, 202);
    let (c, _) = http(
        cortex_port(),
        "POST",
        "/v1/events",
        r#"{"source":"t51-push","kind":"kernel.error","severity":"critical","seq":700202,"data":{"msg":"t51 ambiguous"}}"#,
    );
    assert_eq!(c, 202);
    wait_until("S9 advisor-eligible decisions", 15, || {
        n(&get(cortex_port(), "/stats"), "llm_skipped_disabled") > n(&s0, "llm_skipped_disabled")
    });
    let s1 = get(cortex_port(), "/stats");
    assert_llm_off(cortex_port(), "cortex A");
    let ns = get(nerve_port(), "/stats");
    assert_eq!(n(&ns, "llm_calls"), 0, "{ns}");
    assert_eq!(ns["config"]["escalate"]["paid_enabled"], false, "{ns}");
    let bad_c = non_loopback_sockets(cortex.pid());
    let bad_n = non_loopback_sockets(nerve.pid());
    assert!(bad_c.is_empty(), "cortex non-loopback sockets: {bad_c:?}");
    assert!(bad_n.is_empty(), "nerve non-loopback sockets: {bad_n:?}");
    let ledger =
        std::fs::read_to_string(dir.join("cortex-data-a/llm-spend.json")).unwrap_or_default();
    log(format!(
        "S9 llm: advisor-eligible skipped {} (disabled), llm_calls 0, refused {}, spend {}, endpoint guard {}; nerve paid slot off, nerve llm_calls 0; \
         non-loopback sockets cortex {} nerve {}; ledger {:?}",
        n(&s1, "llm_skipped_disabled") - n(&s0, "llm_skipped_disabled"),
        n(&s1, "llm_refused"),
        s1["llm_spend_usd"],
        s1["llm"]["endpoint_guard"],
        bad_c.len(),
        bad_n.len(),
        ledger.chars().take(200).collect::<String>()
    ));
    summary.insert(
        "S9".into(),
        json!({"skipped_disabled": n(&s1, "llm_skipped_disabled") - n(&s0, "llm_skipped_disabled"), "llm_calls": n(&s1, "llm_calls"),
               "spend": s1["llm_spend_usd"], "endpoint_guard": s1["llm"]["endpoint_guard"], "nerve_llm_calls": n(&ns, "llm_calls"),
               "non_loopback": bad_c.len() + bad_n.len()}),
    );

    // ---- totals + teardown
    let fin = get(cortex_port(), "/stats");
    summary.insert("cortex_stats".into(), fin.clone());
    summary.insert("nerve_stats".into(), get(nerve_port(), "/stats"));
    println!("T51-SUMMARY {}", Value::Object(summary));
    assert_eq!(n(&fin, "actions_executed"), 0);
    cortex.stop();
    nerve.stop();
    for p in [nerve_port(), cortex_port(), proxy_port(), cortex_b_port()] {
        wait_until(&format!("port {p} free"), 10, || {
            TcpListener::bind(("127.0.0.1", p)).is_ok()
        });
    }
    let _ = std::fs::remove_dir_all(&dir);
    log(format!(
        "teardown: processes killed, ports {}-{} free, work dir removed",
        nerve_port(),
        cortex_b_port()
    ));
}

/// The advisor cannot be switched on without the feature: config load refuses
/// `llm.enabled` without the `llm-advisor` feature, and the endpoint guard
/// refuses OpenRouter / Hermes in any build.
#[test]
fn llm_advisor_cannot_be_enabled() {
    if gate().is_none() {
        return;
    }
    let dir = work_dir("llm");
    let run = |envs: &[(&str, String)], args: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"));
        c.env_clear()
            .env("CORTEX_LISTEN", "127.0.0.1:20974")
            .env("CORTEX_DATA_DIR", &dir)
            .args(args);
        for (k, v) in envs {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stderr).to_string(),
            String::from_utf8_lossy(&o.stdout).to_string(),
        )
    };
    if cfg!(feature = "llm-advisor") {
        // Advisor compiled in (the live VPS build): CORTEX_LLM_ENABLED=1 with
        // the default River/DeepSeek endpoint is accepted at start, but with
        // no key in the (cleared) environment and no event it makes no call.
        let mut c = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"));
        c.env_clear()
            .env("CORTEX_LISTEN", "127.0.0.1:20974")
            .env("CORTEX_DATA_DIR", &dir)
            .env("CORTEX_LLM_ENABLED", "1")
            .arg("serve")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = c.spawn().unwrap();
        wait_until("advisor-on cortex /health", 15, || {
            TcpStream::connect(("127.0.0.1", 20974)).is_ok()
        });
        let h = get(20974, "/health");
        let s = get(20974, "/stats");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(h["llm_enabled"], true, "{h}");
        assert_eq!(h["mode"], "dry-run", "{h}");
        assert_eq!(n(&s, "llm_calls"), 0, "{s}");
        assert_eq!(s["llm_spend_usd"].as_f64().unwrap_or(0.0), 0.0, "{s}");
        log("CORTEX_LLM_ENABLED=1 (llm-advisor compiled) -> starts dry-run, llm_calls 0, spend 0, no key in env");
    } else {
        let (code, err, _) = run(&[("CORTEX_LLM_ENABLED", "1".into())], &["serve"]);
        assert_eq!(code, Some(2), "{err}");
        log(format!(
            "CORTEX_LLM_ENABLED=1 -> exit {code:?}: {}",
            err.trim()
        ));
    }
    for ep in [
        "https://openrouter.ai/api/v1",
        "http://127.0.0.1:8642/v1",
        "https://hermes.example/v1",
    ] {
        let cfg = dir.join("c.json");
        std::fs::write(
            &cfg,
            json!({"llm": {"enabled": true, "endpoint": ep}}).to_string(),
        )
        .unwrap();
        let (code, err, _) = run(&[], &["serve", "--config", cfg.to_str().unwrap()]);
        assert_eq!(code, Some(2), "{ep}: {err}");
        log(format!(
            "llm.enabled + {ep} -> exit {code:?}: {}",
            err.trim()
        ));
    }
    let (code, _, out) = run(&[], &["config"]);
    assert_eq!(code, Some(0));
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["llm"]["enabled"], false);
    assert_eq!(v["actions"], "dry-run");
    assert!(v["llm"]["budget_usd_total"].as_f64().unwrap() <= 15.0);
    assert!(!out.contains("sk-"), "no key material in config output");
    assert!(
        TcpListener::bind(("127.0.0.1", 20974)).is_ok(),
        "nothing left listening"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
