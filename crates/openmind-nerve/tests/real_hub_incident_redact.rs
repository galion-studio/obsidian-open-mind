// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for slot 435 (hub-incident-redact, D-422-2 + staged
//! D-422-1), gated by `GALION_REAL=1`; run with `--release`.
//! Ports 23050-23059 (`HUB_REDACT_REAL_PORT_BASE` moves them): run A on +0,
//! run B on +1, listen scope the whole block.
//!
//! One real `openmind-nerve serve` (event-feed on its own `--log`) per run,
//! one `POST /event` at `error` whose data names the machine in every form
//! (IPv4, IPv4:port, IPv6 with zone, `[v6]:port`, host:port, dotted host):
//!
//! - A, fold off (default): no raw address in `/api/v1/state` or the other
//!   loopback JSON views; the hub incident is still its own cause next to
//!   `gaia:<src>` (two 15-point incidents: the D-422-1 double count, kept
//!   as the default until the owner decides).
//! - B, `NERVE_HUB_EVENT_FOLD=1`: same redaction, and exactly one incident
//!   deduction for the source (`gaia:<src>`), scored once; score =
//!   100 - sum(deductions).
//!
//! `HUB_REDACT_NERVE_BIN` runs another binary (negative control: the base
//! 674482f1 must fail the redaction assert).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SRC: &str = "t435";
/// Every raw form the pushed data carries; none may reach a JSON view.
const RAW: &[&str] = &[
    "10.0.0.5",
    "198.51.100.4",
    "2001:db8::7",
    "fe80::1",
    "edge.example.org",
    "nerve.lan",
    ":8443",
    ":8670",
];
const VIEWS: &[&str] = &[
    "/api/v1/state",
    "/api/v1/incidents",
    "/api/v1/signals",
    "/api/v1/cortex",
    "/api/v1/cortex/events",
    "/api/v1/self-eval.json",
    "/api/v1/alerts",
    "/",
];

fn base() -> u16 {
    galion_realtest::ports::base_from_env("HUB_REDACT_REAL_PORT_BASE", 23050, 10)
}

fn bin() -> String {
    std::env::var("HUB_REDACT_NERVE_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_openmind-nerve").to_string())
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(port: u16, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(
        s,
        "{method} {path} HTTP/1.0\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (head, body) = b.split_once("\r\n\r\n")?;
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, body.to_string()))
}

