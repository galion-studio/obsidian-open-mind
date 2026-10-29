// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 347 (cortex-health-nerve-live, F-337-1): the REAL release
//! `gaia-cortex` against the REAL event-mode nerve (`openmind-nerve serve
//! --mode event`, release build of the nerve deploy candidate
//! `swarm/nerve-merge-6` 9baa450c), box only, loopback only, ports
//! 22170-22179.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve>`.
//! Optional negative control: `GALION_CORTEX_BASE_BIN=<release gaia-cortex
//! 73583abf>` (the old `/health` that said `nerve: true` when only
//! configured). Run:
//! `cargo test --release -p gaia-cortex --test real_health_nerve -- --nocapture`.
//!
//! Phases:
//!   P0 cortex started BEFORE the nerve: `nerve` false (`unreachable`); base: true
//!   P1 nerve starts: `nerve` true, stream `connected`, age small (stream reconnect backoff)
//!   P2 nerve SIGKILLed: `nerve` false within 2 s (`unreachable`, stream `down`); base: still true
//!   P3 nerve restarted on the same port: `nerve` true again within the reconnect backoff
//!   P4 nerve SIGSTOPped (hung, socket open): a cortex with `CORTEX_NERVE_STALE_S=5`
//!      reads `stale` after 5 s; the default cortex (660 s) still reads true (documented
//!      limit: two missed keepalives); SIGCONT, then SIGKILL -> both false
//!
//! Nothing here touches systemd, the VPS or the network beyond 127.0.0.1.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
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

const BLOCK: u16 = port_block(22170);
const NERVE_PORT: u16 = BLOCK;
const CORTEX_PORT: u16 = BLOCK + 1;
const STALE5_PORT: u16 = BLOCK + 2;
const BASE_PORT: u16 = BLOCK + 3;

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[r347 {}] {}",
        zagreb_time::from_unix(now_unix() as i64),
        msg.as_ref()
    );
}

fn http(port: u16, path: &str) -> (u16, String) {
    let t = Duration::from_secs(5);
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

fn health(port: u16) -> Value {
    let (c, b) = http(port, "/health");
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
        std::thread::sleep(Duration::from_millis(50));
    }
    t.elapsed()
}

