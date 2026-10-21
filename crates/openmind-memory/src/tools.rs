// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Operator subcommands for the B60 shadow run (no server involved):
//! - `snapshot <src.db> <dst.db>`: consistent copy via the SQLite online
//!   backup API (source opened read-only; safe while the gateway writes).
//! - `parity <base_a> <base_b> <queries.txt> [session_key]`: POST the same
//!   read-only requests (/recall, /search/memories, /search/conversations)
//!   to both servers and print match/diff with lengths and hash prefixes
//!   only, never memory content. Bearer from OPENMIND_MEMORY_API_KEY or
//!   TDAI_GATEWAY_API_KEY (never printed).

use crate::hash::sha256_hex;
use galion_common::http;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::net::ToSocketAddrs;
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

/// Tiny blocking HTTP/1.1 POST over loopback (http:// only), through the
/// galion-common client (60 s for the whole exchange, 16 MiB reply cap).
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
    let addr = hostport
        .to_socket_addrs()
        .map_err(|e| format!("connect {hostport}: {e}"))?
        .next()
        .ok_or_else(|| format!("connect {hostport}: no address"))?;
    let payload = body.to_string();
    let mut headers = vec![
        ("Host".to_string(), hostport.to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Content-Length".to_string(), payload.len().to_string()),
    ];
    if let Some(k) = key {
        headers.push(("Authorization".to_string(), format!("Bearer {k}")));
    }
    headers.push(("Connection".to_string(), "close".to_string()));
    let req = http::ClientRequest {
        method: "POST",
        path,
        minor: 1,
        headers,
        body: payload.as_bytes(),
    };
    match http::send(addr, &req, Duration::from_secs(60), 16 << 20) {
        Ok(r) => Ok((r.status, r.text())),
        Err(http::ClientError::Connect(e)) => Err(format!("connect {hostport}: {e}")),
        Err(http::ClientError::BadStatus(_)) => Err("bad response".into()),
        Err(e) => Err(e.to_string()),
    }
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
