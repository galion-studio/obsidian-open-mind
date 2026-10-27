// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for slot 264 nerve-t24-wire-gaps (ports 21310-21319).
//!
//! Builds and starts openmind-nerve plus the six live-first services that
//! slot 226 listed as highest-value nerve-client gaps, each with
//! `NERVE_URL` set. Asserts every source pushed a `started` event (and
//! that a heartbeat has arrived). Gated by `GALION_REAL=1`.
//!
//! | port  | process |
//! |-------|---------|
//! | 21310 | openmind-nerve serve |
//! | 21311 | openmind-queue serve |
//! | 21312 | galion-one (`ONE_BIND`) |
//! | 21313 | galion-one mouth (`ONE_MOUTH_BIND`) |
//! | 21314 | galion-health |
//! | 21315 | galion-desk |
//! | 21316 | tuner |
//! | (none)| link-watch (file writer; still pushes) |
//!
//! Override the base with `REAL_WIRE_GAPS_PORT_BASE` (default 21310).
//! Skip the build with `NERVE_WIRING_BIN_DIR` pointing at a release/ dir
//! that already holds the seven binaries.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn port_base() -> u16 {
    std::env::var("REAL_WIRE_GAPS_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21310)
}
fn nerve_p() -> u16 {
    port_base()
}
fn queue_p() -> u16 {
    port_base() + 1
}
fn one_p() -> u16 {
    port_base() + 2
}
fn mouth_p() -> u16 {
    port_base() + 3
}
fn health_p() -> u16 {
    port_base() + 4
}
fn desk_p() -> u16 {
    port_base() + 5
}
fn tuner_p() -> u16 {
    port_base() + 6
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let _ = self.0.wait();
    }
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

fn http_get(port: u16, path: &str) -> (u16, String) {
    let addr = format!("127.0.0.1:{port}").parse().unwrap();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    let req = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

fn get_json(port: u16, path: &str) -> Value {
    let (code, body) = http_get(port, path);
    assert_eq!(code, 200, "GET {path} -> {code} {body}");
    serde_json::from_str(&body).unwrap_or(Value::Null)
}

fn wait_up(port: u16, path: &str) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if http_get(port, path).0 == 200 {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("port {port} path {path} did not come up");
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
}

fn binaries() -> PathBuf {
    if let Some(d) = std::env::var_os("NERVE_WIRING_BIN_DIR") {
        return PathBuf::from(d);
    }
    let td = target_dir().join("real-nerve-wire-gaps");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let ok = Command::new(cargo)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .env("CARGO_TARGET_DIR", &td)
        .args(["build", "-q", "-j", "2", "--release"])
        .args([
            "-p",
            "openmind-nerve",
            "-p",
            "openmind-queue",
            "-p",
            "galion-one",
            "-p",
            "galion-health",
            "-p",
            "galion-desk",
            "-p",
            "link-watch",
            "-p",
            "tuner",
        ])
        .status()
        .expect("run cargo build")
        .success();
    assert!(ok, "release build of wire-gaps stack failed");
    td.join("release")
}

fn sources(sig: &Value) -> Vec<Value> {
    sig["sources"].as_array().cloned().unwrap_or_default()
}

fn source(sig: &Value, name: &str) -> Value {
    sources(sig)
        .into_iter()
        .find(|s| s["source"] == name)
        .unwrap_or(Value::Null)
}

fn has_started(sig: &Value, name: &str) -> bool {
    source(sig, name)["recent"].as_array().is_some_and(|r| {
        r.iter()
            .any(|e| e["kind"] == "event" && e["name"] == "started")
    })
}

fn has_heartbeat(sig: &Value, name: &str) -> bool {
    source(sig, name)["recent"]
        .as_array()
        .is_some_and(|r| r.iter().any(|e| e["kind"] == "heartbeat"))
}

fn log_file(work: &Path, name: &str) -> Stdio {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(work.join(name))
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null())
}

