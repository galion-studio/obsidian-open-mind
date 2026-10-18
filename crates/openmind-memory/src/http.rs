// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Minimal std-only HTTP/1.1 server: one short-lived thread per connection,
//! `Connection: close`, Content-Length bodies. Enough for a loopback sidecar
//! whose only client is Hermes' urllib, and it keeps RSS in the low MB.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

pub struct Response {
    pub status: u16,
    pub body: String,
}

pub enum ReadError {
    TooLarge(usize),
    Bad(String),
    Io,
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

pub fn read_request(stream: &TcpStream, max_body: usize) -> Result<Request, ReadError> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    if r.read_line(&mut line).map_err(|_| ReadError::Io)? == 0 {
        return Err(ReadError::Io);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().unwrap_or("/").to_string();
    if method.is_empty() {
        return Err(ReadError::Bad("empty request line".into()));
    }
    let mut headers = Vec::new();
    let mut header_bytes = 0usize;
    loop {
        let mut h = String::new();
        let n = r.read_line(&mut h).map_err(|_| ReadError::Io)?;
        header_bytes += n;
        if n == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if header_bytes > 64 * 1024 {
            return Err(ReadError::Bad("headers too large".into()));
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    if get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
        return Err(ReadError::Bad(
            "chunked bodies are not supported; send Content-Length".into(),
        ));
    }
    let len: usize = get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > max_body {
        return Err(ReadError::TooLarge(max_body));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|_| ReadError::Io)?;
    // Path without query string; trailing-slash handling is per route.
    let path = target.split('?').next().unwrap_or("/").to_string();
    let path = match path
        .strip_prefix("http://")
        .or_else(|| path.strip_prefix("https://"))
    {
        Some(rest) => rest
            .find('/')
            .map(|i| rest[i..].to_string())
            .unwrap_or_else(|| "/".into()),
        None => path,
    };
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

pub fn write_response(mut stream: &TcpStream, resp: &Response) {
    let head = if resp.status == 204 {
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_string()
    } else {
        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            resp.status,
            reason(resp.status),
            resp.body.len()
        )
    };
    let _ = stream.write_all(head.as_bytes());
    if resp.status != 204 {
        let _ = stream.write_all(resp.body.as_bytes());
    }
    let _ = stream.flush();
}

/// Serve forever. `handler` must be cheap to share across threads.
pub fn serve<H>(listener: TcpListener, max_body: usize, max_conns: usize, handler: Arc<H>)
where
    H: Fn(Request) -> Response + Send + Sync + 'static,
{
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        if active.load(Ordering::SeqCst) >= max_conns {
            write_response(
                &stream,
                &Response {
                    status: 503,
                    body: r#"{"error":"Too many connections"}"#.into(),
                },
            );
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (h, a) = (handler.clone(), active.clone());
        let spawned = std::thread::Builder::new().stack_size(512 * 1024).spawn(move || {
            let resp = match read_request(&stream, max_body) {
                Ok(req) => h(req),
                Err(ReadError::TooLarge(m)) => Response {
                    status: 413,
                    body: serde_json::json!({ "error": format!("Request body exceeds {m} bytes") }).to_string(),
                },
                Err(ReadError::Bad(m)) => Response { status: 400, body: serde_json::json!({ "error": m }).to_string() },
                Err(ReadError::Io) => {
                    a.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
            };
            write_response(&stream, &resp);
            a.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
