// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! A small loopback-only HTTP/1.x server for the read-only status API.
//!
//! std only: one thread per connection, at most [`MAX_CONNS`] at once (the
//! next gets 503), 5 s read timeout, 8 KiB request head limit, GET only,
//! `Connection: close`. Binding to anything but a loopback address is
//! refused: the status API shows task titles and agent ids, which stay on
//! the host.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_CONNS: usize = 16;
pub const MAX_HEAD: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
}

impl Request {
    pub fn param(&self, k: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn json(status: u16, v: &serde_json::Value) -> Response {
        Response {
            status,
            body: serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into()),
        }
    }
    pub fn error(status: u16, msg: &str) -> Response {
        Response::json(status, &serde_json::json!({"error": msg}))
    }
}

pub type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

/// Bind, refusing non-loopback addresses.
///
/// # Errors
/// Not loopback, unparsable, or the bind failed.
pub fn bind_loopback(addr: &str) -> Result<TcpListener, String> {
    let sa: SocketAddr = addr
        .parse()
        .map_err(|e| format!("bad address {addr:?}: {e}"))?;
    if !sa.ip().is_loopback() {
        return Err(format!(
            "refusing to bind {addr}: the Hermes status API is loopback-only"
        ));
    }
    TcpListener::bind(sa).map_err(|e| format!("bind {addr}: {e}"))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

fn write_response(stream: &mut TcpStream, r: &Response) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        r.status,
        reason(r.status),
        r.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(r.body.as_bytes());
    let _ = stream.flush();
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse the request line and drain the headers.
///
/// # Errors
/// A status code and message for a bad request.
pub fn read_request(stream: &TcpStream) -> Result<Request, (u16, &'static str)> {
    let mut reader = BufReader::new(stream.take(MAX_HEAD as u64));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| (400, "unreadable request"))?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err((400, "bad request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err((400, "HTTP/1.x only"));
    }
    let mut total = line.len();
    loop {
        let mut h = String::new();
        let n = reader
            .read_line(&mut h)
            .map_err(|_| (400, "unreadable headers"))?;
        total += n;
        if n == 0 {
            if total >= MAX_HEAD {
                return Err((431, "request head too large"));
            }
            break;
        }
        if h == "\r\n" || h == "\n" {
            break;
        }
    }
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let query = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    Ok(Request {
        method: method.to_string(),
        path: decode(path),
        query,
    })
}

/// Serve forever on `listener` (call from its own thread).
pub fn serve(listener: TcpListener, handler: Handler) {
    let live = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNS {
            live.fetch_sub(1, Ordering::SeqCst);
            write_response(&mut stream, &Response::error(503, "busy"));
            continue;
        }
        let handler = handler.clone();
        let live2 = live.clone();
        let spawned = std::thread::Builder::new()
            .name("hermes-http".into())
            .spawn(move || {
                let resp = match read_request(&stream) {
                    Ok(req) if req.method != "GET" && req.method != "HEAD" => {
                        Response::error(405, "read-only API: GET only")
                    }
                    Ok(req) => handler(&req),
                    Err((code, msg)) => Response::error(code, msg),
                };
                write_response(&mut stream, &resp);
                live2.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            live.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Minimal client for tests and the readiness self-check: GET, return
/// (status, body).
///
/// # Errors
/// Connect, write or read failures.
pub fn get(addr: &SocketAddr, path: &str) -> Result<(u16, String), String> {
    raw(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

/// Send raw request bytes; return (status, body).
///
/// # Errors
/// Connect, write or read failures.
pub fn raw(addr: &SocketAddr, request: &str) -> Result<(u16, String), String> {
    let mut s =
        TcpStream::connect_timeout(addr, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    s.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    let status = buf
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("no status line")?;
    let body = buf
        .split_once("\r\n\r\n")
        .map_or("", |(_, b)| b)
        .to_string();
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> SocketAddr {
        let l = bind_loopback("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        let h: Handler = Arc::new(|r: &Request| {
            Response::json(200, &serde_json::json!({"path": r.path, "n": r.param("n")}))
        });
        std::thread::spawn(move || serve(l, h));
        a
    }

    #[test]
    fn get_query_and_method_rules() {
        let a = start();
        let (s, b) = get(&a, "/v1/x?n=a%20b").unwrap();
        assert_eq!(s, 200);
        assert!(b.contains("\"a b\""), "{b}");
        let (s, _) = raw(&a, "POST /v1/x HTTP/1.1\r\nContent-Length: 0\r\n\r\n").unwrap();
        assert_eq!(s, 405);
        let (s, _) = raw(&a, "GARBAGE\r\n\r\n").unwrap();
        assert_eq!(s, 400);
    }

    #[test]
    fn non_loopback_bind_is_refused() {
        assert!(bind_loopback("0.0.0.0:0")
            .unwrap_err()
            .contains("loopback-only"));
        assert!(bind_loopback("[::]:0").is_err());
    }

    #[test]
    fn oversized_head_is_refused() {
        let a = start();
        let big = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "y".repeat(MAX_HEAD * 2));
        let (s, _) = raw(&a, &big).unwrap_or((431, String::new()));
        assert!(s == 431 || s == 400, "{s}");
    }
}
