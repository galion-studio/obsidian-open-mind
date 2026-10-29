// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Client for the live openmind-memory HTTP API (loopback only).
//!
//! Routes: `POST /recall`, `/capture`, `/search/memories`,
//! `/search/conversations`, `/recent`. The Bearer key is sent when one is
//! configured and is never printed or put into a tool result.

use galion_common::http;
use serde_json::{json, Value};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

/// Max bytes accepted from the memory server per reply.
pub const MAX_REPLY: usize = 4 << 20;

#[derive(Clone)]
pub struct Memory {
    addr: SocketAddr,
    host_header: String,
    key: Option<String>,
    timeout: Duration,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory")
            .field("addr", &self.addr)
            .field("key", &self.key.as_ref().map(|_| "<set>"))
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl Memory {
    /// `url` must be `http://<loopback host>:<port>` (`127.0.0.1`,
    /// `localhost` or `[::1]`), optionally with a trailing `/`.
    pub fn new(url: &str, key: Option<String>, timeout: Duration) -> Result<Memory, String> {
        let hostport = url
            .trim()
            .strip_prefix("http://")
            .ok_or("memory URL must start with http:// (loopback only)")?
            .trim_end_matches('/');
        if hostport.contains('/') || hostport.contains('@') || hostport.is_empty() {
            return Err("memory URL must be http://host:port with no path".into());
        }
        let host = match hostport.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => h,
            _ => return Err("memory URL needs an explicit port".into()),
        };
        if !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
            return Err(format!(
                "memory URL host {host} is not loopback (127.0.0.1, localhost, [::1])"
            ));
        }
        let addr = hostport
            .to_socket_addrs()
            .map_err(|e| format!("memory URL: {e}"))?
            .find(|a| a.ip().is_loopback())
            .ok_or("memory URL does not resolve to a loopback address")?;
        Ok(Memory {
            addr,
            host_header: hostport.to_string(),
            key: key.filter(|k| !k.trim().is_empty()),
            timeout,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// POST JSON, return (status, parsed body). Errors never contain the key.
    pub fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), String> {
        let payload = body.to_string();
        let mut headers = vec![
            ("Host".to_string(), self.host_header.clone()),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Length".to_string(), payload.len().to_string()),
            ("Connection".to_string(), "close".to_string()),
        ];
        if let Some(k) = &self.key {
            headers.push(("Authorization".to_string(), format!("Bearer {}", k.trim())));
        }
        let req = http::ClientRequest {
            method: "POST",
            path,
            minor: 1,
            headers,
            body: payload.as_bytes(),
        };
        match http::send(self.addr, &req, self.timeout, MAX_REPLY) {
            Ok(r) => {
                let v =
                    serde_json::from_slice(&r.body).unwrap_or_else(|_| json!({"raw": r.text()}));
                Ok((r.status, v))
            }
            Err(http::ClientError::Connect(e)) => {
                Err(format!("openmind-memory at {} unreachable: {e}", self.addr))
            }
            Err(e) => Err(format!("openmind-memory {path}: {e}")),
        }
    }
}
