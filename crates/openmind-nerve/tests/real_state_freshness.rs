// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 448 (nerve-state-ts-staleness): a quiet event-mode nerve is not a
//! stale nerve, and a nerve whose cycles really stop says so itself.
//!
//! A) event mode (heartbeat 60 s, max_wait 4 s): `/state` ts holds still
//!    while quiet, `/health` `state_age_s` grows, `state_fresh` stays true,
//!    `freshness.max_age_s` = 60 + 4 + 60 = 124 (basis event).
//! B) polling with a 600 s interval and `NERVE_STATE_MAX_AGE=10`: after
//!    12 s without a cycle `/health` says `state_fresh: false` and the reader
//!    helper `freshness::is_stale` agrees on the served `/state` body.
//! C) negative control: `NERVE_STATE_MAX_AGE=5` (below the floor) refuses
//!    to start (exit 2).
//!
//! Gated: `GALION_REAL=1`. Ports 23180-23189 (`REAL_NERVE_PORT_BASE` moves the block).
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-nerve \
//!   --test real_state_freshness -- --nocapture --test-threads=1
//! ```

use openmind_nerve::freshness;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

const fn port_block(default: u16) -> u16 {
    match option_env!("REAL_NERVE_PORT_BASE") {
        None => default,
        Some(s) => {
            let b = s.as_bytes();
            let mut i = 0;
            let mut v: u32 = 0;
            assert!(!b.is_empty(), "REAL_NERVE_PORT_BASE empty");
            while i < b.len() {
                assert!(b[i].is_ascii_digit(), "REAL_NERVE_PORT_BASE not a number");
                v = v * 10 + (b[i] - b'0') as u32;
                assert!(v <= 65_526, "REAL_NERVE_PORT_BASE too large");
                i += 1;
            }
            v as u16
        }
    }
}

const BLOCK: u16 = port_block(23180);

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

fn http(port: u16, path: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_secs(2),
    ) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    );
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

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-state-fresh-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::write(d.join("components.json"), "[]").unwrap();
    d
}

fn cmd(dir: &Path, port: u16, event: bool, extra: &[(&str, &str)]) -> Command {
    let mut c = Command::new(BIN);
    c.arg("serve");
    if event {
        c.args(["--mode", "event"]);
    }
    c.args(["--listen", &format!("127.0.0.1:{port}")])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env(
            "HOME",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
        )
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_EDGE_HOSTS", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in extra {
        c.env(k, v);
    }
    c
}

fn start(c: &mut Command, port: u16) -> Kill {
    let n = Kill(c.spawn().expect("spawn nerve"));
    let t = Instant::now();
    loop {
        let (code, b) = http(port, "/health");
        if code == 200
            && serde_json::from_str::<Value>(&b).is_ok_and(|v| v["cycle"].as_u64() >= Some(1))
        {
            return n;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "nerve on {port} not up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn real_state_freshness() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    // A) quiet event mode is fresh
    let port = BLOCK;
    let dir = tmp("a");
    let _n = start(
        &mut cmd(
            &dir,
            port,
            true,
            &[
                ("NERVE_EVT_HEARTBEAT", "60"),
                ("NERVE_EVT_DEBOUNCE", "1"),
                ("NERVE_EVT_MAX_WAIT", "4"),
            ],
        ),
        port,
    );
    let s1 = get_json(port, "/state");
    assert_eq!(s1["freshness"]["max_age_s"], 124, "{}", s1["freshness"]);
    assert!(s1["freshness"]["basis"]
        .as_str()
        .unwrap()
        .starts_with("event"));
    std::thread::sleep(Duration::from_secs(8));
    let s2 = get_json(port, "/state");
    let h = get_json(port, "/health");
    println!(
        "== {} A event quiet: state ts {} -> {}, health ts {}, age {} max {} fresh {}",
        stamp(),
        s1["ts"],
        s2["ts"],
        h["ts"],
        h["state_age_s"],
        h["state_max_age_s"],
        h["state_fresh"]
    );
    assert_eq!(s1["ts"], s2["ts"], "quiet event-mode ts should hold still");
    assert_ne!(h["ts"], s2["ts"], "/health ts is request time");
    assert!(h["state_age_s"].as_u64().unwrap() >= 7, "{h}");
    assert_eq!(h["state_fresh"], true, "{h}");
    assert_eq!(h["state_ts"], s2["ts"]);
    assert_eq!(freshness::is_stale(&s2, now()), Some(false));
    drop(_n);

    // B) cycles stop (600 s interval) with a 10 s limit -> stale
    let port = BLOCK + 1;
    let dir = tmp("b");
    let _n = start(
        &mut cmd(
            &dir,
            port,
            false,
            &[("NERVE_INTERVAL", "600"), ("NERVE_STATE_MAX_AGE", "10")],
        ),
        port,
    );
    let h0 = get_json(port, "/health");
    assert_eq!(h0["state_fresh"], true, "{h0}");
    assert_eq!(h0["state_max_age_s"], 10);
    std::thread::sleep(Duration::from_secs(12));
    let st = get_json(port, "/state");
    let h = get_json(port, "/health");
    println!(
        "== {} B poll stalled: state ts {}, age {} max {} fresh {} basis {}",
        stamp(),
        st["ts"],
        h["state_age_s"],
        h["state_max_age_s"],
        h["state_fresh"],
        st["freshness"]["basis"]
    );
    assert_eq!(h["state_fresh"], false, "{h}");
    assert_eq!(st["freshness"]["basis"], "NERVE_STATE_MAX_AGE");
    assert_eq!(freshness::is_stale(&st, now()), Some(true));
    drop(_n);

    // C) bad limit refuses to start
    let dir = tmp("c");
    let mut c = cmd(&dir, BLOCK + 2, false, &[("NERVE_STATE_MAX_AGE", "5")]);
    let mut child = c.spawn().expect("spawn");
    let t = Instant::now();
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if t.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("bad NERVE_STATE_MAX_AGE did not refuse");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    println!("== {} C NERVE_STATE_MAX_AGE=5 exit {:?}", stamp(), code);
    assert_eq!(code, Some(2));
    for t in ["a", "b", "c"] {
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("nerve-state-fresh-{t}-{}", std::process::id())),
        );
    }
}