fn state(port: u16) -> Value {
    http(port, "GET", "/api/v1/state", "")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn wait(port: u16, what: &str, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = state(port);
        if ok(&st) {
            return st;
        }
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout waiting for {what}: {}",
            st["self_eval"]
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn deductions(st: &Value) -> Vec<(u64, String)> {
    st["self_eval"]["deductions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            (
                d["points"].as_u64().unwrap_or(0),
                d["what"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn incidents_for_src(st: &Value) -> Vec<(u64, String)> {
    deductions(st)
        .into_iter()
        .filter(|(_, w)| w.starts_with("error incident: ") && w.contains(SRC))
        .collect()
}

/// One run; returns (incident deductions for SRC, leaks found).
fn run(port: u16, fold: bool) -> (Vec<(u64, String)>, Vec<String>) {
    let dir = std::env::temp_dir().join(format!(
        "nerve-real-hub-redact-{}-{port}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("events.jsonl");
    let comps = dir.join("components.json");
    std::fs::write(&comps, "[]").unwrap();
    let b = base();
    let nerve = Kill(
        Command::new(bin())
            .arg("serve")
            .arg("--log")
            .arg(&log)
            .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
            .env("NERVE_INTERVAL", "3")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", format!("{b}-{}", b + 9))
            .env("NERVE_EVENT_FEED_FILE", &log)
            .env("NERVE_HUB_EVENT_FOLD", if fold { "1" } else { "0" })
            .env_remove("NERVE_SENSORS_OFF")
            .env_remove("NERVE_EDGE_HOSTS")
            .env_remove("NERVE_EVENT_FEED_SOURCES")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let st = wait(port, "first cycle", 40, |s| {
        s["cycle"].as_u64().unwrap_or(0) >= 1
    });
    assert!(incidents_for_src(&st).is_empty());
    let data = json!({"id": "edge-flood", "component": "tunnel",
        "title": "edge flood on 10.0.0.5:443 from [2001:db8::7]:8443 via edge.example.org",
        "fix": "ban 10.0.0.5 and 198.51.100.4; check fe80::1%eth0 and nerve.lan:8670"});
    let body =
        json!({"source": SRC, "kind": "ddos", "severity": "error", "data": data}).to_string();
    let (code, rb) = http(port, "POST", "/event", &body).expect("POST /event");
    assert_eq!(code, 200, "{rb}");
    let t = Instant::now();
    let st = wait(port, "incident for the pushed error", 40, |s| {
        s["self_eval"]["covered"]
            .as_array()
            .is_some_and(|a| a.iter().any(|c| c["by"] == format!("gaia:{SRC}")))
            && !incidents_for_src(s).is_empty()
    });
    // One more cycle so every view has seen the event.
    let c0 = st["cycle"].as_u64().unwrap_or(0);
    let st = wait(port, "next cycle", 20, |s| {
        s["cycle"].as_u64().unwrap_or(0) > c0
    });
    let inc = incidents_for_src(&st);
    let mut leaks = vec![];
    for v in VIEWS {
        let (code, text) = http(port, "GET", v, "").unwrap_or((0, String::new()));
        for r in RAW {
            if text.contains(r) {
                leaks.push(format!("{v} ({code}) has {r}"));
            }
        }
        eprintln!(
            "   {v}: {code} {} B, redacted markers <addr> {} <host> {}",
            text.len(),
            text.matches("<addr>").count(),
            text.matches("<host>").count()
        );
    }
    // The raw log is the evidence store and keeps the data as pushed.
    let (_, ev) = http(port, "GET", "/api/v1/events?n=5", "").unwrap_or_default();
    eprintln!(
        "   /api/v1/events (raw log, by design): has 10.0.0.5 {}",
        ev.contains("10.0.0.5")
    );
    let sum: u64 = deductions(&st).iter().map(|d| d.0).sum();
    assert_eq!(
        st["self_eval"]["score"].as_u64().unwrap_or(0),
        100u64.saturating_sub(sum),
        "{:?}",
        deductions(&st)
    );
    eprintln!(
        "{} fold={fold} :{port} cycle {} after {:.1} s: incidents {inc:?}; score {} {}; leaks {}",
        if fold { "B." } else { "A." },
        st["cycle"],
        t.elapsed().as_secs_f64(),
        st["self_eval"]["score"],
        st["self_eval"]["grade"],
        leaks.len()
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
    (inc, leaks)
}

#[test]
fn real_hub_incident_redact_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let b = base();
    let (inc_a, leaks_a) = run(b, false);
    assert!(leaks_a.is_empty(), "fold off leaks: {leaks_a:?}");
    let gaia = format!("error incident: gaia:{SRC}: ");
    assert_eq!(
        inc_a.iter().filter(|(_, w)| w.starts_with(&gaia)).count(),
        1,
        "{inc_a:?}"
    );
    assert_eq!(
        inc_a.len(),
        2,
        "fold off keeps the D-422-1 double count: {inc_a:?}"
    );

    let (inc_b, leaks_b) = run(b + 1, true);
    assert!(leaks_b.is_empty(), "fold on leaks: {leaks_b:?}");
    assert_eq!(
        inc_b.len(),
        1,
        "fold on: exactly one scored cause: {inc_b:?}"
    );
    assert!(inc_b[0].1.starts_with(&gaia), "{inc_b:?}");
    assert_eq!(
        inc_b[0].0, 15,
        "one cause, one component, scored once: {inc_b:?}"
    );
    eprintln!(
        "PASS: A {} pts over {} incidents -> B {} pts over 1",
        inc_a.iter().map(|d| d.0).sum::<u64>(),
        inc_a.len(),
        inc_b[0].0
    );
}
