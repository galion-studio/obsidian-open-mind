// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
//! Operator subcommands for the B60 shadow run (no server involved):
//! - `snapshot <src.db> <dst.db>`: consistent copy via the SQLite online
//!   backup API (source opened read-only; safe while the gateway writes).
//! - `parity <base_a> <base_b> <queries.txt> [session_key]`: POST the same
//!   read-only requests (/recall, /search/memories, /search/conversations)
//!   to both servers and print match/diff with lengths and hash prefixes
//!   only, never memory content. Bearer from OPENMIND_MEMORY_API_KEY or
//!   TDAI_GATEWAY_API_KEY (never printed).

use crate::hash::sha256_hex;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

pub fn snapshot(src: &Path, dst: &Path) -> Result<(), String> {
    let s = Connection::open_with_flags(
        src,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open {}: {e}", src.display()))?;
    if dst.exists() {
        return Err(format!("{} exists; refusing to overwrite", dst.display()));
    }
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut d = Connection::open(dst).map_err(|e| e.to_string())?;
    let b = rusqlite::backup::Backup::new(&s, &mut d).map_err(|e| e.to_string())?;
    b.run_to_completion(256, Duration::from_millis(20), None)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Tiny blocking HTTP/1.1 POST over loopback (http:// only).
pub fn post_json(
    base: &str,
    path: &str,
    body: &Value,
    key: Option<&str>,
) -> Result<(u16, String), String> {
    let hostport = base
        .strip_prefix("http://")
        .ok_or("only http:// bases are supported")?
        .trim_end_matches('/');
    let mut s = TcpStream::connect(hostport).map_err(|e| format!("connect {hostport}: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let payload = body.to_string();
    let auth = key
        .map(|k| format!("Authorization: Bearer {k}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {hostport}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth}Connection: close\r\n\r\n{payload}",
        payload.len()
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").ok_or("bad response")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Ok((status, body.to_string()))
}

fn summary(status: u16, body: &str) -> Value {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let text = v
        .get("context")
        .or_else(|| v.get("results"))
        .and_then(Value::as_str)
        .unwrap_or("");
    json!({
        "status": status,
        "keys": v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "text_len": text.chars().count(),
        "text_sha": &sha256_hex(text.as_bytes())[..12],
        "memory_count": v.get("memory_count"),
        "total": v.get("total"),
        "strategy": v.get("strategy"),
    })
}

pub fn parity(
    a: &str,
    b: &str,
    queries: &Path,
    session: &str,
    key: Option<&str>,
) -> Result<bool, String> {
    let qs = std::fs::read_to_string(queries).map_err(|e| e.to_string())?;
    let (mut same, mut diff) = (0, 0);
    for (i, q) in qs
        .lines()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .enumerate()
    {
        for (path, body) in [
            ("/recall", json!({"query": q, "session_key": session})),
            ("/search/memories", json!({"query": q, "limit": 5})),
            ("/search/conversations", json!({"query": q, "limit": 5})),
        ] {
            let ra = post_json(a, path, &body, key)
                .map(|(s, t)| summary(s, &t))
                .unwrap_or_else(|e| json!({"error": e}));
            let rb = post_json(b, path, &body, key)
                .map(|(s, t)| summary(s, &t))
                .unwrap_or_else(|e| json!({"error": e}));
            let ok = ra == rb;
            if ok {
                same += 1;
            } else {
                diff += 1;
            }
            println!(
                "{}",
                json!({"q": i + 1, "path": path, "match": ok, "a": ra, "b": rb})
            );
        }
    }
    println!("{}", json!({"summary": {"match": same, "diff": diff}}));
    Ok(diff == 0)
}
