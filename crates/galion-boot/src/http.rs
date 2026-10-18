// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tiny HTTP/1.0 POST for `fire-loops` (loop-daemon.sh died when curl went
//! missing on 2026-10-10; this needs no curl).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub fn post_json(
    addr: SocketAddr,
    path: &str,
    body: &str,
    timeout: Duration,
) -> Result<u16, String> {
    let mut s = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let req = format!(
        "POST {path} HTTP/1.0\r\nHost: {addr}\r\ncontent-type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut buf = [0u8; 256];
    let n = s.read(&mut buf).map_err(|e| format!("read: {e}"))?;
    let head = String::from_utf8_lossy(&buf[..n]);
    head.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| "bad status line".into())
}
