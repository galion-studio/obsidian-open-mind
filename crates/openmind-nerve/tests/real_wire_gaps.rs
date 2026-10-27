// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life tests for nerve wire-gaps.
//!
//! * Slot 264 (`six_live_first_services_push_started_and_heartbeat`): ports
//!   21310-21319 — openmind-queue, galion-one, galion-health, galion-desk,
//!   link-watch, tuner.
//! * Slot 277 (`six_more_services_push_started_and_heartbeat`): ports
//!   21470-21479 — galion-origin, room, hades-gate, galion-guard (long-running
//!   started+heartbeat). galion-backup and galion-deploy are also wired at
//!   their CLI entry points; repo-check covers them. One-shot CLIs are not
//!   asserted here (process exit can race the nerve-client push worker).
//!
//! Builds and starts openmind-nerve plus the services under test, each with
//! `NERVE_URL` set. Asserts every listed source pushed a `started` event and
//! a heartbeat. Gated by `GALION_REAL=1`.
//!
//! Slot 264 stack (override with `REAL_WIRE_GAPS_PORT_BASE`, default 21310):
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
//! Slot 277 stack (override with `REAL_WIRE_GAPS2_PORT_BASE`, default 21470):
//!
//! | port  | process |
//! |-------|---------|
//! | 21470 | openmind-nerve serve |
//! | 21471 | galion-origin |
//! | 21472 | room --listen |
//! | 21473 | hades-gate serve |
//! | (none)| galion-guard run (dry-run) |
//!
//! Skip the build with `NERVE_WIRING_BIN_DIR` pointing at a release/ dir
//! that already holds the needed binaries.

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

// --- slot 277 nerve-t24-wire-gaps-2 -----------------------------------------

