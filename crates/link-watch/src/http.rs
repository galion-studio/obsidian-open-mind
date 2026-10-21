// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tiny blocking HTTP/1.1 GET client for the plain-HTTP loopback probes
//! (Ollama, Hermes, the France console), exactly like the Python urllib
//! calls it replaces: no TLS, no redirects followed. HTTPS public URLs go
//! through [`crate::urls`], which uses the system `curl` like hermes-rs does.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Largest response the probe keeps (head + body). Probes only need a status
/// and a small JSON body, so a huge or endless response is cut here.
pub const MAX_RESPONSE: u64 = 1024 * 1024;

pub(crate) fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Decode a chunked body. `None` when the framing is broken.
pub fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = find(data, b"\r\n")?;
        let line = String::from_utf8_lossy(&data[..nl]);
        let size_str = line.split(';').next()?.trim();
        let size = usize::from_str_radix(size_str, 16).ok()?;
        data = &data[nl + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size..];
        if data.len() >= 2 {
            data = &data[2..];
        }
    }
    Some(out)
}

/// Split an `http://host[:port]/path` URL into `(host:port, path)`.
pub fn split_http_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if hostport.is_empty() {
        return None;
    }
    let addr = if hostport.contains(':') {
        hostport.to_string()
    } else {
        format!("{hostport}:80")
    };
    Some((addr, path.to_string()))
}

/// `GET url` over plain HTTP. `(status, body)` or `None` on any failure.
pub fn http_get(url: &str, timeout: Duration) -> Option<(u16, Vec<u8>)> {
    let (addr, path) = split_http_url(url)?;
    let host_header = url
        .strip_prefix("http://")
        .and_then(|r| r.split('/').next())
        .unwrap_or(&addr)
        .to_string();
    let sock = addr.to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&sock, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUser-Agent: link-watch\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.take(MAX_RESPONSE).read_to_end(&mut buf).ok()?;
    parse_response(&buf)
}

/// Parse a raw HTTP/1.x response into `(status, decoded body)`.
pub fn parse_response(buf: &[u8]) -> Option<(u16, Vec<u8>)> {
    let sep = find(buf, b"\r\n\r\n")?;
    let head = &buf[..sep];
    let raw_body = &buf[sep + 4..];
    let first_line_end = find(head, b"\r\n").unwrap_or(head.len());
    let status_line = String::from_utf8_lossy(&head[..first_line_end]);
    let status: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    let headers = String::from_utf8_lossy(head).to_ascii_lowercase();
    let body = if headers.contains("transfer-encoding: chunked") {
        dechunk(raw_body)?
    } else {
        raw_body.to_vec()
    };
    Some((status, body))
}

/// CPython `probe(url, timeout)`: `(ok, milliseconds, body)`. `ok` means a
/// status in 200..400.
pub fn probe(url: &str, timeout: Duration) -> (bool, i64, Vec<u8>) {
    let started = Instant::now();
    let got = http_get(url, timeout);
    let ms = started.elapsed().as_millis() as i64;
    match got {
        Some((status, body)) if (200..400).contains(&status) => (true, ms, body),
        _ => (false, ms, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_split() {
        assert_eq!(
            split_http_url("http://127.0.0.1:11434/api/ps"),
            Some(("127.0.0.1:11434".into(), "/api/ps".into()))
        );
        assert_eq!(
            split_http_url("http://example.com"),
            Some(("example.com:80".into(), "/".into()))
        );
        assert_eq!(split_http_url("https://example.com/"), None);
        assert_eq!(split_http_url("http:///x"), None);
    }

    #[test]
    fn chunked_and_plain_bodies() {
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked), Some((200, b"Wikipedia".to_vec())));
        let plain = b"HTTP/1.0 302 Found\r\nLocation: /x\r\n\r\nmoved";
        assert_eq!(parse_response(plain), Some((302, b"moved".to_vec())));
        assert_eq!(parse_response(b"garbage"), None);
        assert_eq!(parse_response(b"HTTP/1.1 abc\r\n\r\n"), None);
        assert_eq!(dechunk(b"zz\r\n"), None);
        assert_eq!(dechunk(b"10\r\nshort\r\n"), None);
    }

    #[test]
    fn closed_port_is_down() {
        let (ok, _, body) = probe("http://127.0.0.1:1/health", Duration::from_millis(300));
        assert!(!ok);
        assert!(body.is_empty());
    }
}
