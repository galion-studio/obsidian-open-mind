// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (gated: `GALION_REAL=1`), wave-4 slot 344, N-332-1: the
//! live nerve's `cycle` counter moves once per ~8-15 min although
//! `interval_s` is 30. Read-only VPS check (docs/real-tests/nerve-cycle-latency.md):
//! the live unit runs `--mode event`, where the full cycle runs at start, on
//! the 900 s heartbeat and debounced after events (by design), and the live
//! `cycle_ms` is ~1 s. The real latency hazard found while reproducing: HTTP
//! probes ran one after another and the loopback client's timeout was per
//! read call, so k hung components cost k x 3 s and one slow-drip component
//! stalled the cycle without bound.
//!
//! This test starts the real `openmind-nerve serve` (polling mode, interval
//! 5 s) against 9 components: 6 that accept and never answer, 1 that drips a
//! header byte every 200 ms forever, 1 healthy (with a deep route) and 1 dead
//! port, then checks:
//!
//! - leg A (default `NERVE_PROBE_PARALLEL`=8): every observed cycle takes
//!   <= one HTTP budget + slack (4.5 s) and therefore <= interval + timeout
//!   (8 s); cycles keep coming every interval; the healthy component stays
//!   healthy, hung/drip/dead ones are failing with a probe error, never a
//!   false 200 from the drip;
//! - leg B (control, `NERVE_PROBE_PARALLEL=1` = the old sequential walk with
//!   the new total deadline): one cycle costs >= 6 x 3 s, and stays bounded
//!   (< 26 s) even with the drip component.
//!
//! Ports 22140-22149 (REAL_NERVE_PORT_BASE moves the block at build time).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

const BLOCK: u16 = port_block(22140);
const NERVE_A: u16 = BLOCK;
const HANG: u16 = BLOCK + 1;
const DRIP: u16 = BLOCK + 2;
const OK: u16 = BLOCK + 3;
const DEAD: u16 = BLOCK + 4;
const NERVE_B: u16 = BLOCK + 5;
const INTERVAL_S: u64 = 5;
const HTTP_BUDGET_MS: u64 = 3000;

struct Kill(Option<Child>);
impl Drop for Kill {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}");
    let mut s = TcpStream::connect_timeout(&addr.parse().ok()?, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn state(port: u16) -> serde_json::Value {
    get(port, "/api/v1/state")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn comp(st: &serde_json::Value, n: &str) -> serde_json::Value {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == n).cloned())
        .unwrap_or_default()
}

/// Accept-and-hold (never answers) / slow drip / healthy servers.
fn server(port: u16, kind: &'static str, stop: Arc<AtomicBool>) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("port free");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut held = vec![];
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((mut c, _)) => {
                    let _ = c.set_nonblocking(false);
                    let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = [0u8; 1024];
                    let _ = c.read(&mut buf);
                    match kind {
                        "hang" => held.push(c),
                        "drip" => {
                            let stop = stop.clone();
                            std::thread::spawn(move || {
                                let _ = c.write_all(b"HTTP/1.0 200 OK\r\nX-Drip: ");
                                while !stop.load(Ordering::Relaxed) {
                                    if c.write_all(b"a").is_err() {
                                        break;
                                    }
                                    std::thread::sleep(Duration::from_millis(200));
                                }
                            });
                        }
                        _ => {
                            let _ = c.write_all(
                                b"HTTP/1.0 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}",
                            );
                        }
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    });
}

fn nerve(port: u16, dir: &std::path::Path, parallel: Option<&str>) -> Kill {
    let mut c = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"));
    c.arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", INTERVAL_S.to_string())
        .env("NERVE_DATA_DIR", dir.join(format!("data-{port}")))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_ROLLBACK", "0")
        .env("NERVE_EVT_ESCALATE", "off")
        .env("NERVE_LISTEN_SCOPE", format!("{BLOCK}-{}", BLOCK + 9))
        .stderr(Stdio::null());
    c.env_remove("NERVE_PROBE_PARALLEL");
    if let Some(p) = parallel {
        c.env("NERVE_PROBE_PARALLEL", p);
    }
    Kill(Some(c.spawn().expect("spawn nerve")))
}

