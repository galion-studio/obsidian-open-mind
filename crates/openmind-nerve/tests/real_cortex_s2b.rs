// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the S2 sensors `drift` (T2.5) and `conn-rate` (T2.7),
//! gated `GALION_REAL=1`; run with `--release`. Slot 56: ports 19560-19569.
//!
//! `openmind-nerve serve` on 127.0.0.1:19560 (interval 5 s) watches a live
//! service on :19561 and a release manifest listing the real nerve binary and
//! a "deployed" copy:
//! 1. clean: both sensors registered, no drift / conn-rate finding;
//! 2. the deployed copy is patched -> `drift-<name>` (B33 event `drift`,
//!    `high`); restored -> cleared;
//! 3. ~100 connections/s to :19561 -> `conn-rate-19561` bleeding; load
//!    stops -> cleared.
//!
//! Optional: `CONNRATE_VPS_DIR` with `tcp-0.txt` / `tcp-1.txt` (two read-only
//! `/proc/net/tcp*` reads of the VPS, `CONNRATE_VPS_DT` seconds apart) prints
//! the per-port new-connection rate the sensor would compute for 8420 & co.

use openmind_nerve::sensors::connrate;
use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

fn wait_state(port: u16, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = get(port, "/api/v1/state").unwrap_or_default();
        if ok(&st) || t.elapsed() > Duration::from_secs(secs) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn finding(st: &Value, id: &str) -> Option<Value> {
    ["vulnerabilities", "bleeding"]
        .iter()
        .filter_map(|k| st["cortex"][k].as_array())
        .flatten()
        .find(|f| f["id"] == id)
        .cloned()
}

fn event(st: &Value, source: &str) -> Option<NerveEvent> {
    let e = st["cortex_events"]
        .as_array()?
        .iter()
        .find(|e| e["source"] == source)?;
    serde_json::from_value(e.clone()).ok()
}

fn sensor_us(st: &Value) -> String {
    st["cortex_sensors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| format!("{}={}us", s["name"].as_str().unwrap_or("?"), s["us"]))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
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

fn sha_hex(p: &Path) -> String {
    let b = std::fs::read(p).unwrap();
    galion_common::hex::encode(&galion_common::sha256::digest(&b))
}

fn vps_compare() {
    let Ok(dir) = std::env::var("CONNRATE_VPS_DIR") else {
        eprintln!("CONNRATE_VPS_DIR unset: VPS comparison skipped");
        return;
    };
    let dt: f64 = std::env::var("CONNRATE_VPS_DT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30.0);
    let ports = [8420u16, 8642, 8646, 8650, 8091, 8670];
    let read = |k| std::fs::read_to_string(Path::new(&dir).join(format!("tcp-{k}.txt"))).unwrap();
    let (a, b) = (
        connrate::parse_tables(&read(0), &ports),
        connrate::parse_tables(&read(1), &ports),
    );
    for p in ports {
        let (x, y) = (&a[&p], &b[&p]);
        let new = y.conns.difference(&x.conns).count();
        let growth = (y.estab + y.time_wait) as f64 - (x.estab + x.time_wait) as f64;
        eprintln!(
            "VPS :{p}: estab {}->{} time_wait {}->{}; new {new} = {:.2}/s; B41 growth {:.2}/s",
            x.estab,
            y.estab,
            x.time_wait,
            y.time_wait,
            new as f64 / dt,
            growth / dt
        );
    }
}

#[test]
fn real_drift_and_conn_rate_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    vps_compare();
    let dir = std::env::temp_dir().join(format!("nerve-real-s2b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("deployed")).unwrap();

    // Release manifest: the real nerve binary (absolute) + a deployed copy.
    let copy = dir.join("deployed/s2b-svc");
    std::fs::copy(BIN, &copy).unwrap();
    let manifest = dir.join("SHA256SUMS");
    std::fs::write(
        &manifest,
        format!(
            "# release manifest (real test)\n{}  {BIN}\n{}  s2b-svc\n",
            sha_hex(Path::new(BIN)),
            sha_hex(&copy)
        ),
    )
    .unwrap();
    let bin_mib = std::fs::metadata(BIN).unwrap().len() as f64 / 1048576.0;

    // Live service on :19561.
    let svc = TcpListener::bind("127.0.0.1:19561").expect("19561 free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 512];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    let comps = dir.join("components.json");
    std::fs::write(&comps, r#"[{"name":"s2b-svc","port":19561}]"#).unwrap();

    let mut c = Command::new(BIN);
    c.arg("serve")
        .env("NERVE_ADDR", "127.0.0.1:19560")
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_LISTEN_SCOPE", "19560-19569")
        .env("NERVE_DRIFT_MANIFEST", &manifest)
        .env("NERVE_DRIFT_ROOT", dir.join("deployed"))
        .env("NERVE_CONNRATE_PORTS", "19561")
        .env_remove("NERVE_SENSORS_OFF")
        .env_remove("NERVE_DRIFT_BUDGET_KIB")
        .stderr(Stdio::null());
    let nerve = Kill(c.spawn().expect("spawn nerve"));

    // 1. Clean. Default budget 2 MiB/cycle: the binary needs ~MiB/2 cycles.
    let cycles_needed = (bin_mib / 2.0).ceil() as u64 + 2;
    let st = wait_state(19560, 5 * cycles_needed + 20, |s| {
        s["cycle"].as_u64().unwrap_or(0) >= cycles_needed
    });
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .expect("cortex_sensors")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "listeners",
            "scanners",
            "edge",
            "components",
            "ddos",
            "drift",
            "conn-rate"
        ],
        "{st}"
    );
    assert!(
        event(&st, "drift").is_none(),
        "clean manifest: {}",
        st["cortex"]
    );
    assert!(finding(&st, "conn-rate-19561").is_none());
    eprintln!(
        "serve :19560 cycle {} clean (binary {bin_mib:.1} MiB hashed at 2 MiB/cycle): {}; cycle_ms {}; rss {} KiB",
        st["cycle"],
        sensor_us(&st),
        st["cycle_ms"],
        rss_kib(nerve.0.id())
    );

    // 2. Patch the deployed copy (same size, one byte flipped).
    let mut b = std::fs::read(&copy).unwrap();
    let k = b.len() / 2;
    b[k] ^= 0xff;
    std::fs::write(&copy, &b).unwrap();
    let st = wait_state(19560, 5 * cycles_needed + 20, |s| {
        finding(s, "drift-s2b-svc").is_some()
    });
    let f = finding(&st, "drift-s2b-svc").expect("drift reported");
    assert_eq!(
        (
            f["kind"].as_str(),
            f["source"].as_str(),
            f["severity"].as_str()
        ),
        (Some("vulnerability"), Some("drift"), Some("error"))
    );
    assert_eq!(
        f["title"],
        "s2b-svc: deployed file differs from the release manifest"
    );
    assert_eq!(f["component"], "s2b-svc", "basename maps to the component");
    let ev = event(&st, "drift").expect("drift event");
    assert_eq!(
        (ev.kind.as_str(), ev.severity.as_str()),
        ("vulnerability", "high")
    );
    let evs = serde_json::to_string(&ev).unwrap();
    assert!(
        !evs.contains(dir.to_str().unwrap()),
        "no path in event: {evs}"
    );
    assert!(ev.ts.ends_with("+02:00") || ev.ts.ends_with("+01:00"));
    eprintln!("cycle {}: patched -> event {evs}", st["cycle"]);
    b[k] ^= 0xff;
    std::fs::write(&copy, &b).unwrap();
    let st = wait_state(19560, 5 * cycles_needed + 20, |s| {
        s["cycle"].as_u64().is_some() && event(s, "drift").is_none()
    });
    assert!(event(&st, "drift").is_none(), "{}", st["cortex"]);
    eprintln!("cycle {}: restored -> drift cleared", st["cycle"]);

    // 3. Connection burst to :19561 (~100/s) until the finding shows.
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let made = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let made2 = made.clone();
    let load = std::thread::spawn(move || {
        while !stop2.load(Ordering::Relaxed) {
            if let Ok(mut s) = TcpStream::connect("127.0.0.1:19561") {
                let _ = s.write_all(b"GET / HTTP/1.0\r\n\r\n");
                let mut x = Vec::new();
                let _ = s.read_to_end(&mut x);
                made2.fetch_add(1, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let t0 = Instant::now();
    let st = wait_state(19560, 40, |s| finding(s, "conn-rate-19561").is_some());
    stop.store(true, Ordering::Relaxed);
    load.join().unwrap();
    let f = finding(&st, "conn-rate-19561").expect("conn-rate reported");
    assert_eq!(
        (f["kind"].as_str(), f["source"].as_str()),
        (Some("bleeding"), Some("s2b-svc"))
    );
    let ev = event(&st, "s2b-svc").expect("conn-rate event");
    assert_eq!(ev.kind, "bleeding");
    assert!(
        ["high", "crit"].contains(&ev.severity.as_str()),
        "{}",
        ev.severity
    );
    eprintln!(
        "cycle {}: {} connections made in {:.1} s -> {} ({}); event {}; {}",
        st["cycle"],
        made.load(Ordering::Relaxed),
        t0.elapsed().as_secs_f64(),
        f["title"],
        ev.severity,
        serde_json::to_string(&ev).unwrap(),
        sensor_us(&st)
    );
    let st = wait_state(19560, 30, |s| {
        s["cycle"].as_u64().is_some() && finding(s, "conn-rate-19561").is_none()
    });
    assert!(
        finding(&st, "conn-rate-19561").is_none(),
        "{}",
        st["cortex"]
    );
    eprintln!(
        "cycle {}: load stopped -> conn-rate cleared; cycle_ms {}; rss {} KiB",
        st["cycle"],
        st["cycle_ms"],
        rss_kib(nerve.0.id())
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
