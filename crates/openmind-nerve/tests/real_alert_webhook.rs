// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-        |st| hook(st)["sent"].as_u64().unwrap_or(0) >= 1 && st["cycle"].as_u64().unwrap_or(0) >= 2,udio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 382 (nerve-alerts-stage), real-life test gated with `GALION_REAL=1`.
//! Real `openmind-nerve serve` (release with `--release`) on ports
//! 22520-22529 (REAL_NERVE_PORT_BASE remaps), two components on closed ports
//! so the nerve raises real status + incident alerts, and a capturing HTTP
//! receiver on loopback standing in for the owner's alert relay.
//!
//! 1. `json` sink, per_hour 2: the receiver gets exactly 2 POSTs (valid JSON,
//!    `source galion-nerve`, Zagreb offset), `/state` counts sent 2 and
//!    rate_limited >= 1, `/alerts` still lists every alert.
//! 2. `ntfy` sink: text/plain body with ntfy `Title` / `Priority` / `Tags`.
//! 3. receiver down: sends fail, counted, nerve keeps cycling.
//! 4. rejected target (`http://ntfy.sh/...`, non-loopback): nerve still
//!    serves, `/state` shows the rejection, self-eval `alerts-channel` = warn,
//!    nothing is sent anywhere.
//! 5. unset (the live VPS config): `external` = "none configured ...", the
//!    exact string the live nerve reports (read-only probe 2026-10-28 10:37 +01:00).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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

const BLOCK: u16 = port_block(22520);
const NERVE_PORT: u16 = BLOCK;
const RECV: u16 = BLOCK + 1;
const DEAD_A: u16 = BLOCK + 2;
const DEAD_B: u16 = BLOCK + 3;
/// Never bound: "receiver down" target.
const RECV_DOWN: u16 = BLOCK + 4;

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skipped: set GALION_REAL=1");
    }
    on
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-382-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug, Clone)]
struct Req {
    head: String,
    body: String,
}

/// Capturing receiver: records every request, answers 200.
fn receiver(stop: Arc<AtomicBool>) -> (std::thread::JoinHandle<()>, Arc<Mutex<Vec<Req>>>) {
    let l = TcpListener::bind(("127.0.0.1", RECV)).expect("receiver port free");
    l.set_nonblocking(true).unwrap();
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    let h = std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((s, _)) => {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut r = BufReader::new(s);
                    let mut head = String::new();
                    let mut len = 0usize;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                        head.push_str(&line);
                    }
                    let mut body = vec![0u8; len];
                    let _ = r.read_exact(&mut body);
                    let _ = r.get_mut().write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    );
                    g.lock().unwrap().push(Req {
                        head,
                        body: String::from_utf8_lossy(&body).into(),
                    });
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    });
    (h, got)
}

fn get(p: &str) -> Option<serde_json::Value> {
    let mut s = TcpStream::connect_timeout(
        &format!("127.0.0.1:{NERVE_PORT}").parse().ok()?,
        Duration::from_secs(2),
    )
    .ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {p} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    if h.split_whitespace().nth(1)? != "200" {
        return None;
    }
    serde_json::from_str(body).ok()
}

struct Run {
    state: serde_json::Value,
    alerts: serde_json::Value,
    self_eval: serde_json::Value,
    secs: f64,
}

