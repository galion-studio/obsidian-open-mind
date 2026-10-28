// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the T2.6 `ingress-lint` sensor (gated: `GALION_REAL=1`,
//! run with `--release`). Slot 159: ports 20290-20299. Loopback only.
//!
//! Nerve A (`openmind-nerve serve` on 127.0.0.1:20290) lints a config copy in
//! a temp dir against the box's real listening sockets:
//! 1. :20291 is served, :20292 and :20293 are not -> `ingress-closed-20292`,
//!    `ingress-closed-20293`, nothing for :20291;
//! 2. a listener starts on :20292 and the copy is rewritten (rule to :20293
//!    removed, rule to :20294 added) -> the sensor follows both without a
//!    restart;
//! 3. B33 events carry no hostname; cycle < 50 ms, RSS < 8 MiB.
//!
//! Nerve B (:20295) is pointed at a symlink into a `cloudflared` directory
//! holding credentials-like text -> `ingress-config-refused`, and the text
//! never shows up in its API.

use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const A: u16 = 20290;
const B: u16 = 20295;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(port: u16, path: &str) -> Option<(String, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.to_string(), body.to_string()))
}

fn state(port: u16) -> Value {
    http(port, "/api/v1/state")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn findings(st: &Value) -> Vec<Value> {
    let c = &st["cortex"];
    [
        &c["vulnerabilities"],
        &c["bleeding"],
        &c["ddos"]["findings"],
    ]
    .iter()
    .filter_map(|a| a.as_array())
    .flatten()
    .filter(|f| f["source"] == "ingress-lint")
    .cloned()
    .collect()
}

fn ids(st: &Value) -> Vec<String> {
    let mut v: Vec<String> = findings(st)
        .iter()
        .filter_map(|f| f["id"].as_str().map(str::to_string))
        .collect();
    v.sort();
    v
}

fn serve(port: u16) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("port free");
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n");
        }
    });
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn nerve(port: u16, dir: &Path, config: &Path) -> Kill {
    let comps = dir.join(format!("components-{port}.json"));
    std::fs::write(&comps, "[]").unwrap();
    Kill(
        Command::new(BIN)
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join(format!("data-{port}")))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "20290-20299")
            .env_remove("NERVE_SENSORS_OFF")
            .env("NERVE_INGRESS_LINT_CONFIG", config)
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    )
}

/// Poll `port` until `ok(state)` or `secs` pass; returns the last state.
fn wait(port: u16, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = state(port);
        if ok(&st) || t.elapsed() > Duration::from_secs(secs) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

const CFG1: &str = "tunnel: 00000000-0000-0000-0000-000000000000
credentials-file: /nonexistent/never-read.json
ingress:
- hostname: up.t26.invalid
  service: http://127.0.0.1:20291
- hostname: late.t26.invalid
  service: http://localhost:20292
- hostname: gone.t26.invalid
  path: ^/api
  service: http://127.0.0.1:20293
- service: http_status:404
";

const CFG2: &str = "tunnel: 00000000-0000-0000-0000-000000000000
credentials-file: /nonexistent/never-read.json
ingress:
- hostname: up.t26.invalid
  service: http://127.0.0.1:20291
- hostname: late.t26.invalid
  service: http://localhost:20292
- hostname: new.t26.invalid
  service: http://127.0.0.1:20294
- hostname: extra.t26.invalid
  service: http://127.0.0.1:20294
- service: http_status:404
";

#[test]
fn real_ingress_lint_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    for p in [20292u16, 20293, 20294] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            ":{p} must be free"
        );
    }
    let dir = std::env::temp_dir().join(format!("nerve-real-t26-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("cloudflared")).unwrap();
    serve(20291);
    let copy = dir.join("ingress-copy.yml");
    std::fs::write(&copy, CFG1).unwrap();

    // Phase 1.
    let a = nerve(A, &dir, &copy);
    let t0 = Instant::now();
    let st = wait(A, 40, |st| {
        ids(st) == ["ingress-closed-20292", "ingress-closed-20293"]
            && st["cycle"].as_u64().unwrap_or(0) >= 2
    });
    let p1 = t0.elapsed();
    assert_eq!(
        ids(&st),
        ["ingress-closed-20292", "ingress-closed-20293"],
        "{}",
        st["cortex"]
    );
    for f in findings(&st) {
        assert_eq!(f["severity"], "warn", "{f}");
        assert_eq!(f["kind"], "bleeding");
        assert!(!f["title"].as_str().unwrap().contains("t26.invalid"));
    }
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(names.last(), Some(&"ingress-lint"), "{names:?}");
    eprintln!(
        "phase 1 ({:.1} s, cycle {}): {:?}",
        p1.as_secs_f64(),
        st["cycle"],
        ids(&st)
    );

    // Phase 2: listener appears on :20292, copy rewritten (different size).
    serve(20292);
    std::fs::write(&copy, CFG2).unwrap();
    let t1 = Instant::now();
    let st = wait(A, 30, |st| ids(st) == ["ingress-closed-20294"]);
    let p2 = t1.elapsed();
    assert_eq!(ids(&st), ["ingress-closed-20294"], "{}", st["cortex"]);
    let f = &findings(&st)[0];
    assert!(f["title"].as_str().unwrap().contains("2 route(s)"), "{f}");
    eprintln!(
        "phase 2 ({:.1} s, cycle {}): {:?} title {:?}",
        p2.as_secs_f64(),
        st["cycle"],
        ids(&st),
        f["title"]
    );

    // Phase 3: events + budget.
    let (head, body) = http(A, "/api/v1/cortex/events").expect("feed");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let mut n = 0;
    for l in body.lines() {
        let e: NerveEvent = serde_json::from_str(l).expect("B33 line");
        if e.source == "ingress-lint" {
            n += 1;
        }
        assert!(
            !l.contains("t26.invalid") && !l.contains("never-read"),
            "{l}"
        );
    }
    assert!(n >= 1, "ingress-lint event in feed: {body}");
    let cycle_ms = st["cycle_ms"].as_u64().unwrap_or(u64::MAX);
    let rss = rss_kib(a.0.id());
    let us = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "ingress-lint")
        .map(|s| s["us"].clone())
        .unwrap_or_default();
    eprintln!("nerve A: cycle_ms {cycle_ms}, rss {rss} KiB, ingress-lint {us} us, feed events from ingress-lint {n}");
    assert!(cycle_ms < 50, "cycle budget: {cycle_ms} ms");
    assert!(rss > 0 && rss < 8 * 1024, "RSS budget: {rss} KiB");

    // Nerve B: symlink into a cloudflared dir with credentials-like text.
    let creds = dir.join("cloudflared/config.yml");
    std::fs::write(
        &creds,
        "{\"AccountTag\":\"t26\",\"TunnelSecret\":\"T26-MARKER-NOT-A-SECRET\"}\n",
    )
    .unwrap();
    let link = dir.join("innocent-copy.yml");
    std::os::unix::fs::symlink(&creds, &link).unwrap();
    let b = nerve(B, &dir, &link);
    let st = wait(B, 30, |st| !ids(st).is_empty());
    assert_eq!(ids(&st), ["ingress-config-refused"], "{}", st["cortex"]);
    let all = http(B, "/api/v1/state").unwrap().1 + &http(B, "/api/v1/cortex").unwrap().1;
    assert!(!all.contains("T26-MARKER"), "refused content never shown");
    eprintln!(
        "nerve B (symlink -> cloudflared/): {:?}, evidence {:?}",
        ids(&st),
        findings(&st)[0]["evidence"]
    );
    drop(b);
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}
