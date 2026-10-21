// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! A deliberately small HTTP/1.1 layer on `std::net`: one request per
//! connection (`Connection: close`), `Content-Length` bodies only, hard
//! limits on header and body size. Enough for a loopback control API and
//! its CLI client; not a general web server.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const MAX_HEADER: usize = 16 * 1024;
pub const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Path without the query string.
    pub path: String,
    pub query: String,
    /// Raw `Authorization` header value, if any. Never log it.
    pub authorization: Option<String>,
    pub body: Vec<u8>,
}

impl Request {
    /// Value of `key` in the query string (no percent-decoding; the API only
    /// uses simple names and numbers).
    pub fn param(&self, key: &str) -> Option<&str> {
        self.query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Read one request from `r`.
pub fn read_request<R: Read>(r: R) -> io::Result<Request> {
    let mut br = BufReader::new(r.take((MAX_HEADER + MAX_BODY) as u64));
    let mut line = String::new();
    let mut header_bytes = 0usize;
    br.read_line(&mut line)?;
    header_bytes += line.len();
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| bad("empty request"))?
        .to_string();
    let target = parts.next().ok_or_else(|| bad("no target"))?.to_string();
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(bad("not HTTP/1.x"));
    }
    let mut len = 0usize;
    let mut authorization = None;
    loop {
        let mut h = String::new();
        let n = br.read_line(&mut h)?;
        if n == 0 {
            return Err(bad("headers cut short"));
        }
        header_bytes += n;
        if header_bytes > MAX_HEADER {
            return Err(bad("headers too large"));
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            if k == "content-length" {
                len = v.trim().parse().map_err(|_| bad("bad content-length"))?;
                if len > MAX_BODY {
                    return Err(bad("body too large"));
                }
            } else if k == "authorization" {
                if authorization.is_some() {
                    return Err(bad("duplicate authorization header"));
                }
                authorization = Some(v.trim().to_string());
            } else if k == "transfer-encoding" {
                return Err(bad("transfer-encoding not supported"));
            }
        }
    }
    let mut body = vec![0u8; len];
    br.read_exact(&mut body)?;
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    Ok(Request {
        method,
        path,
        query,
        authorization,
        body,
    })
}

pub fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// Write a JSON response and close.
pub fn write_response<W: Write>(mut w: W, code: u16, body: &str) -> io::Result<()> {
    let extra = if code == 401 {
        "WWW-Authenticate: Bearer realm=\"poseidon\"\r\n"
    } else {
        ""
    };
    let head = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        reason(code),
        body.len()
    );
    w.write_all(head.as_bytes())?;
    w.write_all(body.as_bytes())?;
    w.flush()
}

/// A blocking client call. Returns (status, body).
pub fn request(
    addr: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> io::Result<(u16, String)> {
    request_auth(addr, method, path, body, None, timeout)
}

/// Like [`request`], optionally sending `Authorization: Bearer <token>`.
/// The token is written to the socket only, never logged.
pub fn request_auth(
    addr: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    bearer: Option<&[u8]>,
    timeout: Duration,
) -> io::Result<(u16, String)> {
    let sa = addr
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| bad("address did not resolve"))?;
    let mut s = TcpStream::connect_timeout(&sa, timeout)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let b = body.unwrap_or("");
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: openmind-queue\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        b.len()
    );
    let mut req = Vec::with_capacity(head.len() + b.len() + 64);
    req.extend_from_slice(head.as_bytes());
    if let Some(t) = bearer {
        req.extend_from_slice(b"Authorization: Bearer ");
        req.extend_from_slice(t);
        req.extend_from_slice(b"\r\n");
    }
    req.extend_from_slice(b"Connection: close\r\n\r\n");
    req.extend_from_slice(b.as_bytes());
    let sent = s.write_all(&req);
    req.iter_mut().for_each(|x| *x = 0);
    sent?;
    let mut raw = Vec::new();
    s.take((MAX_HEADER + MAX_BODY) as u64)
        .read_to_end(&mut raw)?;
    parse_response(&raw)
}

/// Parse a full response (status line, headers, body to EOF or Content-Length).
pub fn parse_response(raw: &[u8]) -> io::Result<(u16, String)> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| bad("no header end"))?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let code: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| bad("bad status line"))?;
    let mut body = &raw[split + 4..];
    for l in head.lines().skip(1) {
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                if let Ok(n) = v.trim().parse::<usize>() {
                    body = &body[..n.min(body.len())];
                }
            }
        }
    }
    Ok((code, String::from_utf8_lossy(body).into_owned()))
}

/// Split `http://host:port/path` into (`host:port`, `/path`). Only plain
/// `http://` is supported (no TLS, by design: no openssl).
pub fn split_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if hostport.is_empty() {
        return None;
    }
    let hp = if hostport.contains(':') {
        hostport.to_string()
    } else {
        format!("{hostport}:80")
    };
    Some((hp, path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_with_body_and_query() {
        let raw =
            b"POST /jobs?wait_ms=5&x=1 HTTP/1.1\r\nHost: a\r\nContent-Length: 4\r\n\r\nabcdEXTRA";
        let r = read_request(&raw[..]).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/jobs");
        assert_eq!(r.param("wait_ms"), Some("5"));
        assert_eq!(r.param("nope"), None);
        assert_eq!(r.body, b"abcd");
    }

    #[test]
    fn rejects_oversized_and_chunked() {
        let big = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert!(read_request(big.as_bytes()).is_err());
        let ch = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(read_request(&ch[..]).is_err());
        assert!(read_request(&b"GARBAGE\r\n\r\n"[..]).is_err());
    }

    #[test]
    fn response_roundtrip() {
        let mut out = Vec::new();
        write_response(&mut out, 201, "{\"id\":1}").unwrap();
        assert_eq!(parse_response(&out).unwrap(), (201, "{\"id\":1}".into()));
    }

    #[test]
    fn authorization_header() {
        let raw = b"GET /stats HTTP/1.1\r\nauthorization:  Bearer xyz \r\n\r\n";
        let r = read_request(&raw[..]).unwrap();
        assert_eq!(r.authorization.as_deref(), Some("Bearer xyz"));
        let none = read_request(&b"GET / HTTP/1.1\r\n\r\n"[..]).unwrap();
        assert_eq!(none.authorization, None);
        let dup = b"GET / HTTP/1.1\r\nAuthorization: Bearer a\r\nAuthorization: Bearer b\r\n\r\n";
        assert!(read_request(&dup[..]).is_err());
    }

    #[test]
    fn unauthorized_has_challenge() {
        let mut out = Vec::new();
        write_response(&mut out, 401, "{}").unwrap();
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(s.starts_with("HTTP/1.1 401 Unauthorized\r\n"), "{s}");
        assert!(s.contains("WWW-Authenticate: Bearer realm=\"poseidon\"\r\n"));
        assert_eq!(parse_response(&out).unwrap().0, 401);
        let mut ok = Vec::new();
        write_response(&mut ok, 200, "{}").unwrap();
        assert!(!String::from_utf8(ok).unwrap().contains("WWW-Authenticate"));
    }

    #[test]
    fn url_split() {
        assert_eq!(
            split_url("http://127.0.0.1:8420/health"),
            Some(("127.0.0.1:8420".into(), "/health".into()))
        );
        assert_eq!(
            split_url("http://localhost"),
            Some(("localhost:80".into(), "/".into()))
        );
        assert_eq!(split_url("https://x"), None);
    }
}
