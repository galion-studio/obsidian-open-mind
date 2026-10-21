// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the cortex `Sensor` registry (gated: `GALION_REAL=1`,
//! run with `--release` for the release binary). Slot 43: ports 19430-19439.
//!
//! 1. `openmind-nerve replay fixtures/cortex-replay` (the real binary) gives
//!    the pre-trait golden verdict (kind/source keys aside), and the live
//!    capture `/workspace/swarm/nerve-capture/replay1` (or `$CORTEX_REPLAY_DIR`)
//!    scores 56/D when present (61/D before the T3.2 member floor).
//! 2. `openmind-nerve serve` on 127.0.0.1:19430 with a live service (:19431),
//!    a dead one (:19432) and a stray listener (:19433): the `listeners`
//!    sensor reports it as a B33 event, then the finding clears when the
//!    stray goes away.
//! 3. `NERVE_SENSORS_OFF=listeners` on :19434 with a stray on :19435: the
//!    sensor is gone from the registry and reports nothing.

use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[allow(dead_code)] // shared golden helpers; each test uses a subset
mod support;

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

/// Poll `/api/v1/state` until `ok` holds (or 20 s pass).
fn wait_state(port: u16, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = get(port, "/api/v1/state").unwrap_or_default();
        if ok(&st) || t.elapsed() > Duration::from_secs(20) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn replay(dir: &Path, data: &Path, md: bool) -> (Option<i32>, String) {
    let mut c = Command::new(BIN);
    c.env_clear()
        .env("NERVE_DATA_DIR", data)
        .arg("replay")
        .arg(dir);
    if md {
        c.arg("--md");
    }
    let o = c.stderr(Stdio::null()).output().expect("run replay");
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn unmanaged(st: &Value) -> Option<Value> {
    st["cortex"]["vulnerabilities"]
        .as_array()?
        .iter()
        .find(|f| f["id"] == "unmanaged-listeners")
        .cloned()
}

fn spawn(port: u16, dir: &Path, comps: &Path, extra: &[(&str, &str)]) -> Kill {
    let mut c = Command::new(BIN);
    c.arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join(format!("data-{port}")))
        .env("NERVE_COMPONENTS", comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_LISTEN_SCOPE", "19430-19439")
        .env_remove("NERVE_SENSORS_OFF")
        .stderr(Stdio::null());
    for (k, v) in extra {
        c.env(k, v);
    }
    Kill(c.spawn().expect("spawn nerve"))
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

#[test]
fn real_cortex_sensors_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = std::env::temp_dir().join(format!("nerve-real-cortex-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Replay through the real binary: the pre-trait verdict.
    let (code, out) = replay(&fx.join("cortex-replay"), &dir.join("replay-data"), false);
    assert_eq!(code, Some(1), "a failing verdict exits 1");
    let mut got: Value = serde_json::from_str(&out).unwrap();
    assert!(got["cortex"]["vulnerabilities"][0]["kind"].is_string());
    support::drop_edge_hosts(&mut got); // T2.12 additive streams (tests/real_edge_rate.rs)
    support::drop_correlation(&mut got); // T3.2 additive incident `correlation`
    support::canon(&mut got);
    let mut want: Value = serde_json::from_str(
        &std::fs::read_to_string(fx.join("cortex-replay.expected.json")).unwrap(),
    )
    .unwrap();
    support::canon(&mut want);
    support::t32_delta(&mut want); // T3.2 member floor: scanner 5 -> 10, score 21 -> 16
    assert_eq!(got, want);
    let (_, md) = replay(&fx.join("cortex-replay"), &dir.join("replay-data"), true);
    assert_eq!(
        support::drop_correlation_md(&support::drop_edge_hosts_md(&md)),
        support::t32_delta_md(&support::expected_md(&fx))
    );
    eprintln!(
        "replay fixture: score {} grade {} pass {} (= pre-trait golden)",
        want["score"], want["grade"], want["pass"]
    );
    let live_cap = std::env::var("CORTEX_REPLAY_DIR")
        .unwrap_or_else(|_| "/workspace/swarm/nerve-capture/replay1".into());
    if Path::new(&live_cap).join("state-1.json").exists() {
        let (_, out) = replay(Path::new(&live_cap), &dir.join("replay-data"), false);
        let r: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            (r["score"].as_u64(), r["grade"].as_str()),
            (Some(56), Some("D")),
            "replay1 verdict changed (56/D since the T3.2 scanner member floor, 61/D before)"
        );
        eprintln!("replay1 live capture: score 56 grade D (61 before the T3.2 member floor)");
    } else {
        eprintln!("replay1 live capture not on this box: skipped");
    }

    // 2. Serve on 19430: stray listener appears, then clears.
    let svc = TcpListener::bind("127.0.0.1:19431").expect("19431 free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        r#"[{"name":"live-svc","port":19431},{"name":"dead-svc","port":19432,"gaia":"APOLLO"}]"#,
    )
    .unwrap();
    let stray = TcpListener::bind("127.0.0.1:19433").expect("19433 free");
    let nerve = spawn(19430, &dir, &comps, &[]);
    let st = wait_state(19430, |s| unmanaged(s).is_some());
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
            "edge-rate"
        ],
        "{st}"
    );
    let f = unmanaged(&st).expect("stray listener reported");
    assert_eq!(
        f["title"],
        "1 loopback listener(s) no monitored unit owns: :19433"
    );
    assert_eq!(
        (f["kind"].as_str(), f["source"].as_str()),
        (Some("vulnerability"), Some("listeners"))
    );
    let ev = st["cortex_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == "listeners")
        .cloned()
        .expect("listeners event");
    let ev: NerveEvent = serde_json::from_value(ev).expect("B33 NerveEvent");
    assert_eq!(
        (ev.kind.as_str(), ev.severity.as_str()),
        ("vulnerability", "warn")
    );
    assert!(
        ev.ts.ends_with("+02:00") || ev.ts.ends_with("+01:00"),
        "{}",
        ev.ts
    );
    let cortex = get(19430, "/api/v1/cortex").unwrap();
    assert_eq!(
        cortex["exposure"]["unmanaged_ports"],
        serde_json::json!([19433])
    );
    let us: Vec<u64> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["us"].as_u64())
        .collect();
    eprintln!(
        "serve :19430 cycle {}: sensors {names:?} us {us:?}, cycle_ms {}, rss {} KiB; event {}",
        st["cycle"],
        st["cycle_ms"],
        rss_kib(nerve.0.id()),
        serde_json::to_string(&ev).unwrap()
    );
    drop(stray);
    let st = wait_state(19430, |s| {
        s["cycle"].as_u64().unwrap_or(0) > 0 && unmanaged(s).is_none()
    });
    assert!(
        unmanaged(&st).is_none(),
        "finding cleared after the stray left: {}",
        st["cortex"]
    );
    assert!(!st["cortex_events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["source"] == "listeners"));
    eprintln!(
        "serve :19430 cycle {}: stray gone, finding cleared",
        st["cycle"]
    );
    drop(nerve);

    // 3. NERVE_SENSORS_OFF=listeners on 19434.
    let stray = TcpListener::bind("127.0.0.1:19435").expect("19435 free");
    let nerve = spawn(19434, &dir, &comps, &[("NERVE_SENSORS_OFF", "listeners")]);
    let st = wait_state(19434, |s| s["cycle"].as_u64().unwrap_or(0) >= 2);
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["scanners", "edge", "components", "ddos", "edge-rate"]
    );
    assert!(unmanaged(&st).is_none());
    assert!(st["cortex"]["exposure"].is_null());
    eprintln!("serve :19434 NERVE_SENSORS_OFF=listeners cycle {}: sensors {names:?}, stray :19435 not reported", st["cycle"]);
    drop(stray);
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
