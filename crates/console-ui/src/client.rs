// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Loopback HTTP/1.1 client for the console API upstream (and the desk
//! pulse). One connection per call, `Connection: close`, bounded in time and
//! size. Handles `Content-Length`, `chunked` and read-to-close bodies.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// Whole-call budget (the TSX used 1.5-4 s fetch timeouts).
pub const CALL_BUDGET: Duration = Duration::from_secs(4);
/// Largest response accepted (the biggest page payload today is ~4 KB).
pub const MAX_RESPONSE: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Every value of a repeatable header (e.g. `Set-Cookie`).
    pub fn header_all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// Headers the console UI passes through to the upstream so the upstream's
/// own phone gate sees the browser's identity (defence in depth).
#[derive(Debug, Clone, Default)]
pub struct Forward {
    pub host: String,
    pub cookie: Option<String>,
    pub forwarded_for: Option<String>,
    pub forwarded_proto: Option<String>,
}

fn clean(v: &str) -> String {
    v.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

/// Build the request bytes (exposed for tests).
pub fn request_bytes(
    method: &str,
    path: &str,
    fwd: &Forward,
    body: Option<(&str, &[u8])>,
) -> Vec<u8> {
    request_bytes_with(method, path, fwd, &[], body)
}

/// [`request_bytes`] plus extra request headers (CR/LF stripped), e.g. the
/// Guardian PIN header `X-Galion-Pin`.
pub fn request_bytes_with(
    method: &str,
    path: &str,
    fwd: &Forward,
    extra: &[(&str, &str)],
    body: Option<(&str, &[u8])>,
) -> Vec<u8> {
    let mut h = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\nUser-Agent: galion-console-ui\r\n",
        clean(path),
        clean(&fwd.host)
    );
    if let Some(c) = &fwd.cookie {
        h.push_str(&format!("Cookie: {}\r\n", clean(c)));
    }
    if let Some(x) = &fwd.forwarded_for {
        h.push_str(&format!("X-Forwarded-For: {}\r\n", clean(x)));
    }
    if let Some(p) = &fwd.forwarded_proto {
        h.push_str(&format!("X-Forwarded-Proto: {}\r\n", clean(p)));
    }
    for (k, v) in extra {
        h.push_str(&format!("{}: {}\r\n", clean(k), clean(v)));
    }
    let mut out;
    match body {
        Some((ctype, b)) => {
            h.push_str(&format!(
                "Content-Type: {}\r\nContent-Length: {}\r\n\r\n",
                clean(ctype),
                b.len()
            ));
            out = h.into_bytes();
            out.extend_from_slice(b);
        }
        None => {
            h.push_str("\r\n");
            out = h.into_bytes();
        }
    }
    out
}

fn too_slow() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "upstream call budget exceeded")
}

/// Perform one call.
pub fn call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    fwd: &Forward,
    body: Option<(&str, &[u8])>,
) -> io::Result<Response> {
    call_with(addr, method, path, fwd, &[], body)
}

/// [`call`] with extra request headers.
pub fn call_with(
    addr: SocketAddr,
    method: &str,
    path: &str,
    fwd: &Forward,
    extra: &[(&str, &str)],
    body: Option<(&str, &[u8])>,
) -> io::Result<Response> {
    let start = Instant::now();
    let mut s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    s.set_write_timeout(Some(CALL_BUDGET))?;
    s.write_all(&request_bytes_with(method, path, fwd, extra, body))?;
    s.flush()?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let left = CALL_BUDGET
            .checked_sub(start.elapsed())
            .filter(|d| !d.is_zero())
            .ok_or_else(too_slow)?;
        s.set_read_timeout(Some(left))?;
        let n = match s.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(too_slow())
            }
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
        if raw.len() > MAX_RESPONSE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response too large",
            ));
        }
        // Stop early once a Content-Length body is complete (keeps working
        // if the upstream ignores Connection: close).
        if let Some(done) = complete_len(&raw) {
            raw.truncate(done);
            break;
        }
    }
    parse_response(&raw, method == "HEAD")
}