/// Poll `/api/v1/state` until `cycles` cycles completed; returns the
/// per-cycle `cycle_ms` seen, the wall time when each new cycle appeared, and
/// the last state.
fn watch(
    port: u16,
    cycles: u64,
    max: Duration,
) -> (Vec<(u64, u64)>, Vec<Instant>, serde_json::Value) {
    let t0 = Instant::now();
    let mut seen: Vec<(u64, u64)> = vec![];
    let mut at = vec![];
    loop {
        let st = state(port);
        if let (Some(c), Some(ms)) = (st["cycle"].as_u64(), st["cycle_ms"].as_u64()) {
            if seen.last().is_none_or(|(lc, _)| *lc != c) {
                eprintln!(
                    "  :{port} cycle {c} cycle_ms {ms} at +{} ms",
                    t0.elapsed().as_millis()
                );
                seen.push((c, ms));
                at.push(Instant::now());
            }
            if c >= cycles {
                return (seen, at, st);
            }
        }
        assert!(
            t0.elapsed() < max,
            "nerve :{port} reached only {seen:?} in {max:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn real_cycle_latency() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nerve-cycle-latency-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    server(HANG, "hang", stop.clone());
    server(DRIP, "drip", stop.clone());
    server(OK, "ok", stop.clone());
    let mut comps = vec![];
    for i in 1..=6 {
        comps.push(format!(
            r#"{{"name":"hang-{i}","port":{HANG},"path":"/health","gaia":"HEPHAESTUS"}}"#
        ));
    }
    comps.push(format!(
        r#"{{"name":"drip","port":{DRIP},"path":"/health","gaia":"HEPHAESTUS"}}"#
    ));
    comps.push(format!(
        r#"{{"name":"ok","port":{OK},"path":"/health","gaia":"HEPHAESTUS","deep":["/api/v1/x"]}}"#
    ));
    comps.push(format!(
        r#"{{"name":"dead","port":{DEAD},"path":"/health","gaia":"HEPHAESTUS"}}"#
    ));
    std::fs::write(
        dir.join("components.json"),
        format!("[{}]", comps.join(",")),
    )
    .unwrap();

    // Leg A: default bounded parallel probes.
    eprintln!("leg A: NERVE_PROBE_PARALLEL unset (default 8)");
    let a = nerve(NERVE_A, &dir, None);
    let (seen, at, st) = watch(NERVE_A, 4, Duration::from_secs(60));
    drop(a);
    assert_eq!(st["probe_parallel"], 8, "{}", st["probe_parallel"]);
    assert_eq!(st["cadence"], "poll: full cycle every interval_s");
    let limit = INTERVAL_S * 1000 + HTTP_BUDGET_MS;
    for (c, ms) in &seen {
        assert!(
            *ms <= 4500,
            "cycle {c} took {ms} ms (> one 3 s budget + slack)"
        );
        assert!(
            *ms <= limit,
            "cycle {c} took {ms} ms > interval+timeout {limit}"
        );
    }
    // Consecutive cycles arrive about one interval apart (sleep + cycle).
    for w in at.windows(2).skip(1) {
        let gap = w[1].duration_since(w[0]);
        assert!(
            gap <= Duration::from_millis(limit + 1500),
            "cycle gap {gap:?}"
        );
    }
    for i in 1..=6 {
        let h = comp(&st, &format!("hang-{i}"));
        assert!(h["http"].is_null(), "{h}");
        assert_eq!(h["status"], "failing", "{h}");
    }
    let d = comp(&st, "drip");
    assert!(d["http"].is_null(), "drip must not read as HTTP 200: {d}");
    assert_eq!(d["status"], "failing", "{d}");
    let o = comp(&st, "ok");
    assert_eq!(
        (o["http"].as_u64(), o["status"].as_str()),
        (Some(200), Some("healthy")),
        "{o}"
    );
    assert_eq!(o["extra"]["deep"][0]["ok"], true, "{o}");
    let x = comp(&st, "dead");
    assert_eq!(x["status"], "failing", "{x}");
    eprintln!(
        "leg A verdicts: hang-1 {} | drip {} reasons {} | ok {} | dead {}",
        comp(&st, "hang-1")["status"],
        d["status"],
        d["reasons"],
        o["status"],
        x["status"]
    );

    // Leg B (control): sequential walk, still bounded by the total deadline.
    eprintln!("leg B: NERVE_PROBE_PARALLEL=1 (sequential control)");
    let b = nerve(NERVE_B, &dir, Some("1"));
    let (seen_b, _, st_b) = watch(NERVE_B, 1, Duration::from_secs(60));
    drop(b);
    assert_eq!(st_b["probe_parallel"], 1);
    let ms = seen_b.last().unwrap().1;
    assert!(ms >= 6 * HTTP_BUDGET_MS, "sequential control only {ms} ms");
    assert!(ms < 26_000, "sequential control unbounded? {ms} ms");
    eprintln!(
        "leg B: first cycle {ms} ms (sequential), leg A max {} ms",
        seen.iter().map(|x| x.1).max().unwrap_or(0)
    );

    stop.store(true, Ordering::Relaxed);
    let _ = std::fs::remove_dir_all(&dir);
}