fn serve(tag: &str, env: &[(&str, String)], done: impl Fn(&serde_json::Value) -> bool) -> Run {
    let dir = tmp(tag);
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        serde_json::json!([
            {"name": "dead-a", "port": DEAD_A, "path": "/health", "critical": true, "restarts_per_hour": 0},
            {"name": "dead-b", "port": DEAD_B, "path": "/health", "restarts_per_hour": 0}
        ])
        .to_string(),
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"));
    cmd.arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{NERVE_PORT}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_FAIL_CYCLES", "1")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_ROLLBACK", "0")
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_EVT_ESCALATE", "off")
        .env("NERVE_LISTEN_SCOPE", format!("{BLOCK}-{}", BLOCK + 9))
        .env_remove("NERVE_TELEGRAM_ENV")
        .env_remove("NERVE_ALERT_WEBHOOK")
        .env_remove("NERVE_ALERT_WEBHOOK_FORMAT")
        .env_remove("NERVE_ALERT_WEBHOOK_MIN")
        .env_remove("NERVE_ALERT_WEBHOOK_PER_HOUR");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let _n = Kill(
        cmd.stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    let mut last = serde_json::Value::Null;
    while t.elapsed() < Duration::from_secs(45) {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(st) = get("/api/v1/state") {
            last = st;
            if done(&last) {
                break;
            }
        }
    }
    let alerts = get("/api/v1/alerts").unwrap_or_default();
    let self_eval = get("/api/v1/self-eval").unwrap_or_default();
    let secs = t.elapsed().as_secs_f64();
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!(
        "382 {tag}: {secs:.1}s cycle {} external {:?} webhook {}",
        last["cycle"], last["alerts"]["external"], last["alerts"]["webhook"]
    );
    Run {
        state: last,
        alerts,
        self_eval,
        secs,
    }
}

fn finding<'a>(se: &'a serde_json::Value, check: &str) -> &'a serde_json::Value {
    se["findings"]
        .as_array()
        .and_then(|a| a.iter().find(|f| f["check"] == check))
        .unwrap_or(&serde_json::Value::Null)
}

fn hook(st: &serde_json::Value) -> &serde_json::Value {
    &st["alerts"]["webhook"]
}