#[test]
fn six_live_first_services_push_started_and_heartbeat() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run the real-life wire-gaps test");
        return;
    }
    let bin = binaries();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!("nerve-wire-gaps-{}-{now}", std::process::id()));
    for d in [
        "data",
        "queue-data",
        "desk-state",
        "desk-one",
        "desk-hermes",
        "tuner-www",
        "link-out",
    ] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(
        work.join("components.json"),
        format!(
            r#"[
 {{"name":"openmind-queue","port":{},"path":"/stats","gaia":"POSEIDON","system":"queue"}},
 {{"name":"galion-one","port":{},"path":"/health","gaia":"AETHER","system":"one"}},
 {{"name":"galion-health","port":{},"path":"/healthz","gaia":"AETHER","system":"health"}},
 {{"name":"galion-desk","port":{},"path":"/health","gaia":"AETHER","system":"desk"}},
 {{"name":"tuner","port":{},"path":"/health","gaia":"AETHER","system":"tuner"}},
 {{"name":"link-watch","port":0,"path":"-","gaia":"AETHER","system":"link-watch"}}
]"#,
            queue_p(),
            one_p(),
            health_p(),
            desk_p(),
            tuner_p()
        ),
    )
    .unwrap();

    let nerve_url = format!("http://127.0.0.1:{}", nerve_p());

    let _nerve = {
        let p = Command::new(bin.join("openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{}", nerve_p()))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", work.join("data"))
            .env("NERVE_COMPONENTS", work.join("components.json"))
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env(
                "NERVE_LISTEN_SCOPE",
                format!("{}-{}", port_base(), port_base() + 9),
            )
            .stdout(Stdio::null())
            .stderr(log_file(&work, "nerve.log"))
            .spawn()
            .expect("spawn openmind-nerve");
        Proc(p)
    };
    wait_up(nerve_p(), "/healthz");

    let _queue = {
        let mut c = Command::new(bin.join("openmind-queue"));
        c.args([
            "serve",
            "--bind",
            &format!("127.0.0.1:{}", queue_p()),
            "--data",
            work.join("queue-data").to_str().unwrap(),
        ])
        .env("NERVE_URL", &nerve_url)
        .env("NERVE_HEARTBEAT_S", "10")
        .stdout(Stdio::null())
        .stderr(log_file(&work, "queue.log"));
        Proc(c.spawn().expect("spawn openmind-queue"))
    };

    let _one = {
        let mut c = Command::new(bin.join("galion-one"));
        c.env("ONE_BIND", format!("127.0.0.1:{}", one_p()))
            .env("ONE_MOUTH_BIND", format!("127.0.0.1:{}", mouth_p()))
            .env("ONE_NO_WARM", "1")
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "one.log"));
        Proc(c.spawn().expect("spawn galion-one"))
    };

    let _health = {
        let mut c = Command::new(bin.join("galion-health"));
        c.env("GALION_HEALTH_ADDR", format!("127.0.0.1:{}", health_p()))
            .env("GALION_HEALTH_CHECKS", "")
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "health.log"));
        Proc(c.spawn().expect("spawn galion-health"))
    };

    let _desk = {
        let mut c = Command::new(bin.join("galion-desk"));
        c.env("DESK_BIND", format!("127.0.0.1:{}", desk_p()))
            .env("DESK_STATE_DIR", work.join("desk-state"))
            .env("DESK_ONE_DIR", work.join("desk-one"))
            .env("DESK_HERMES_DIR", work.join("desk-hermes"))
            .env("OPENMIND_CONFIG", work.join("missing-openmind.toml"))
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "desk.log"));
        Proc(c.spawn().expect("spawn galion-desk"))
    };

    let _tuner = {
        let mut c = Command::new(bin.join("tuner"));
        c.env("TUNER_ADDR", format!("127.0.0.1:{}", tuner_p()))
            .env("TUNER_ROOT", work.join("tuner-www"))
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "tuner.log"));
        Proc(c.spawn().expect("spawn tuner"))
    };

    let _link = {
        let mut c = Command::new(bin.join("link-watch"));
        c.env("LINK_WATCH_OUT", work.join("link-out").join("link.json"))
            .env("LINK_WATCH_OLLAMA_URL", "http://127.0.0.1:9")
            .env("LINK_WATCH_HERMES_URL", "http://127.0.0.1:9")
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "link-watch.log"));
        Proc(c.spawn().expect("spawn link-watch"))
    };

    wait_up(queue_p(), "/stats");
    wait_up(health_p(), "/healthz");
    wait_up(desk_p(), "/health");
    wait_up(tuner_p(), "/health");
    // galion-one /health may 200; if not, still wait for listen by retrying started signals
    wait_listen(one_p());

    println!(
        "== {} stack up on base {}; waiting for heartbeats",
        stamp(),
        port_base()
    );
    // Heartbeat interval is 10 s; wait up to 25 s for all six sources.
    let want = [
        "openmind-queue",
        "galion-one",
        "galion-health",
        "galion-desk",
        "link-watch",
        "tuner",
    ];
    let mut sig = Value::Null;
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(25) {
        sig = get_json(nerve_p(), "/api/v1/signals");
        if want
            .iter()
            .all(|n| has_started(&sig, n) && has_heartbeat(&sig, n))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    println!("-- pushed sources");
    for s in sources(&sig) {
        println!(
            "{}",
            serde_json::json!({
                "source": s["source"],
                "total": s["total"],
                "dropped": s["dropped"],
            })
        );
    }

    for name in want {
        assert!(
            has_started(&sig, name),
            "missing started event from {name}: {}",
            source(&sig, name)
        );
        assert!(
            has_heartbeat(&sig, name),
            "missing heartbeat from {name}: {}",
            source(&sig, name)
        );
    }

    // Probe HTTP paths so observe_request has something to count (no assert on 5xx).
    let _ = http_get(queue_p(), "/stats");
    let _ = http_get(health_p(), "/healthz");
    let _ = http_get(desk_p(), "/health");
    let _ = http_get(tuner_p(), "/health");

    println!("== {} PASS: six live-first services wired", stamp());
    let _ = std::fs::remove_dir_all(&work);
}

fn wait_listen(port: u16) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        let addr = format!("127.0.0.1:{port}").parse().unwrap();
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("port {port} did not accept");
}
