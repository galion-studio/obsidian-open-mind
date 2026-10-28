// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// Plain-HTTP GET with std only (no TLS), for the one case that does not need
// curl: an `http://` URL on loopback (a local egress proxy, a cache, or a
// test stand-in). HTTPS still goes through curl (see host.rs header and
// docs/third-party/one-no-shellout.md for why a pure-Rust TLS stack is not
// in this binary yet).
//
// Loopback only on purpose: the request may carry a bearer key, and a key
// must never cross a network in clear text. Non-loopback hosts are refused
// before any connection is made.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub const MAX_BODY: usize = 8 << 20;

/// (host, port, path) of an `http://` URL.
pub fn split_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "not an http:// URL".to_string())?;
    let (auth, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    if auth.contains('@') {
        return Err("credentials in the URL are not supported".into());
    }
    let (host, port) = if let Some(h) = auth.strip_prefix('[') {
        let (h, p) = h.split_once(']').ok_or("bad IPv6 host")?;
        let port = p.strip_prefix(':').map(str::parse).transpose();
        (h.to_string(), port.map_err(|_| "bad port")?.unwrap_or(80))
    } else {
        match auth.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().map_err(|_| "bad port")?),
            None => (auth.to_string(), 80),
        }
    };
    if host.is_empty() || path.contains(['\r', '\n', ' ']) {
        return Err("bad URL".into());
    }
    Ok((host, port, path))
}

fn loopback(addrs: &[SocketAddr]) -> bool {
    !addrs.is_empty()
        && addrs.iter().all(|a| match a.ip() {
            IpAddr::V4(v) => v.is_loopback(),
            IpAddr::V6(v) => v.is_loopback(),
        })
}

/// GET `url` (loopback http:// only) with extra headers. Returns the status
/// code and the body (Content-Length, chunked, or read-to-close), at most
/// `MAX_BODY` bytes, within `timeout` overall.
pub fn get(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let (host, port, path) = split_url(url)?;
    let addrs: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve: {e}"))?
        .collect();
    if !loopback(&addrs) {
        return Err("plain http is allowed to loopback only".into());
    }
    let start = Instant::now();
    let mut sock =
        TcpStream::connect_timeout(&addrs[0], timeout).map_err(|e| format!("connect: {e}"))?;
    sock.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    sock.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nAccept: application/json\r\n");
    for (k, v) in headers {
        if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
            return Err("bad header".into());
        }
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    sock.write_all(req.as_bytes())
        .map_err(|e| format!("send: {e}"))?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 << 10];
    loop {
        if start.elapsed() > timeout {
            return Err("timeout".into());
        }
        match sock.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > MAX_BODY + 64 * 1024 {
                    return Err("response too large".into());
                }
                if let Some(done) = complete(&raw) {
                    raw.truncate(done);
                    break;
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::Interrupted) => continue,
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    parse(&raw)
}

fn head_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Length of a complete Content-Length response, if we already have it.
fn complete(raw: &[u8]) -> Option<usize> {
    let end = head_end(raw)?;
    let head = std::str::from_utf8(&raw[..end]).ok()?;
    let len: usize = header(head, "content-length")?.parse().ok()?;
    (raw.len() >= end + 4 + len).then_some(end + 4 + len)
}

/// Status code and decoded body of a full HTTP/1.x response.
pub fn parse(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let end = head_end(raw).ok_or("no response header")?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| "bad header bytes")?;
    let status = head
        .lines()
        .next()
        .filter(|l| l.starts_with("HTTP/1."))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("bad status line")?;
    let body = &raw[end + 4..];
    let chunked = header(head, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    if !chunked {
        let body = match header(head, "content-length").and_then(|v| v.parse::<usize>().ok()) {
            Some(n) if n <= body.len() => &body[..n],
            Some(_) => return Err("body shorter than Content-Length".into()),
            None => body,
        };
        if body.len() > MAX_BODY {
            return Err("response too large".into());
        }
        return Ok((status, body.to_vec()));
    }
    let mut out = Vec::new();
    let mut p = 0;
    loop {
        let nl = body[p..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("bad chunk")?
            + p;
        let size_txt = std::str::from_utf8(&body[p..nl]).map_err(|_| "bad chunk size")?;
        let size = usize::from_str_radix(size_txt.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "bad chunk size")?;
        p = nl + 2;
        if size == 0 {
            return Ok((status, out));
        }
        if out.len() + size > MAX_BODY {
            return Err("response too large".into());
        }
        let chunk = body.get(p..p + size).ok_or("truncated chunk")?;
        out.extend_from_slice(chunk);
        p += size + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn urls_split_and_bad_ones_fail() {
        assert_eq!(
            split_url("http://127.0.0.1:8080/api/v1/models").unwrap(),
            ("127.0.0.1".into(), 8080, "/api/v1/models".into())
        );
        assert_eq!(
            split_url("http://localhost").unwrap(),
            ("localhost".into(), 80, "/".into())
        );
        assert_eq!(
            split_url("http://[::1]:9/x").unwrap(),
            ("::1".into(), 9, "/x".into())
        );
        for bad in [
            "https://x/",
            "http://u:p@h/",
            "http://h:99999/",
            "http:///x",
            "http://h/a b",
        ] {
            assert!(split_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn non_loopback_is_refused_before_connecting() {
        // 192.0.2.1 is TEST-NET-1: never routable, and we must not even try.
        let t = Instant::now();
        let e = get("http://192.0.2.1:9/models", &[], Duration::from_secs(5)).unwrap_err();
        assert!(e.contains("loopback"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn parses_length_chunked_and_close() {
        assert_eq!(
            parse(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhelloEXTRA").unwrap(),
            (200, b"hello".to_vec())
        );
        assert_eq!(
            parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n").unwrap(),
            (200, b"abcde".to_vec())
        );
        assert_eq!(
            parse(b"HTTP/1.0 404 Nope\r\n\r\nbody").unwrap(),
            (404, b"body".to_vec())
        );
        assert!(parse(b"garbage").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nshort").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
    }

    #[test]
    fn real_loopback_round_trip_sends_headers() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let srv = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut got = Vec::new();
            let mut b = [0u8; 4096];
            while head_end(&got).is_none() {
                let n = s.read(&mut b).unwrap();
                got.extend_from_slice(&b[..n]);
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n").unwrap();
            String::from_utf8(got).unwrap()
        });
        let (code, body) = get(
            &format!("http://127.0.0.1:{port}/api/v1/models"),
            &[("Authorization", "Bearer test-not-a-key")],
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!((code, body.as_slice()), (200, &b"{\"a\":1}"[..]));
        let req = srv.join().unwrap();
        assert!(req.starts_with("GET /api/v1/models HTTP/1.1\r\n"), "{req}");
        assert!(req.contains("\r\nAuthorization: Bearer test-not-a-key\r\n"));
        assert!(get(
            "http://127.0.0.1:1/",
            &[("Bad\r\nX", "1")],
            Duration::from_secs(1)
        )
        .is_err());
    }
}