struct Proc(Child);
impl Proc {
    fn kill9(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
    fn signal(&self, sig: &str) {
        assert!(Command::new("kill")
            .args([sig, &self.0.id().to_string()])
            .status()
            .unwrap()
            .success());
    }
}
impl Drop for Proc {
    fn drop(&mut self) {
        // A stopped process must be continued to die cleanly.
        let _ = Command::new("kill")
            .args(["-CONT", &self.0.id().to_string()])
            .status();
        self.kill9();
    }
}

fn start_nerve(bin: &Path, dir: &Path, fifo: &Path, tag: &str) -> Proc {
    let data = dir.join(format!("nerve-data-{tag}"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(dir.join("components.json"), "[]").unwrap();
    let stderr = std::fs::File::create(dir.join(format!("nerve-{tag}.log"))).unwrap();
    let child = Command::new(bin)
        .args([
            "serve",
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{NERVE_PORT}"),
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("spawn nerve");
    let p = Proc(child);
    wait_until("nerve /health", 30, || http(NERVE_PORT, "/health").0 == 200);
    p
}

fn start_cortex(bin: &Path, dir: &Path, port: u16, tag: &str, stale: Option<u64>) -> Proc {
    let data = dir.join(format!("cortex-data-{tag}"));
    let stderr = std::fs::File::create(dir.join(format!("cortex-{tag}.log"))).unwrap();
    let mut c = Command::new(bin);
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("CORTEX_LISTEN", format!("127.0.0.1:{port}"))
        .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
        .env("CORTEX_DATA_DIR", &data)
        .env("CORTEX_COALESCE_MS", "500")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    if let Some(s) = stale {
        c.env("CORTEX_NERVE_STALE_S", s.to_string());
    }
    let p = Proc(c.spawn().expect("spawn cortex"));
    wait_until("cortex /health", 20, || http(port, "/health").0 == 200);
    p
}

fn brief(h: &Value) -> String {
    let c = &h["nerve_contact"];
    format!(
        "nerve={} state={} stream={} age_s={} last_error={} ok={} fail={}",
        h["nerve"], c["state"], c["stream"], c["age_s"], c["last_error"], c["ok"], c["fail"]
    )
}

#[test]
fn real_health_nerve_reachability() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skipped: set GALION_REAL=1 and GALION_NERVE_BIN");
        return;
    }
    let nerve_bin = PathBuf::from(std::env::var("GALION_NERVE_BIN").expect("GALION_NERVE_BIN"));
    let base_bin = std::env::var("GALION_CORTEX_BASE_BIN")
        .ok()
        .map(PathBuf::from);
    let cortex_bin = PathBuf::from(env!("CARGO_BIN_EXE_gaia-cortex"));
    let dir = std::env::temp_dir().join(format!("cortex-r347-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("journal.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let mut record: Vec<Value> = vec![];

    // P0: cortex first, nothing on the nerve port.
    let _cx = start_cortex(&cortex_bin, &dir, CORTEX_PORT, "new", None);
    let _cx5 = start_cortex(&cortex_bin, &dir, STALE5_PORT, "stale5", Some(5));
    let _base = base_bin
        .as_ref()
        .map(|b| start_cortex(b, &dir, BASE_PORT, "base", None));
    wait_until("P0 first failed contact", 10, || {
        health(CORTEX_PORT)["nerve_contact"]["fail"].as_u64() >= Some(1)
    });
    let h = health(CORTEX_PORT);
    log(format!("P0 nerve not running: new {}", brief(&h)));
    assert_eq!(h["nerve"], false, "{h}");
    assert_eq!(h["nerve_contact"]["state"], "unreachable");
    let mut p0 = json!({"phase": "P0", "new": h});
    if base_bin.is_some() {
        let b = health(BASE_PORT);
        log(format!("P0 nerve not running: base nerve={}", b["nerve"]));
        assert_eq!(
            b["nerve"], true,
            "base must show the F-337-1 blind spot: {b}"
        );
        p0["base"] = b;
    }
    record.push(p0);

    // P1: nerve starts.
    let t = Instant::now();
    let mut nerve = start_nerve(&nerve_bin, &dir, &fifo, "a");
    let mut j = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    let up = wait_until("P1 nerve true", 90, || health(CORTEX_PORT)["nerve"] == true);
    wait_until("P1 stale5 true", 90, || {
        health(STALE5_PORT)["nerve"] == true
    });
    let h = health(CORTEX_PORT);
    log(format!(
        "P1 nerve up: true after {} ms (from nerve spawn; reconnect backoff) {}",
        up.as_millis(),
        brief(&h)
    ));
    assert_eq!(h["nerve_contact"]["stream"], "connected", "{h}");
    assert_eq!(h["nerve_contact"]["state"], "ok");
    let _ = t;
    record.push(json!({"phase": "P1", "ms_to_true": up.as_millis() as u64, "new": h}));

    // P2: SIGKILL the nerve.
    nerve.kill9();
    let t = Instant::now();
    let down = wait_until("P2 nerve false", 2, || {
        health(CORTEX_PORT)["nerve"] == false
    });
    let h = health(CORTEX_PORT);
    log(format!(
        "P2 nerve SIGKILLed: false after {} ms {}",
        down.as_millis(),
        brief(&h)
    ));
    assert_eq!(h["nerve_contact"]["state"], "unreachable", "{h}");
    assert_eq!(h["nerve_contact"]["stream"], "down", "{h}");
    let mut p2 = json!({"phase": "P2", "ms_to_false": down.as_millis() as u64, "new": h});
    if base_bin.is_some() {
        std::thread::sleep(Duration::from_millis(1500));
        let b = health(BASE_PORT);
        log(format!(
            "P2 nerve SIGKILLed {} ms ago: base nerve={}",
            t.elapsed().as_millis(),
            b["nerve"]
        ));
        assert_eq!(b["nerve"], true, "{b}");
        p2["base"] = b;
    }
    record.push(p2);

    // P3: nerve back on the same port.
    drop(j);
    let mut nerve = start_nerve(&nerve_bin, &dir, &fifo, "b");
    j = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    let back = wait_until("P3 nerve true again", 90, || {
        health(CORTEX_PORT)["nerve"] == true
    });
    wait_until("P3 stale5 true again", 90, || {
        health(STALE5_PORT)["nerve"] == true
    });
    let h = health(CORTEX_PORT);
    log(format!(
        "P3 nerve restarted: true after {} ms {}",
        back.as_millis(),
        brief(&h)
    ));
    assert_eq!(h["nerve_contact"]["stream"], "connected");
    record.push(json!({"phase": "P3", "ms_to_true": back.as_millis() as u64, "new": h}));

    // P4: hung nerve (SIGSTOP): socket stays open, no lines.
    nerve.signal("-STOP");
    let t = Instant::now();
    let stale = wait_until("P4 stale5 false", 15, || {
        health(STALE5_PORT)["nerve"] == false
    });
    let h5 = health(STALE5_PORT);
    let hd = health(CORTEX_PORT);
    log(format!(
        "P4 nerve SIGSTOPped: stale5 false after {} ms {} | default {}",
        stale.as_millis(),
        brief(&h5),
        brief(&hd)
    ));
    assert_eq!(h5["nerve_contact"]["state"], "stale", "{h5}");
    assert_eq!(h5["nerve_contact"]["stream"], "connected", "{h5}");
    assert!(
        stale >= Duration::from_secs(3),
        "stale too early: {stale:?}"
    );
    assert_eq!(hd["nerve"], true, "default 660 s window: {hd}");
    record.push(json!({"phase": "P4", "ms_to_stale": stale.as_millis() as u64, "stale5": h5, "default": hd, "since_stop_ms": t.elapsed().as_millis() as u64}));
    nerve.signal("-CONT");
    nerve.kill9();
    wait_until("P4 both false after kill", 5, || {
        health(CORTEX_PORT)["nerve"] == false && health(STALE5_PORT)["nerve"] == false
    });
    let h = health(CORTEX_PORT);
    log(format!("P4 end (SIGCONT + SIGKILL): {}", brief(&h)));
    record.push(json!({"phase": "P4-end", "new": h}));
    drop(j);

    if let Ok(out) = std::env::var("GALION_R347_RECORD") {
        std::fs::write(out, serde_json::to_string_pretty(&record).unwrap()).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
