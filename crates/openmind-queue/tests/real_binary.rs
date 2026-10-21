// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-binary test, gated: runs only with `GALION_REAL=1`. Starts the
//! `openmind-queue` (path from `POSEIDON_BIN`, default cargo's own build of
//! the binary: run with `--release`) on 127.0.0.1:19121 (slot 12),
//! submits jobs, kills it with SIGKILL mid-lease, restarts it on the same
//! data directory and checks that nothing acknowledged was lost.
//! The full scenario lives in `tests/real_poseidon.rs`.

use openmind_queue::http;
use serde_json::{json, Value};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const ADDR: &str = "127.0.0.1:19121";

fn bin() -> String {
    // `cargo test --release` makes this the release binary.
    std::env::var("POSEIDON_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_openmind-queue").into())
}

fn call(m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request(ADDR, m, p, body.as_deref(), Duration::from_secs(5)).unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

fn start(data: &std::path::Path) -> Child {
    let mut child = Command::new(bin())
        .args(["serve", "--bind", ADDR, "--data"])
        .arg(data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start release binary (build it first)");
    for _ in 0..50 {
        if http::request(ADDR, "GET", "/healthz", None, Duration::from_millis(200)).is_ok() {
            return child;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("server did not come up on {ADDR}");
}

#[test]
fn real_binary_survives_sigkill() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-binary test");
        return;
    }
    let data = std::env::temp_dir().join(format!("poseidon-real-bin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let mut s = start(&data);
    for p in [3, 8, 1] {
        let (c, _) = call("POST", "/jobs", Some(json!({"kind":"echo","priority":p})));
        assert_eq!(c, 201);
    }
    let (_, l) = call(
        "POST",
        "/lease",
        Some(json!({"worker":"r","lease_ms":60000})),
    );
    assert_eq!(l["job"]["spec"]["priority"], 8);
    s.kill().unwrap(); // SIGKILL
    let _ = s.wait();
    let mut s = start(&data);
    let (_, st) = call("GET", "/stats", None);
    assert_eq!(st["depth"]["ready"], 2);
    assert_eq!(st["depth"]["leased"], 1);
    let path = format!("/jobs/{}/ack", l["id"]);
    let (c, _) = call("POST", &path, Some(json!({"token": l["token"]})));
    assert_eq!(c, 200);
    let (_, l2) = call("POST", "/lease", Some(json!({"worker":"r"})));
    assert_eq!(l2["job"]["spec"]["priority"], 3);
    s.kill().unwrap();
    let _ = s.wait();
    let _ = std::fs::remove_dir_all(&data);
}
