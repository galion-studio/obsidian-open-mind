// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of `omnode boot` (swarm slot 20, ports 19205-19206; with
//! GALION_REAL_PORT_BASE=B it uses B+5 and B+6 instead).
//! Runs only with GALION_REAL=1. Uses OMNODE_BIN (e.g. the x86_64 musl
//! release binary) or else the binary cargo built for this test
//! (`cargo test --release` gives the release one). Pairing codes and tokens
//! are never printed.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn enabled() -> bool {
    if std::env::var("GALION_REAL").as_deref() == Ok("1") {
        return true;
    }
    eprintln!("SKIP: set GALION_REAL=1 to run the real-life edge-boot test");
    false
}

/// Port block: GALION_REAL_PORT_BASE (the real-test harness sets it) or 19200.
fn port_base() -> u16 {
    std::env::var("GALION_REAL_PORT_BASE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(19200)
}

fn bin() -> PathBuf {
    std::env::var_os("OMNODE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_omnode")))
}

fn http(port: u16, method: &str, path: &str, body: &str, bearer: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let auth = if bearer.is_empty() {
        String::new()
    } else {
        format!("Authorization: Bearer {bearer}\r\n")
    };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out.get(9..12).and_then(|c| c.parse().ok()).unwrap_or(0);
    let b = out.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (code, b)
}

fn field(json: &str, key: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .get(key)
        .cloned()
}

fn boot(state: &Path, port: u16, code_file: &Path, ttl: u32) -> Child {
    Command::new(bin())
        .args(["boot", "--state"])
        .arg(state)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--code-file"])
        .arg(code_file)
        .args(["--code-ttl", &ttl.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn omnode")
}

fn wait_up(port: u16, boot_count: u64) -> serde_json::Value {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) {
        let (c, b) = http(port, "GET", "/v1/node", "", "");
        if c == 200 && field(&b, "boot_count").and_then(|v| v.as_u64()) == Some(boot_count) {
            return serde_json::from_str(&b).unwrap();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("omnode did not come up on {port}");
}

fn term(mut c: Child) -> i32 {
    // SAFETY: plain kill(2) on our own child.
    unsafe { libc::kill(c.id() as libc::pid_t, libc::SIGTERM) };
    c.wait().unwrap().code().unwrap_or(-1)
}

#[test]
fn real_first_boot_pairing_restart() {
    if !enabled() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("omnode-real-edge-boot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let state = dir.join("state");
    let cf = dir.join("code");
    let port = port_base() + 5;
    let port2 = port_base() + 6;

    let mut c = boot(&state, port, &cf, 300);
    let node = wait_up(port, 1);
    let dev = node["device_id"].as_str().unwrap().to_string();
    let code = std::fs::read_to_string(&cf).unwrap().trim().to_string();
    assert_eq!(http(port, "GET", "/v1/whoami", "", "").0, 401);
    let (c403, b) = http(port, "POST", "/v1/pair", r#"{"code":"AAAA-AAAA"}"#, "");
    assert_eq!((c403, field(&b, "error")), (403, Some("no".into())));
    let (c200, b) = http(
        port,
        "POST",
        "/v1/pair",
        &format!(r#"{{"code":"{code}"}}"#),
        "",
    );
    assert_eq!(c200, 200);
    let tok = field(&b, "token").unwrap().as_str().unwrap().to_string();
    assert_eq!(http(port, "GET", "/v1/whoami", "", &tok).0, 200);

    // kill -9 and boot again on the same state
    c.kill().unwrap();
    c.wait().unwrap();
    let c2 = boot(&state, port, &cf, 300);
    let node = wait_up(port, 2);
    assert_eq!(node["device_id"].as_str(), Some(dev.as_str()));
    assert_eq!(http(port, "GET", "/v1/whoami", "", &tok).0, 200);
    let (c, _) = http(
        port,
        "POST",
        "/v1/pair",
        &format!(r#"{{"code":"{code}"}}"#),
        "",
    );
    assert_eq!(c, 403, "single-use code accepted after restart");
    assert_eq!(term(c2), 0);

    // expired code on a second node
    let state2 = dir.join("state2");
    let cf2 = dir.join("code2");
    let n2 = boot(&state2, port2, &cf2, 1);
    wait_up(port2, 1);
    std::thread::sleep(Duration::from_millis(2200));
    let code2 = std::fs::read_to_string(&cf2).unwrap().trim().to_string();
    let (c, b) = http(
        port2,
        "POST",
        "/v1/pair",
        &format!(r#"{{"code":"{code2}"}}"#),
        "",
    );
    assert_eq!((c, field(&b, "error")), (403, Some("expired".into())));
    assert_eq!(term(n2), 0);
    std::fs::remove_dir_all(&dir).ok();
}
