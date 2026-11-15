// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test for F-354-1 (wave-4 slot 367): start the real
//! `openmind-memory` binary on 127.0.0.1 (slot ports 22370-22379, override
//! with `REAL_MEMORY_LOCALTIME_PORT`), seed DST-boundary rows into a fresh
//! fixture DB, capture a live message over HTTP, then check that
//! `/search/conversations` prints Europe/Zagreb stamps with an explicit offset
//! while the DB keeps the UTC `toISOString()` value. Gated: `GALION_REAL=1`.
//! Synthetic data only.

use openmind_memory::store::{L0Record, Store};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn port() -> u16 {
    std::env::var("REAL_MEMORY_LOCALTIME_PORT")
        .ok()
        .map(|v| {
            v.parse()
                .expect("REAL_MEMORY_LOCALTIME_PORT must be a port")
        })
        .unwrap_or(22370)
}

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skip: set GALION_REAL=1");
    }
    on
}

fn bin() -> PathBuf {
    std::env::var_os("OPENMIND_MEMORY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openmind-memory")))
}

struct Server(Option<Child>);

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> std::io::Result<(u16, Value)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Ok((status, serde_json::from_str(rest).unwrap_or(Value::Null)))
}

fn start(dir: &Path) -> Server {
    let p = port();
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} already in use"
    );
    let mut cmd = Command::new(bin());
    for k in [
        "OPENMIND_MEMORY_API_KEY",
        "TDAI_GATEWAY_API_KEY",
        "OPENMIND_MEMORY_READ_ONLY",
        "TDAI_INSTANCE_ID",
        "TDAI_DATA_DIR",
    ] {
        cmd.env_remove(k);
    }
    let child = cmd
        .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", p.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", dir)
        .env("OPENMIND_MEMORY_INSTANCE_ID", "default")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openmind-memory");
    let srv = Server(Some(child));
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        if let Ok((200, _)) = http(p, "GET", "/health", None) {
            return srv;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("openmind-memory on {p} did not become healthy");
}

fn now_ms() -> i64 {
    std::time::SystemTime::now() // repo-check: allow (epoch ms for the /capture wire timestamp, never printed)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn real_search_conversations_prints_zagreb_offsets() {
    if !real() {
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-localtime");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("vectors.db");
    {
        let st = Store::open(&db, false).unwrap();
        let extra = extra_rows();
        for (id, text, at) in [
            ("dst-a", "korcula ferry first", "2026-10-25T00:30:00.000Z"), // repo-check: allow (stored UTC row under test)
            ("dst-b", "korcula ferry second", "2026-10-25T01:30:00.000Z"), // repo-check: allow (stored UTC row under test)
        ]
        .into_iter()
        .chain(
            extra
                .iter()
                .map(|(id, at, _)| (id.as_str(), "korcula vps stamp", at.as_str())),
        ) {
            st.upsert_l0(&L0Record {
                id: id.into(),
                session_key: "dst".into(),
                session_id: "dst".into(),
                role: "user".into(),
                message_text: text.into(),
                recorded_at: at.into(),
                timestamp: 1,
            })
            .unwrap();
        }
    }
    let srv = start(&dir);
    let p = port();

    // A live capture: recorded_at is stamped "now" by the server.
    let before = zagreb_time::now_rfc3339();
    let ts = now_ms() + 86_400_000; // ahead of any cursor
    let (s, v) = http(
        p,
        "POST",
        "/capture",
        Some(&json!({
            "user_content": "korcula ferry live", "assistant_content": "ok",
            "session_key": "live",
            "messages": [{"role": "user", "content": "korcula ferry live", "timestamp": ts}],
        })),
    )
    .unwrap();
    assert_eq!((s, v["l0_recorded"].clone()), (200, json!(1)), "{v}");
    let after = zagreb_time::now_rfc3339();

    let (s, v) = http(
        p,
        "POST",
        "/search/conversations",
        Some(&json!({"query": "korcula", "limit": 10})),
    )
    .unwrap();
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 3 + extra_rows().len(), "{v}");
    let text = v["results"].as_str().unwrap().to_string();
    println!("--- /search/conversations (port {p}) ---\n{text}\n---");
    assert!(
        text.contains("Session: dst [2026-10-25T02:30:00+02:00]"),
        "{text}"
    );
    assert!(
        text.contains("Session: dst [2026-10-25T02:30:00+01:00]"),
        "{text}"
    );
    assert!(!text.contains("Z]") && !text.contains("+00:00"), "{text}");
    for (_, stored, want) in extra_rows() {
        assert!(
            text.contains(&format!("[{want}]")),
            "{stored} -> {want} missing:\n{text}"
        );
        println!("real VPS stamp {stored} -> [{want}] ok");
    }

    // The live row: Zagreb RFC 3339 between before/after (same offset).
    let live = text
        .split("Session: live [")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .expect("live row stamp");
    assert_eq!(zagreb_time::require_rfc3339(live), Ok(()), "{live}");
    assert!(
        before.as_str() <= live && live <= after.as_str(),
        "{before} <= {live} <= {after}"
    );
    println!("live stamp {live} (window {before} .. {after})");
    drop(srv);

    // Storage unchanged: UTC toISOString in the DB.
    let c = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let stored: String = c
        .query_row(
            "SELECT recorded_at FROM l0_conversations WHERE session_key='live'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(stored.ends_with('Z') && stored.len() == 24, "{stored}");
    println!("stored recorded_at {stored} (UTC, unchanged wire/storage format)");
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} still open after stop"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Optional real stored stamps (read-only from the live VPS DB) with the
/// expected Zagreb rendering from an independent oracle (GNU `date`):
/// `REAL_MEMORY_LOCALTIME_STAMPS="stored=expected,..."`.
fn extra_rows() -> Vec<(String, String, String)> {
    std::env::var("REAL_MEMORY_LOCALTIME_STAMPS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .enumerate()
        .map(|(i, pair)| {
            let (stored, want) = pair.trim().split_once('=').expect("stored=expected");
            (format!("vps-{i}"), stored.to_string(), want.to_string())
        })
        .collect()
}
