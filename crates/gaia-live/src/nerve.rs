// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Client for the loopback `openmind-nerve serve` front: `POST /event` and
//! `GET /events?n=N`. This is the only place in gaia-live that sends a POST,
//! and it only ever talks to a loopback address.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const MAX_RESP: u64 = 1024 * 1024;

fn call(addr: SocketAddr, method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    if !addr.ip().is_loopback() {
        return Err(format!("nerve address {addr} is not loopback; refusing"));
    }
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))
        .map_err(|e| format!("nerve connect {addr}: {}", e.kind()))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: gaia-live/{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        env!("CARGO_PKG_VERSION"),
        body.len()
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("nerve write: {}", e.kind()))?;
    let mut buf = Vec::new();
    s.take(MAX_RESP)
        .read_to_end(&mut buf)
        .map_err(|e| format!("nerve read: {}", e.kind()))?;
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").ok_or("nerve: not HTTP")?;
    let code = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or("nerve: no status")?;
    Ok((code, body.to_string()))
}

/// Append one event. `data` must be a JSON object.
pub fn post(
    addr: SocketAddr,
    source: &str,
    kind: &str,
    severity: &str,
    data: Value,
) -> Result<(), String> {
    let body = serde_json::json!({
        "source": source,
        "kind": kind,
        "severity": severity,
        "data": data,
    })
    .to_string();
    match call(addr, "POST", "/event", &body)? {
        (200, _) => Ok(()),
        (c, b) => Err(format!("nerve POST /event: HTTP {c}: {}", b.trim())),
    }
}

/// The last `n` records (each `{ts,source,kind,severity,data}`).
pub fn tail(addr: SocketAddr, n: usize) -> Result<Vec<Value>, String> {
    match call(addr, "GET", &format!("/events?n={n}"), "")? {
        (200, b) => match serde_json::from_str::<Value>(&b) {
            Ok(Value::Array(a)) => Ok(a),
            _ => Err("nerve /events: not a JSON array".into()),
        },
        (c, b) => Err(format!("nerve GET /events: HTTP {c}: {}", b.trim())),
    }
}

/// `GET /health` is `{"status":"ok"}`.
pub fn healthy(addr: SocketAddr) -> bool {
    matches!(call(addr, "GET", "/health", ""), Ok((200, b)) if b.contains("\"status\":\"ok\""))
}

/// Convert a nerve record into GAIA's dependency-free [`gaia_core::Json`]
/// (floats are truncated to integers; GAIA has no float type).
pub fn to_gaia_json(v: &Value) -> gaia_core::Json {
    use gaia_core::Json;
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) => Json::Num(
            n.as_i64()
                .unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        ),
        Value::String(s) => Json::Str(s.clone()),
        Value::Array(a) => Json::Arr(a.iter().map(to_gaia_json).collect()),
        Value::Object(o) => Json::Obj(
            o.iter()
                .map(|(k, v)| (k.clone(), to_gaia_json(v)))
                .collect(),
        ),
    }
}