fn head_end(b: &[u8]) -> Option<usize> {
    b.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn complete_len(raw: &[u8]) -> Option<usize> {
    let end = head_end(raw)?;
    let head = std::str::from_utf8(&raw[..end]).ok()?;
    if head
        .lines()
        .any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:"))
    {
        return None;
    }
    let len: usize = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| v.trim().parse().ok())?
    })?;
    (raw.len() >= end + len).then_some(end + len)
}

fn bad(m: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m)
}

/// Parse a complete response (exposed for tests).
pub fn parse_response(raw: &[u8], head_only: bool) -> io::Result<Response> {
    let end = head_end(raw).ok_or_else(|| bad("no response head"))?;
    let head = std::str::from_utf8(&raw[..end - 4]).map_err(|_| bad("head not utf8"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or_else(|| bad("empty head"))?;
    let mut parts = status_line.splitn(3, ' ');
    let ver = parts.next().unwrap_or("");
    if !ver.starts_with("HTTP/1.") {
        return Err(bad("not HTTP/1.x"));
    }
    let status: u16 = parts
        .next()
        .and_then(|c| c.parse().ok())
        .filter(|c| (100..600).contains(c))
        .ok_or_else(|| bad("bad status"))?;
    let mut headers = Vec::new();
    for l in lines {
        let (k, v) = l.split_once(':').ok_or_else(|| bad("bad header"))?;
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    let rest = &raw[end..];
    let resp_hdr = |n: &str| {
        headers
            .iter()
            .find(|(k, _): &&(String, String)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.as_str())
    };
    let body = if head_only || status == 204 || status == 304 {
        Vec::new()
    } else if resp_hdr("transfer-encoding")
        .is_some_and(|t| t.to_ascii_lowercase().contains("chunked"))
    {
        dechunk(rest)?
    } else if let Some(len) = resp_hdr("content-length") {
        let n: usize = len.parse().map_err(|_| bad("bad content-length"))?;
        if rest.len() < n {
            return Err(bad("short body"));
        }
        rest[..n].to_vec()
    } else {
        rest.to_vec()
    };
    Ok(Response {
        status,
        headers,
        body,
    })
}

/// Decode a `chunked` body (trailers ignored).
pub fn dechunk(mut b: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = b
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| bad("chunk size line"))?;
        let line = std::str::from_utf8(&b[..nl]).map_err(|_| bad("chunk size"))?;
        let size_txt = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_txt, 16).map_err(|_| bad("chunk size"))?;
        b = &b[nl + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size + 2 {
            return Err(bad("short chunk"));
        }
        out.extend_from_slice(&b[..size]);
        if &b[size..size + 2] != b"\r\n" {
            return Err(bad("chunk terminator"));
        }
        b = &b[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_length_chunked_and_close() {
        let r = parse_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhiEXTRA",
            false,
        )
        .unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"hi"[..]));
        let r = parse_response(
            b"HTTP/1.1 401 Unauthorized\r\nTransfer-Encoding: chunked\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n",
            false,
        )
        .unwrap();
        assert_eq!(r.status, 401);
        assert_eq!(r.body, b"abcde");
        assert_eq!(r.header_all("set-cookie"), vec!["a=1", "b=2"]);
        let r = parse_response(b"HTTP/1.0 200 OK\r\n\r\nto-close", false).unwrap();
        assert_eq!(r.body, b"to-close");
        assert!(parse_response(b"HTTP/2 200\r\n\r\n", false).is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nab", false).is_err());
        assert!(dechunk(b"zz\r\n").is_err());
    }

    #[test]
    fn request_has_no_header_injection() {
        let fwd = Forward {
            host: "console.galion.app\r\nX-Evil: 1".into(),
            cookie: Some("galion_admin=t".into()),
            forwarded_for: None,
            forwarded_proto: Some("https".into()),
        };
        let r = String::from_utf8(request_bytes(
            "POST",
            "/api/v1/command",
            &fwd,
            Some(("application/json", b"{}")),
        ))
        .unwrap();
        assert!(
            r.starts_with("POST /api/v1/command HTTP/1.1\r\nHost: console.galion.appX-Evil: 1\r\n")
        );
        assert!(r.contains("Cookie: galion_admin=t\r\n"));
        assert!(r.contains("Content-Length: 2\r\n\r\n{}"));
        assert_eq!(r.matches("\r\n").count(), 10);
    }
}
