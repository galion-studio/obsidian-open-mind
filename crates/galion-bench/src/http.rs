// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Minimal std-only HTTP/1.x message reading and writing, shared by the mock
//! server and the client. One message per connection (`Connection: close`),
//! `Content-Length` bodies only (no chunked encoding), bounded sizes.

use std::io::{self, BufRead, BufReader, Read, Write};

/// Largest accepted header block.
pub const MAX_HEAD: usize = 64 * 1024;
/// Largest accepted body.
pub const MAX_BODY: usize = 4 * 1024 * 1024;

/// A parsed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Upper-case method, e.g. `GET`.
    pub method: String,
    /// Path without the query string, e.g. `/api/1.0/accounts`.
    pub path: String,
    /// Raw query string without `?` (empty when absent).
    pub query: String,
    /// Headers with lower-cased names, in arrival order.
    pub headers: Vec<(String, String)>,
    /// Body bytes as UTF-8 (lossy).
    pub body: String,
}

impl Request {
    /// First header with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Read the start line + headers. Returns (start line, headers lower-cased).
fn read_head<R: BufRead>(r: &mut R) -> io::Result<(String, Vec<(String, String)>)> {
    let mut total = 0usize;
    let mut start = String::new();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        let n = r.by_ref().take(MAX_HEAD as u64).read_line(&mut line)?;
        if n == 0 {
            return Err(bad("connection closed before end of headers"));
        }
        total += n;
        if total > MAX_HEAD {
            return Err(bad("header block too large"));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if start.is_empty() {
            if line.is_empty() {
                continue; // tolerate leading blank lines (RFC 9112 2.2)
            }
            start = line.to_string();
            continue;
        }
        if line.is_empty() {
            return Ok((start, headers));
        }
        let (k, v) = line
            .split_once(':')
            .ok_or_else(|| bad("malformed header"))?;
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
}

fn read_body<R: Read>(r: &mut R, headers: &[(String, String)], to_eof: bool) -> io::Result<String> {
    if headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && !v.eq_ignore_ascii_case("identity"))
    {
        return Err(bad("chunked bodies are not supported; send Content-Length"));
    }
    let len = headers.iter().find(|(k, _)| k == "content-length");
    let mut buf = Vec::new();
    match len {
        Some((_, v)) => {
            let n: usize = v.parse().map_err(|_| bad("bad content-length"))?;
            if n > MAX_BODY {
                return Err(bad("body too large"));
            }
            buf.resize(n, 0);
            r.read_exact(&mut buf)?;
        }
        None if to_eof => {
            r.take(MAX_BODY as u64 + 1).read_to_end(&mut buf)?;
            if buf.len() > MAX_BODY {
                return Err(bad("body too large"));
            }
        }
        None => {}
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Read one request from a stream.
pub fn read_request<S: Read>(stream: S) -> io::Result<Request> {
    let mut r = BufReader::new(stream);
    let (start, headers) = read_head(&mut r)?;
    let mut parts = start.split_whitespace();
    let method = parts.next().ok_or_else(|| bad("empty request line"))?;
    let target = parts.next().ok_or_else(|| bad("missing request target"))?;
    let version = parts.next().ok_or_else(|| bad("missing HTTP version"))?;
    if !version.starts_with("HTTP/1.") {
        return Err(bad("only HTTP/1.x is supported"));
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.to_string(), String::new()),
    };
    let body = read_body(&mut r, &headers, false)?;
    Ok(Request {
        method: method.to_ascii_uppercase(),
        path,
        query,
        headers,
        body,
    })
}

/// `(status, lower-cased headers, body)` as read by [`read_response`].
pub type RawResponse = (u16, Vec<(String, String)>, String);

/// Read one response (status, headers, body). Body is read by Content-Length,
/// or to EOF when absent.
pub fn read_response<S: Read>(stream: S) -> io::Result<RawResponse> {
    let mut r = BufReader::new(stream);
    let (start, headers) = read_head(&mut r)?;
    let mut parts = start.split_whitespace();
    let _version = parts.next().ok_or_else(|| bad("empty status line"))?;
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad("bad status code"))?;
    let body = read_body(&mut r, &headers, true)?;
    Ok((status, headers, body))
}

/// Canonical reason phrase for common statuses.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}

/// Write a full response with `Content-Length` and `Connection: close`.
/// Header names `content-length` and `connection` in `headers` are ignored.
pub fn write_response<W: Write>(
    mut w: W,
    status: u16,
    headers: &[(String, String)],
    body: &str,
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (k, v) in headers {
        let lk = k.to_ascii_lowercase();
        if lk == "content-length" || lk == "connection" {
            continue;
        }
        if k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            continue; // never let a fixture inject header lines
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    w.write_all(head.as_bytes())?;
    w.write_all(body.as_bytes())?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_with_query_and_body() {
        let raw = b"POST /a/b?x=1&y=2 HTTP/1.1\r\nHost: h\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}";
        let r = read_request(&raw[..]).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/a/b");
        assert_eq!(r.query, "x=1&y=2");
        assert_eq!(r.header("CONTENT-TYPE"), Some("application/json"));
        assert_eq!(r.body, "{\"a\":1}");
    }

    #[test]
    fn rejects_chunked_and_garbage() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n";
        assert!(read_request(&raw[..]).is_err());
        assert!(read_request(&b"GET /\r\n\r\n"[..]).is_err());
        assert!(read_request(&b"GET / HTTP/1.1\r\nnocolon\r\n\r\n"[..]).is_err());
    }

    #[test]
    fn response_roundtrip_and_no_header_injection() {
        let mut out = Vec::new();
        let hs = vec![
            ("X-A".to_string(), "1".to_string()),
            ("X-Bad".to_string(), "a\r\nSet-Cookie: x".to_string()),
            ("Content-Length".to_string(), "999".to_string()),
        ];
        write_response(&mut out, 403, &hs, "nope").unwrap();
        let text = String::from_utf8(out.clone()).unwrap();
        assert!(!text.contains("Set-Cookie"));
        assert!(!text.contains("999"));
        let (s, h, b) = read_response(&out[..]).unwrap();
        assert_eq!(s, 403);
        assert!(h.iter().any(|(k, v)| k == "x-a" && v == "1"));
        assert_eq!(b, "nope");
    }
}