#[test]
fn real_alert_webhook_sink() {
    if !real() {
        return;
    }
    for p in [DEAD_A, DEAD_B, RECV_DOWN] {
        assert!(
            TcpStream::connect_timeout(
                &format!("127.0.0.1:{p}").parse().unwrap(),
                Duration::from_millis(200)
            )
            .is_err(),
            "port {p} must be closed"
        );
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (h, got) = receiver(stop.clone());
    let url = format!("http://127.0.0.1:{RECV}/galion-nerve");

    // 1. json, capped at 2/h.
    let r = serve(
        "json",
        &[
            ("NERVE_ALERT_WEBHOOK", url.clone()),
            ("NERVE_ALERT_WEBHOOK_PER_HOUR", "2".into()),
        ],
        |st| hook(st)["rate_limited"].as_u64().unwrap_or(0) >= 1 && hook(st)["sent"] == 2,
    );
    let w = hook(&r.state);
    assert_eq!(w["sent"], 2, "{w}");
    assert!(w["rate_limited"].as_u64().unwrap() >= 1, "{w}");
    assert_eq!(w["failed"], 0, "{w}");
    assert_eq!(w["format"], "json");
    let reqs = got.lock().unwrap().clone();
    assert_eq!(reqs.len(), 2, "{reqs:?}");
    let all_alerts = r.alerts.as_array().map(|a| a.len()).unwrap_or(0);
    assert!(all_alerts >= 3, "alerts.json keeps all: {}", r.alerts);
    for q in &reqs {
        assert!(
            q.head.starts_with("POST /galion-nerve HTTP/1.1\r\n"),
            "{}",
            q.head
        );
        assert!(
            q.head.contains("Content-Type: application/json"),
            "{}",
            q.head
        );
        let v: serde_json::Value = serde_json::from_str(&q.body).expect("json body");
        assert_eq!(v["source"], "galion-nerve");
        assert!(
            ["warn", "crit"].contains(&v["severity"].as_str().unwrap()),
            "{v}"
        );
        let ts = v["ts"].as_str().unwrap();
        assert!(ts.ends_with("+01:00") || ts.ends_with("+02:00"), "{ts}");
        assert!(v["component"].as_str().unwrap().starts_with("dead-"), "{v}");
    }
    eprintln!(
        "382 json: received {} (alerts.json {all_alerts}), sent {} rate_limited {} in {:.1}s; first {}",
        reqs.len(),
        w["sent"],
        w["rate_limited"],
        r.secs,
        reqs[0].body
    );
    got.lock().unwrap().clear();

    // 2. ntfy format, crit only.
    let r = serve(
        "ntfy",
        &[
            ("NERVE_ALERT_WEBHOOK", url.clone()),
            ("NERVE_ALERT_WEBHOOK_FORMAT", "ntfy".into()),
            ("NERVE_ALERT_WEBHOOK_MIN", "crit".into()),
        ],
        |st| hook(st)["sent"].as_u64().unwrap_or(0) >= 1 && st["cycle"].as_u64().unwrap_or(0) >= 2,
    );
    let w = hook(&r.state);
    assert!(w["sent"].as_u64().unwrap() >= 1, "{w}");
    // Floor is exact: every crit alert sent, every warn/info alert counted below_min.
    let sev_n = |s: &str| {
        r.alerts
            .as_array()
            .map(|a| a.iter().filter(|x| x["severity"] == s).count() as u64)
            .unwrap_or(0)
    };
    assert_eq!(
        w["sent"].as_u64().unwrap(),
        sev_n("crit"),
        "{w} vs {}",
        r.alerts
    );
    assert_eq!(
        w["below_min"].as_u64().unwrap(),
        sev_n("warn") + sev_n("info"),
        "{w} vs {}",
        r.alerts
    );
    let reqs = got.lock().unwrap().clone();
    assert_eq!(reqs.len() as u64, w["sent"].as_u64().unwrap());
    for q in &reqs {
        assert!(q.head.contains("Content-Type: text/plain"), "{}", q.head);
        assert!(q.head.contains("Title: galion-nerve crit "), "{}", q.head);
        assert!(q.head.contains("Priority: urgent"), "{}", q.head);
        assert!(q.head.contains("Tags: galion-nerve,crit"), "{}", q.head);
        assert!(!q.body.is_empty() && !q.body.starts_with('{'), "{}", q.body);
    }
    eprintln!(
        "382 ntfy: received {} crit, below_min {}; head {:?} body {:?}",
        reqs.len(),
        w["below_min"],
        reqs[0]
            .head
            .lines()
            .filter(|l| l.starts_with("Title") || l.starts_with("Priority"))
            .collect::<Vec<_>>(),
        reqs[0].body
    );
    got.lock().unwrap().clear();

    // 3. receiver down.
    let r = serve(
        "down",
        &[(
            "NERVE_ALERT_WEBHOOK",
            format!("http://127.0.0.1:{RECV_DOWN}/x"),
        )],
        |st| {
            hook(st)["failed"].as_u64().unwrap_or(0) >= 2 && st["cycle"].as_u64().unwrap_or(0) >= 2
        },
    );
    let w = hook(&r.state);
    assert!(w["failed"].as_u64().unwrap() >= 2, "{w}");
    assert_eq!(w["sent"], 0);
    assert_eq!(w["last"]["result"], "failed");
    assert!(
        r.state["cycle"].as_u64().unwrap() >= 2,
        "nerve keeps cycling"
    );
    eprintln!(
        "382 down: failed {} last {} cycle {}",
        w["failed"], w["last"], r.state["cycle"]
    );

    // 4. rejected non-loopback target.
    let r = serve(
        "rejected",
        &[("NERVE_ALERT_WEBHOOK", "http://ntfy.sh/galion".into())],
        |st| {
            st["cycle"].as_u64().unwrap_or(0) >= 1
                && st["alerts"]["recent"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
        },
    );
    let w = hook(&r.state);
    assert_eq!(w["configured"], false, "{w}");
    assert!(
        w["rejected"]
            .as_str()
            .unwrap()
            .contains("not an IP literal"),
        "{w}"
    );
    let f = finding(&r.self_eval, "alerts-channel");
    assert_eq!(f["level"], "warn", "{f}");
    assert!(
        f["msg"].as_str().unwrap().contains("webhook REJECTED"),
        "{f}"
    );
    eprintln!("382 rejected: {} / finding {}", w, f["msg"]);

    // 5. unset = live VPS parity.
    let r = serve("unset", &[], |st| st["cycle"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(hook(&r.state)["configured"], false);
    assert_eq!(
        r.state["alerts"]["external"],
        "none configured (no Telegram/Hermes alert channel found on the host)"
    );
    assert_eq!(finding(&r.self_eval, "alerts-channel")["level"], "info");

    stop.store(true, Ordering::Relaxed);
    h.join().unwrap();
    assert!(got.lock().unwrap().is_empty(), "nothing sent in runs 3-5");
}