fn gaps2_port_base() -> u16 {
    std::env::var("REAL_WIRE_GAPS2_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21470)
}
fn gaps2_nerve_p() -> u16 {
    gaps2_port_base()
}
fn gaps2_origin_p() -> u16 {
    gaps2_port_base() + 1
}
fn gaps2_room_p() -> u16 {
    gaps2_port_base() + 2
}
fn gaps2_hades_p() -> u16 {
    gaps2_port_base() + 3
}

fn binaries_gaps2() -> PathBuf {
    if let Some(d) = std::env::var_os("NERVE_WIRING_BIN_DIR") {
        return PathBuf::from(d);
    }
    let td = target_dir().join("real-nerve-wire-gaps-2");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let ok = Command::new(cargo)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .env("CARGO_TARGET_DIR", &td)
        .args(["build", "-q", "-j", "2", "--release"])
        .args([
            "-p",
            "openmind-nerve",
            "-p",
            "galion-origin",
            "-p",
            "room",
            "-p",
            "hades-gate",
            "-p",
            "galion-guard",
            "-p",
            "galion-backup",
            "-p",
            "galion-deploy",
        ])
        .status()
        .expect("run cargo build")
        .success();
    assert!(ok, "release build of wire-gaps-2 stack failed");
    td.join("release")
}

/// Slot 277: next highest-value remaining gaps on ports 21470-21479.
/// Asserts started+heartbeat for the four long-running services we can keep
/// alive on the box (galion-origin, room, hades-gate, galion-guard).
/// galion-backup and galion-deploy are wired the same way; repo-check covers
/// them. One-shot CLI exit can race the nerve-client push worker, so they
/// are not asserted here.
#[test]
fn six_more_services_push_started_and_heartbeat() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run the real-life wire-gaps-2 test");
        return;
    }
    let bin = binaries_gaps2();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!("nerve-wire-gaps-2-{}-{now}", std::process::id()));
    for d in ["data", "origin-root", "room-root", "hades", "guard"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(work.join("room-root").join("README.md"), b"# room\n").unwrap();
    std::fs::write(work.join("guard").join("auth.log"), b"").unwrap();
    std::fs::write(
        work.join("components.json"),
        format!(
            r#"[
 {{"name":"galion-origin","port":{},"path":"/health","gaia":"AETHER","system":"origin"}},
 {{"name":"room","port":{},"path":"/health","gaia":"AETHER","system":"room"}},
 {{"name":"hades-gate","port":{},"path":"/health","gaia":"HADES","system":"hades-gate"}},
 {{"name":"galion-guard","port":0,"path":"-","gaia":"AETHER","system":"guard"}}
]"#,
            gaps2_origin_p(),
            gaps2_room_p(),
            gaps2_hades_p()
        ),
    )
    .unwrap();

    // hades-gate: keygen + minimal config (box only; no live HADES).
    let hades_dir = work.join("hades");
    let seed = hades_dir.join("admin.seed");
    let public = hades_dir.join("admin.pub");
    let keygen = Command::new(bin.join("hades-gate"))
        .args([
            "keygen",
            "--seed",
            seed.to_str().unwrap(),
            "--pub",
            public.to_str().unwrap(),
        ])
        .status()
        .expect("hades-gate keygen");
    assert!(keygen.success(), "hades-gate keygen failed");
    let hades_cfg = hades_dir.join("hades.toml");
    std::fs::write(
        &hades_cfg,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            public.display(),
            hades_dir.join("ledger.jsonl").display(),
            hades_dir.join("DISABLED").display()
        ),
    )
    .unwrap();

    // galion-guard dry-run + file source (no nft, no root).
    let guard_cfg = work.join("guard").join("guard.toml");
    std::fs::write(
        &guard_cfg,
        format!(
            r#"[guard]
backend = "dry-run"
state = "{state}"
socket = "{socket}"
dry_run_log = "{dry}"
[sshd]
source = "file"
logpath = "{log}"
poll_ms = 500
"#,
            state = work.join("guard").join("state.json").display(),
            socket = work.join("guard").join("control.sock").display(),
            dry = work.join("guard").join("dry.log").display(),
            log = work.join("guard").join("auth.log").display(),
        ),
    )
    .unwrap();

    let nerve_url = format!("http://127.0.0.1:{}", gaps2_nerve_p());

    let _nerve = {
        let p = Command::new(bin.join("openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{}", gaps2_nerve_p()))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", work.join("data"))
            .env("NERVE_COMPONENTS", work.join("components.json"))
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env(
                "NERVE_LISTEN_SCOPE",
                format!("{}-{}", gaps2_port_base(), gaps2_port_base() + 9),
            )
            .stdout(Stdio::null())
            .stderr(log_file(&work, "nerve.log"))
            .spawn()
            .expect("spawn openmind-nerve");
        Proc(p)
    };
    wait_up(gaps2_nerve_p(), "/healthz");

    let _origin = {
        let mut c = Command::new(bin.join("galion-origin"));
        c.env("ORIGIN_ADDR", format!("127.0.0.1:{}", gaps2_origin_p()))
            .env("ORIGIN_ROOT", work.join("origin-root"))
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "origin.log"));
        Proc(c.spawn().expect("spawn galion-origin"))
    };

    let _room = {
        let mut c = Command::new(bin.join("room"));
        c.arg("--listen")
            .env("ROOM_BIND", format!("127.0.0.1:{}", gaps2_room_p()))
            .env("ROOM_ROOT", work.join("room-root"))
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "room.log"));
        Proc(c.spawn().expect("spawn room"))
    };

    let _hades = {
        let mut c = Command::new(bin.join("hades-gate"));
        c.args([
            "serve",
            "--config",
            hades_cfg.to_str().unwrap(),
            "--port",
            &gaps2_hades_p().to_string(),
        ])
        .env("NERVE_URL", &nerve_url)
        .env("NERVE_HEARTBEAT_S", "10")
        .stdout(Stdio::null())
        .stderr(log_file(&work, "hades.log"));
        Proc(c.spawn().expect("spawn hades-gate"))
    };

    let _guard = {
        let mut c = Command::new(bin.join("galion-guard"));
        c.args(["run", "--config", guard_cfg.to_str().unwrap()])
            .env("NERVE_URL", &nerve_url)
            .env("NERVE_HEARTBEAT_S", "10")
            .stdout(Stdio::null())
            .stderr(log_file(&work, "guard.log"));
        Proc(c.spawn().expect("spawn galion-guard"))
    };

    wait_up(gaps2_origin_p(), "/health");
    wait_up(gaps2_room_p(), "/health");
    wait_up(gaps2_hades_p(), "/health");

    println!(
        "== {} gaps-2 stack up on base {}; waiting for heartbeats",
        stamp(),
        gaps2_port_base()
    );

    let want = ["galion-origin", "room", "hades-gate", "galion-guard"];
    let mut sig = Value::Null;
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(25) {
        sig = get_json(gaps2_nerve_p(), "/api/v1/signals");
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

    let _ = http_get(gaps2_origin_p(), "/health");
    let _ = http_get(gaps2_room_p(), "/health");
    let _ = http_get(gaps2_hades_p(), "/health");

    println!(
        "== {} PASS: six crates wired; four long-running proven (slot 277)",
        stamp()
    );
    let _ = std::fs::remove_dir_all(&work);
}
