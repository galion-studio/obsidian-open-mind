// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (gated: `GALION_REAL=1`), wave-4 slot 369, D-344-1 prep:
//! `NERVE_EVT_HEARTBEAT` is tunable within validated bounds (120..=900 s,
//! default 900 kept) and the cycles/h vs load trade-off is measured on the
//! box with throwaway event-mode nerves (`openmind-nerve serve --mode event`).
//!
//! - `real_heartbeat_bounds`: the real binary with 60 / 5000 / unset / 300
//!   reports the effective heartbeat (and the clamped request) in `/stats`
//!   config and on stderr; a non-number exits 2 at start.
//! - `real_heartbeat_cycles_per_hour`: three quiet nerves (journal, pidfd
//!   off; one healthy HTTP component) with heartbeat 120, 300 and 900 run
//!   side by side for `REAL_HB_WINDOW_S` (default 1860 s, >= 2 x 900 + slack);
//!   it counts heartbeat wakes and engine cycles, the gaps between cycles,
//!   CPU ticks (utime+stime from /proc) and RSS, prints the per-hour numbers
//!   and checks the cadence equals the heartbeat (gap within +-3 s, count
//!   within +-1 of window/heartbeat).
//!
//! Ports 22390-22399 (REAL_NERVE_PORT_BASE moves the block at build time).
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-nerve \
//!   --test real_heartbeat_tune -- --nocapture --test-threads=2
//! ```

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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

const BLOCK: u16 = port_block(22390);
const N120: u16 = BLOCK;
const N300: u16 = BLOCK + 1;
const N900: u16 = BLOCK + 2;
const OK: u16 = BLOCK + 3;
const NB: u16 = BLOCK + 4;
const OK_B: u16 = BLOCK + 5;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

struct Kill(Option<Child>, PathBuf);
impl Drop for Kill {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn http(port: u16, path: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}");
    let mut s = TcpStream::connect_timeout(&addr.parse().ok()?, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn json(port: u16, path: &str) -> Value {
    http(port, path)
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn healthy(port: u16) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("component port free");
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}");
        }
    });
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-hb369-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cmd(port: u16, comp: u16, dir: &Path, hb: Option<&str>) -> Command {
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"hb-ok","port":{comp},"path":"/health"}}]"#),
    )
    .unwrap();
    let mut c = Command::new(BIN);
    c.args(["serve", "--mode", "event"])
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_ROLLBACK", "0")
        .env("NERVE_EVT_JOURNAL", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .env("NERVE_EVT_ESCALATE", "off")
        .env("NERVE_LISTEN_SCOPE", format!("{BLOCK}-{}", BLOCK + 9))
        .env_remove("NERVE_EVT_HEARTBEAT")
        .env_remove("NERVE_EVT_HEARTBEAT_TEST_FLOOR")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    if let Some(h) = hb {
        c.env("NERVE_EVT_HEARTBEAT", h);
    }
    c
}

