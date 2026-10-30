// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Minimal HTTP/1.1 client for the openmind-memory gateway (`:8420`).
//!
//! Routes used (from `crates/openmind-memory/src/api.rs`):
//! `GET /health`, `POST /search/memories`, `POST /search/conversations`,
//! `POST /recall`, `POST /capture`.
//!
//! Safety: the URL must be plain `http://` on a loopback host
//! (`127.0.0.1`, `localhost`, `::1`), so the Bearer key never crosses a
//! network. The key is never logged or returned. Replies are capped at
//! [`MAX_REPLY_BYTES`].

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Largest reply body accepted.
pub const MAX_REPLY_BYTES: usize = 1024 * 1024;

/// Key names read from the key file (first non-empty wins), in the order
/// the memory server itself prefers.
pub const KEY_NAMES: [&str; 3] = [
    "OPENMIND_MEMORY_API_KEY",
    "MEMORY_TENCENTDB_GATEWAY_API_KEY",
    "TDAI_GATEWAY_API_KEY",
];

/// A loopback memory gateway.
#[derive(Clone)]
pub struct MemoryClient {
    addr: SocketAddr,
    host_header: String,
    key: Option<String>,
    timeout: Duration,
    label: &'static str,
}

impl std::fmt::Debug for MemoryClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryClient")
            .field("addr", &self.addr)
            .field("key", &self.key.as_ref().map(|_| "<set>"))
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl MemoryClient {
    /// Parse `http://<loopback-host>:<port>` (no path).
    ///
    /// # Errors
    /// Not `http://`, not loopback, bad port.
    pub fn new(url: &str, key: Option<String>, timeout: Duration) -> Result<Self, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or("memory url must start with http:// (loopback only)")?;
        let hostport = rest.trim_end_matches('/');
        if hostport.contains('/') || hostport.contains('@') {
            return Err("memory url must be http://host:port with no path".into());
        }
        let addrs: Vec<SocketAddr> = hostport
            .to_socket_addrs()
            .map_err(|e| format!("memory url: {e}"))?
            .collect();
        let Some(addr) = addrs.first().copied() else {
            return Err("memory url: no address".into());
        };
        if !addrs.iter().all(|a| a.ip().is_loopback()) {
            return Err("memory url must be a loopback address".into());
        }
        Ok(MemoryClient {
            addr,
            host_header: hostport.to_string(),
            key: key.filter(|k| !k.is_empty()),
            timeout,
            label: "memory",
        })
    }

    /// Same client, but errors name `label` (e.g. "nerve") instead of "memory".
    #[must_use]
    pub fn with_label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }

    /// `GET <path>` (path may carry a query string).
    ///
    /// # Errors
    /// Connection, HTTP or JSON error.
    pub fn get(&self, path: &str) -> Result<Value, String> {
        self.call("GET", path, None)
    }

    /// True when a Bearer key is configured.
    #[must_use]
    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// `GET /health`.
    ///
    /// # Errors
    /// Connection, HTTP or JSON error.
    pub fn health(&self) -> Result<Value, String> {
        self.call("GET", "/health", None)
    }

    /// `POST <path>` with a JSON body.
    ///
    /// # Errors
    /// Connection error, non-2xx status (message from the server's `error`
    /// field), or a non-JSON body.
    pub fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        self.call("POST", path, Some(body))
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let mut s = TcpStream::connect_timeout(&self.addr, self.timeout)
            .map_err(|e| format!("{} unreachable: {e}", self.label))?;
        let _ = s.set_read_timeout(Some(self.timeout));
        let _ = s.set_write_timeout(Some(self.timeout));
        let payload = body.map(Value::to_string).unwrap_or_default();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n",
            self.host_header
        );
        if body.is_some() {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
        }
        if let Some(k) = &self.key {
            req.push_str(&format!("Authorization: Bearer {k}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(&payload);
        s.write_all(req.as_bytes())
            .and_then(|()| s.flush())
            .map_err(|e| format!("{} write: {e}", self.label))?;
        let mut raw = Vec::new();
        s.take(MAX_REPLY_BYTES as u64 + 64 * 1024)
            .read_to_end(&mut raw)
            .map_err(|e| format!("{} read: {e}", self.label))?;
        let (status, body) = parse_response(&raw)?;
        let v: Value = serde_json::from_slice(&body)
            .map_err(|_| format!("{} {path}: HTTP {status}, reply is not JSON", self.label))?;
        if !(200..300).contains(&status) {
            let msg = v["error"].as_str().unwrap_or("error");
            return Err(format!("{} {path}: HTTP {status}: {msg}", self.label));
        }
        Ok(v)
    }
}

/// Split a raw HTTP/1.1 reply into status and body (handles
/// `Content-Length` and `chunked`).
///
/// # Errors
/// Malformed status line or headers, or a body over [`MAX_REPLY_BYTES`].
pub fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("memory reply: no header end")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut body = raw[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("memory reply: bad status line")?;
    let mut chunked = false;
    let mut len: Option<usize> = None;
    for l in lines {
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if k == "content-length" {
            len = v.parse().ok();
        }
    }
    if chunked {
        body = dechunk(&body)?;
    } else if let Some(n) = len {
        body.truncate(n);
    }
    if body.len() > MAX_REPLY_BYTES {
        return Err("memory reply too large".into());
    }
    Ok((status, body))
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = b
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("memory reply: bad chunk")?;
        let size_txt = String::from_utf8_lossy(&b[..eol]);
        let size = usize::from_str_radix(size_txt.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "memory reply: bad chunk size")?;
        b = &b[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size || out.len() + size > MAX_REPLY_BYTES {
            return Err("memory reply: chunk overflow".into());
        }
        out.extend_from_slice(&b[..size]);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
}

/// Read the Bearer key from an env-style file (`NAME=value` lines,
/// optional `export `, optional quotes). Only [`KEY_NAMES`] are looked at;
/// nothing else in the file is kept.
///
/// # Errors
/// The file cannot be read.
pub fn key_from_env_file(path: &std::path::Path) -> Result<Option<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("key file: {e}"))?;
    Ok(key_from_env_text(&text))
}

/// See [`key_from_env_file`].
#[must_use]
pub fn key_from_env_text(text: &str) -> Option<String> {
    let mut found: [Option<String>; 3] = [None, None, None];
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if let Some(i) = KEY_NAMES.iter().position(|n| *n == k) {
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if !v.is_empty() {
                found[i] = Some(v);
            }
        }
    }
    found.into_iter().flatten().next()
}

/// Read one named value (for example `API_SERVER_KEY`) from env-style
/// text. Same parsing as [`key_from_env_text`]; nothing else is kept.
#[must_use]
pub fn named_key_from_env_text(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == name {
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}
