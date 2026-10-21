// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Minimal std-only HTTP/1.1 (one request per connection, `Connection:
//! close`). Enough for a loopback fixture server and its client; not a
//! general-purpose HTTP stack.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Largest request or response head + body we accept.
pub const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    /// Value of `key` in the query string (no percent-decoding; the bench
    /// only passes plain ASCII values).
    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then_some(v)
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, v: &serde_json::Value) -> Self {
        Response {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: v.to_string().into_bytes(),
        }
    }

    pub fn raw(status: u16, body: &str) -> Self {
        Response {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// Split a raw message into (head lines, body start). `None` until the
/// blank line arrives.
fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_headers(lines: &[&str]) -> Vec<(String, String)> {
    lines
        .iter()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Read one request from a stream (honours `Content-Length`).
pub fn read_request(stream: &mut TcpStream) -> io::Result<Request> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let body_start = loop {
        if let Some(e) = head_end(&buf) {
            break e;
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(bad("eof before head"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_BYTES {
            return Err(bad("head too large"));
        }
    };
    let head = String::from_utf8_lossy(&buf[..body_start - 4]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    if method.is_empty() || !target.starts_with('/') {
        return Err(bad("bad request line"));
    }
    let rest: Vec<&str> = lines.collect();
    let headers = parse_headers(&rest);
    let len: usize = header(&headers, "content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_BYTES {
        return Err(bad("body too large"));
    }
    let mut body = buf[body_start..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

pub fn write_response(stream: &mut TcpStream, r: &Response) -> io::Result<()> {
    let mut out = format!("HTTP/1.1 {} {}\r\n", r.status, reason(r.status));
    for (k, v) in &r.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        r.body.len()
    ));
    stream.write_all(out.as_bytes())?;
    stream.write_all(&r.body)?;
    stream.flush()
}

/// A parsed `http://host:port` base URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

/// Parse `scheme://host[:port][/]`. Paths on the base are rejected.
pub fn parse_url(s: &str) -> Result<Url, String> {
    let (scheme, rest) = s
        .split_once("://")
        .ok_or_else(|| format!("not a URL: {s}"))?;
    let rest = rest.trim_end_matches('/');
    if rest.contains('/') || rest.contains('@') {
        return Err(format!("base URL must be scheme://host:port only: {s}"));
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    let (host, port) = if let Some(h) = rest.strip_prefix('[') {
        let (h, p) = h.split_once(']').ok_or("bad IPv6 literal")?;
        let port = match p.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| "bad port")?,
            None => default_port,
        };
        (h.to_string(), port)
    } else {
        match rest.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().map_err(|_| "bad port")?),
            None => (rest.to_string(), default_port),
        }
    };
    if host.is_empty() {
        return Err("empty host".into());
    }
    Ok(Url {
        scheme: scheme.to_ascii_lowercase(),
        host: host.to_ascii_lowercase(),
        port,
    })
}

/// Send one request and read the whole response.
pub fn send(
    url: &Url,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> io::Result<Response> {
    let addr = (url.host.as_str(), url.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| bad("no address"))?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let host = if url.host.contains(':') {
        format!("[{}]:{}", url.host, url.port)
    } else {
        format!("{}:{}", url.host, url.port)
    };
    let mut out = format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    let body = body.unwrap_or_default();
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    s.write_all(out.as_bytes())?;
    s.write_all(body)?;
    s.flush()?;
    let mut buf = Vec::new();
    s.take(MAX_BYTES as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > MAX_BYTES {
        return Err(bad("response too large"));
    }
    let end = head_end(&buf).ok_or_else(|| bad("no response head"))?;
    let head = String::from_utf8_lossy(&buf[..end - 4]).to_string();
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| bad("bad status line"))?;
    let rest: Vec<&str> = lines.collect();
    let headers = parse_headers(&rest);
    let mut body = buf[end..].to_vec();
    if let Some(len) = header(&headers, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.truncate(len);
    }
    Ok(Response {
        status,
        headers,
        body,
    })
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn urls() {
        let u = parse_url("http://127.0.0.1:19510").unwrap();
        assert_eq!((u.host.as_str(), u.port), ("127.0.0.1", 19510));
        let u = parse_url("https://api.revolut.com/").unwrap();
        assert_eq!((u.scheme.as_str(), u.port), ("https", 443));
        let u = parse_url("http://[::1]:8080").unwrap();
        assert_eq!(u.host, "::1");
        assert!(parse_url("http://127.0.0.1:1/api").is_err());
        assert!(parse_url("http://user@127.0.0.1:1").is_err());
        assert!(parse_url("nope").is_err());
    }
}