fn wait_up(port: u16) {
    let t = Instant::now();
    while http(port, "/health").map(|r| r.0) != Some(200) {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "nerve :{port} not up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// (utime+stime ticks, rss KiB) of a pid.
fn proc_cost(pid: u32) -> (u64, u64) {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let after = stat.rsplit_once(')').map(|x| x.1).unwrap_or("");
    let f: Vec<&str> = after.split_whitespace().collect();
    // after ')': state=0 ... utime=11 stime=12
    let ticks = f.get(11).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0)
        + f.get(12).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
    let rss = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        .unwrap_or(0);
    (ticks, rss)
}

fn now_local() -> String {
    // Box clock is Europe/Zagreb; `date` prints the offset.
    Command::new("date")
        .arg("+%F %T %z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[test]
fn real_heartbeat_bounds() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    healthy(OK_B);
    // (env value, effective, clamped_from)
    let cases: [(Option<&str>, u64, Value); 5] = [
        (None, 900, Value::Null),
        (Some("300"), 300, Value::Null),
        (Some("60"), 120, Value::from(60)),
        (Some("5000"), 900, Value::from(5000)),
        (Some("120"), 120, Value::Null),
    ];
    for (v, want, from) in cases {
        let dir = tmp("bounds");
        let mut c = cmd(NB, OK_B, &dir, v);
        let log = std::fs::File::create(dir.join("stderr.log")).unwrap();
        let child = c.stderr(log).spawn().expect("spawn nerve");
        let k = Kill(Some(child), dir.clone());
        wait_up(NB);
        let st = json(NB, "/stats");
        let cfg = &st["config"];
        assert_eq!(cfg["heartbeat_s"], want, "{v:?}: {cfg}");
        assert_eq!(cfg["heartbeat_clamped_from"], from, "{v:?}: {cfg}");
        assert_eq!(cfg["heartbeat_bounds_s"], serde_json::json!([120, 900]));
        assert_eq!(cfg["heartbeat_test_floor"], false);
        let err = std::fs::read_to_string(dir.join("stderr.log")).unwrap_or_default();
        assert!(
            err.contains(&format!("event mode: heartbeat={want}s")),
            "{v:?}: {err}"
        );
        let warned = err.contains("out of bounds 120..=900 s");
        assert_eq!(warned, !from.is_null(), "{v:?}: {err}");
        eprintln!(
            "369 bounds {} env={v:?} -> heartbeat_s {} clamped_from {} cycles/h {} warn={warned}",
            now_local(),
            cfg["heartbeat_s"],
            cfg["heartbeat_clamped_from"],
            cfg["heartbeat_cycles_per_hour"]
        );
        drop(k);
        std::thread::sleep(Duration::from_millis(300));
    }
    for bad in ["abc", "300s", "-1"] {
        let dir = tmp("bad");
        let out = cmd(NB, OK_B, &dir, Some(bad))
            .stderr(Stdio::piped())
            .output()
            .expect("run nerve");
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{bad}: {err}");
        assert!(
            err.contains("NERVE_EVT_HEARTBEAT: not a number"),
            "{bad}: {err}"
        );
        eprintln!("369 bounds env={bad:?} -> exit {:?}", out.status.code());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

struct Leg {
    hb: u64,
    port: u16,
    _k: Kill,
    pid: u32,
    cycles_at: Vec<Instant>,
    last_total: u64,
    hb_wakes0: u64,
    cost0: (u64, u64),
}

#[test]
fn real_heartbeat_cycles_per_hour() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let window: u64 = std::env::var("REAL_HB_WINDOW_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1860);
    assert!(window >= 300, "REAL_HB_WINDOW_S >= 300");
    healthy(OK);
    let t_start = now_local();
    let mut legs: Vec<Leg> = [(120u64, N120), (300, N300), (900, N900)]
        .into_iter()
        .map(|(hb, port)| {
            let dir = tmp(&hb.to_string());
            let child = cmd(port, OK, &dir, Some(&hb.to_string()))
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn nerve");
            let pid = child.id();
            Leg {
                hb,
                port,
                _k: Kill(Some(child), dir),
                pid,
                cycles_at: vec![],
                last_total: 0,
                hb_wakes0: 0,
                cost0: (0, 0),
            }
        })
        .collect();
    for l in &mut legs {
        wait_up(l.port);
    }
    // Let the start cycle (heartbeat at t=0) land, then open the window.
    std::thread::sleep(Duration::from_secs(5));
    for l in &mut legs {
        let st = json(l.port, "/stats");
        l.last_total = st["engine_cycles_total"].as_u64().unwrap_or(0);
        l.hb_wakes0 = st["wakes_by"]["heartbeat"].as_u64().unwrap_or(0);
        l.cost0 = proc_cost(l.pid);
        assert!(
            l.last_total >= 1,
            "start cycle missing on :{}: {st}",
            l.port
        );
    }
    let t0 = Instant::now();
    let tick = std::env::var("CLK_TCK_HZ")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(100.0);
    while t0.elapsed() < Duration::from_secs(window) {
        std::thread::sleep(Duration::from_secs(1));
        for l in &mut legs {
            let st = json(l.port, "/stats");
            if let Some(n) = st["engine_cycles_total"].as_u64() {
                if n > l.last_total {
                    l.cycles_at.push(Instant::now());
                    eprintln!(
                        "  hb={} :{} cycle {} at +{} s",
                        l.hb,
                        l.port,
                        n,
                        t0.elapsed().as_secs()
                    );
                    l.last_total = n;
                }
            }
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    let mut report = vec![];
    for l in &legs {
        let st = json(l.port, "/stats");
        let state = json(l.port, "/api/v1/state");
        let hb_wakes = st["wakes_by"]["heartbeat"].as_u64().unwrap_or(0) - l.hb_wakes0;
        let wakes_total = st["wakes_total"].as_u64().unwrap_or(0);
        let cycles = l.cycles_at.len() as u64;
        let cost1 = proc_cost(l.pid);
        let cpu_s = (cost1.0 - l.cost0.0) as f64 / tick;
        let per_h = |n: f64| n * 3600.0 / secs;
        let gaps: Vec<u64> = l
            .cycles_at
            .windows(2)
            .map(|w| w[1].duration_since(w[0]).as_secs())
            .collect();
        let expect = (secs / l.hb as f64).floor() as i64;
        let line = format!(
            "hb={}s window={:.0}s heartbeat_wakes={} engine_cycles={} -> {:.2} cycles/h (theory {:.2}); gaps_s={:?}; cpu {:.2} s in window = {:.3} cpu-s/h ({:.3} cpu-s/cycle); rss {} KiB; wakes_total {}; cycle_ms {}",
            l.hb, secs, hb_wakes, cycles, per_h(cycles as f64),
            openmind_nerve::evt::heartbeat_cycles_per_hour(l.hb), gaps, cpu_s, per_h(cpu_s),
            if cycles > 0 { cpu_s / cycles as f64 } else { 0.0 }, cost1.1, wakes_total, state["cycle_ms"]
        );
        eprintln!("369 {line}");
        report.push(line);
        assert!(
            (cycles as i64 - expect).abs() <= 1,
            "hb={}: {cycles} cycles in {secs:.0}s, expected ~{expect}",
            l.hb
        );
        assert_eq!(hb_wakes, cycles, "quiet nerve: every cycle is a heartbeat");
        for g in &gaps {
            assert!(
                g.abs_diff(l.hb) <= 3,
                "hb={}: cycle gap {g}s not ~{}s",
                l.hb,
                l.hb
            );
        }
    }
    eprintln!("369 window {t_start} .. {}", now_local());
    drop(legs);
    let _ = report;
}
