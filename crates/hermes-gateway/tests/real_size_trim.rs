// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test for slot 294 (release-size-trim): the size-trimmed
//! hermes-gateway (SQLite built with the extra `.cargo/config.toml` omit
//! flags and without C unwind tables) must still serve, write its SQLite
//! session store and read it back after a restart. Gated: `GALION_REAL=1`.
//!
//! Knobs: `SIZE_TRIM_GATEWAY_BIN` (binary under test, e.g. the static musl
//! release build; default this crate's own build), `SIZE_TRIM_PORT`
//! (default 21646, slot 294 range 21640-21649). Offline only: `--mock`
//! provider, no network, no spend. The Bearer value is a fixture, not a secret.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "st294-fixture-key-0123456789";

fn real() -> bool {
    let on = std::env::var("GALION_REAL").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skip: set GALION_REAL=1");
    }
    on
}

fn bin() -> PathBuf {
    std::env::var_os("SIZE_TRIM_GATEWAY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")))
}

fn port() -> u16 {
    std::env::var("SIZE_TRIM_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21646)
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn(home: &Path, port: u16) -> Proc {
    let child = Command::new(bin())
        .arg("--mock")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("API_SERVER_KEY", KEY)
        .env("HERMES_HOME", home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_PROVIDER_LOOPBACK_ONLY", "1")
        .stdout(Stdio::null())
        .stderr(
            std::fs::File::create(home.join(format!("gw-{port}.log")))
                .map(Stdio::from)
                .unwrap_or(Stdio::null()),
        )
        .spawn()
        .expect("spawn hermes-gateway");
    Proc(child)
}

fn http(port: u16, method: &str, path: &str, extra: &[(&str, &str)], body: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .to_string();
    (code, body)
}

fn wait_up(port: u16) -> String {
    let t = Instant::now();
    loop {
        let (c, b) = http(port, "GET", "/health", &[], "");
        if c == 200 {
            return b;
        }
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "gateway :{port} not up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_free(port: u16) {
    let t = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "port {port} still busy"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn trimmed_gateway_serves_and_persists_sqlite_sessions() {
    if !real() {
        return;
    }
    let port = port();
    let b = bin();
    let size = std::fs::metadata(&b).map(|m| m.len()).unwrap_or(0);
    eprintln!("binary {} ({size} B, 2 MiB budget 2097152 B)", b.display());
    let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-size-trim");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let sid = "st294-session-a";
    let marker = "size-trim marker 294 ünïcode ✓";
    {
        let _gw = spawn(&home, port);
        let health = wait_up(port);
        eprintln!("GET /health -> 200 {health}");
        assert!(health.contains("\"ok\""), "health body: {health}");
        let (c, _) = http(port, "GET", "/v1/models", &[], "");
        assert_eq!(c, 200, "/v1/models");
        let body = serde_json::json!({"model": "hermes-agent", "messages": [{"role": "user", "content": marker}]}).to_string();
        for i in 0..3 {
            let (c, b) = http(
                port,
                "POST",
                "/v1/chat/completions",
                &[("X-Hermes-Session-Id", sid)],
                &body,
            );
            assert_eq!(c, 200, "chat {i}: {b}");
        }
        eprintln!("3x POST /v1/chat/completions (session {sid}) -> 200");
    }
    wait_free(port);
    let db = home.join("rs-state.db");
    let db_len = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);
    eprintln!("{} = {db_len} B after stop", db.display());
    assert!(db_len > 0, "SQLite session store not written");
    // Restart on the same HERMES_HOME: the session must come back from SQLite.
    let _gw = spawn(&home, port);
    wait_up(port);
    let (c, list) = http(port, "GET", "/api/sessions", &[], "");
    assert_eq!(c, 200, "/api/sessions: {list}");
    assert!(list.contains(sid), "session list after restart: {list}");
    let (c, msgs) = http(
        port,
        "GET",
        &format!("/api/sessions/{sid}/messages"),
        &[],
        "",
    );
    assert_eq!(c, 200, "messages: {msgs}");
    let v: serde_json::Value = serde_json::from_str(&msgs).expect("messages JSON");
    let text = v.to_string();
    assert!(
        text.contains("size-trim marker 294"),
        "marker missing after restart: {text}"
    );
    let user_turns = text.matches("size-trim marker 294").count();
    eprintln!("after restart: /api/sessions lists {sid}; messages hold the marker {user_turns}x");
    assert!(user_turns >= 3, "expected 3 user turns, got {user_turns}");
}
