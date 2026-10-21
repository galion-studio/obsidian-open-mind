// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Minimal, strict HTTP/1.1 request-head parsing and response writing.

use std::io::{self, Read, Write};

/// Largest request head accepted (request line + headers).
pub const MAX_HEAD: usize = 16 * 1024;
/// Most header lines accepted.
pub const MAX_HEADERS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    pub method: String,
    pub target: String,
    /// `true` for HTTP/1.1, `false` for HTTP/1.0.
    pub http11: bool,
    pub headers: Vec<(String, String)>,
}

impl RequestHead {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether the client asked to keep the connection open.
    pub fn wants_keep_alive(&self) -> bool {
        let conn = self.header("connection").unwrap_or("").to_ascii_lowercase();
        if self.http11 {
            !conn.split(',').any(|t| t.trim() == "close")
        } else {
            conn.split(',').any(|t| t.trim() == "keep-alive")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadError {
    /// Peer closed before sending anything.
    Closed,
    TooLarge,
    Malformed,
    /// Timeout or socket error.
    Io,
}

fn is_tchar(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}

/// Find the end of the head (`\r\n\r\n`) in `buf`; returns the index after it.
pub fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// Parse a complete head (including the final blank line).
pub fn parse_head(head: &[u8]) -> Result<RequestHead, HeadError> {
    let text = std::str::from_utf8(head).map_err(|_| HeadError::Malformed)?;
    let text = text.strip_suffix("\r\n\r\n").ok_or(HeadError::Malformed)?;
    let mut lines = text.split("\r\n");
    let line = lines.next().ok_or(HeadError::Malformed)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(HeadError::Malformed);
    };
    if method.is_empty() || !method.bytes().all(is_tchar) || target.is_empty() {
        return Err(HeadError::Malformed);
    }
    if target.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(HeadError::Malformed);
    }
    let http11 = match version {
        "HTTP/1.1" => true,
        "HTTP/1.0" => false,
        _ => return Err(HeadError::Malformed),
    };
    let mut headers = Vec::new();
    for l in lines {
        if headers.len() >= MAX_HEADERS {
            return Err(HeadError::TooLarge);
        }
        // No obs-fold, no whitespace before the colon.
        let (name, value) = l.split_once(':').ok_or(HeadError::Malformed)?;
        if name.is_empty() || !name.bytes().all(is_tchar) {
            return Err(HeadError::Malformed);
        }
        let value = value.trim_matches(|c| c == ' ' || c == '\t');
        if value.bytes().any(|b| (b < b' ' && b != b'\t') || b == 0x7f) {
            return Err(HeadError::Malformed);
        }
        headers.push((name.to_string(), value.to_string()));
    }
    let head = RequestHead {
        method: method.to_string(),
        target: target.to_string(),
        http11,
        headers,
    };
    if head.http11 && head.header("host").is_none() {
        return Err(HeadError::Malformed);
    }
    // Duplicate Content-Length / Host are request-smuggling material.
    for n in ["content-length", "host", "transfer-encoding"] {
        if head
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(n))
            .count()
            > 1
        {
            return Err(HeadError::Malformed);
        }
    }
    Ok(head)
}

/// Read one request head from `r`. `carry` holds bytes already read past the
/// previous request; on success it holds the bytes after this head (body or
/// pipelined data).
pub fn read_head<R: Read>(r: &mut R, carry: &mut Vec<u8>) -> Result<RequestHead, HeadError> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = head_end(carry) {
            let rest = carry.split_off(end);
            let head = std::mem::replace(carry, rest);
            return parse_head(&head);
        }
        if carry.len() > MAX_HEAD {
            return Err(HeadError::TooLarge);
        }
        let n = match r.read(&mut chunk) {
            Ok(n) => n,
            Err(_) => {
                return Err(if carry.is_empty() {
                    HeadError::Closed
                } else {
                    HeadError::Io
                })
            }
        };
        if n == 0 {
            return Err(if carry.is_empty() {
                HeadError::Closed
            } else {
                HeadError::Malformed
            });
        }
        carry.extend_from_slice(&chunk[..n]);
    }
}

pub fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

/// Security headers on every response this server generates itself.
pub const SECURITY_HEADERS: &str = "X-Content-Type-Options: nosniff\r\n\
Referrer-Policy: strict-origin-when-cross-origin\r\n";

/// Write a status line plus headers. `extra` is pre-formatted `Name: v\r\n`
/// lines. Body length is always given (no chunked output).
pub fn write_head<W: Write>(
    w: &mut W,
    code: u16,
    extra: &str,
    len: u64,
    keep_alive: bool,
) -> io::Result<()> {
    let conn = if keep_alive { "keep-alive" } else { "close" };
    let head = format!(
        "HTTP/1.1 {code} {}\r\n{extra}{SECURITY_HEADERS}Content-Length: {len}\r\nConnection: {conn}\r\n\r\n",
        reason(code)
    );
    w.write_all(head.as_bytes())
}

/// A small generated response (errors, redirects).
pub fn write_simple<W: Write>(
    w: &mut W,
    code: u16,
    extra: &str,
    body: &str,
    head_only: bool,
    keep_alive: bool,
) -> io::Result<()> {
    let extra =
        format!("Content-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\n{extra}");
    write_head(w, code, &extra, body.len() as u64, keep_alive)?;
    if !head_only {
        w.write_all(body.as_bytes())?;
    }
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_get() {
        let h = parse_head(b"GET /a?b HTTP/1.1\r\nHost: x\r\nAccept: */*\r\n\r\n").unwrap();
        assert_eq!(h.method, "GET");
        assert_eq!(h.target, "/a?b");
        assert!(h.http11);
        assert_eq!(h.header("HOST"), Some("x"));
        assert!(h.wants_keep_alive());
    }

    #[test]
    fn connection_semantics() {
        let h = parse_head(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        assert!(!h.wants_keep_alive());
        let h = parse_head(b"GET / HTTP/1.0\r\n\r\n").unwrap();
        assert!(!h.wants_keep_alive());
        let h = parse_head(b"GET / HTTP/1.0\r\nConnection: keep-alive\r\n\r\n").unwrap();
        assert!(h.wants_keep_alive());
    }

    #[test]
    fn rejects_malformed() {
        for raw in [
            &b"GET / HTTP/1.1\r\n\r\n"[..], // no Host on 1.1
            b"GET  / HTTP/1.1\r\nHost: x\r\n\r\n",
            b"GET / HTTP/2\r\nHost: x\r\n\r\n",
            b"G(T / HTTP/1.1\r\nHost: x\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: x\r\n folded\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost : x\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: x\r\nHost: y\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
            b"GET /a\x01 HTTP/1.1\r\nHost: x\r\n\r\n",
        ] {
            assert!(
                parse_head(raw).is_err(),
                "{:?}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    #[test]
    fn read_head_keeps_carry() {
        let mut src: &[u8] = b"POST /api/x HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabcGET";
        let mut carry = Vec::new();
        let h = read_head(&mut src, &mut carry).unwrap();
        assert_eq!(h.method, "POST");
        assert_eq!(carry, b"abcGET");
    }

    #[test]
    fn read_head_limits() {
        let big = vec![b'a'; MAX_HEAD + 10];
        let mut src: &[u8] = &big;
        let mut carry = Vec::new();
        assert_eq!(read_head(&mut src, &mut carry), Err(HeadError::TooLarge));
        let mut empty: &[u8] = b"";
        let mut carry = Vec::new();
        assert_eq!(read_head(&mut empty, &mut carry), Err(HeadError::Closed));
    }
}
