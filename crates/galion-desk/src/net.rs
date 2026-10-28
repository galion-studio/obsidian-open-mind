// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
//! Loopback plumbing: TCP reachability, a tiny HTTP/1.1 client for Ollama,
//! and request reading/response writing for the desk server.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Largest request head the server reads.
pub const MAX_HEAD: usize = 16 * 1024;
/// Largest request body the server reads.
pub const MAX_BODY: usize = 64 * 1024;

/// First address `host:port` resolves to. `None` when it does not resolve.
pub fn resolve_addr(addr: &str) -> Option<SocketAddr> {
    addr.to_socket_addrs().ok()?.next()
}

/// `true` when a TCP connect to `addr` succeeds within `timeout`.
pub fn tcp_open_addr(addr: &str, timeout: Duration) -> bool {
    match resolve_addr(addr) {
        Some(sock) => TcpStream::connect_timeout(&sock, timeout).is_ok(),
        None => false,
    }
}

fn exchange(host: &str, request: &str, connect: Duration, read: Duration) -> Option<Vec<u8>> {
    let sock = resolve_addr(host)?;
    let mut stream = TcpStream::connect_timeout(&sock, connect).ok()?;
    stream.set_read_timeout(Some(read)).ok()?;
    stream.set_write_timeout(Some(connect)).ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    let mut buf = Vec::new();
    // A timeout after some bytes still yields what arrived.
    let _ = stream.read_to_end(&mut buf);
    Some(buf)
}

/// Body of `GET path` on `host` (`host:port`), or empty on any failure.
pub fn http_get(host: &str, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    let Some(buf) = exchange(
        host,
        &req,
        Duration::from_millis(400),
        Duration::from_millis(700),
    ) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&buf);
    text.split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
}

/// Raw response of `POST path` with a JSON body, or empty on any failure.
pub fn http_post(host: &str, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    exchange(host, &req, Duration::from_secs(2), Duration::from_secs(90))
        .map(|buf| String::from_utf8_lossy(&buf).into_owned())
        .unwrap_or_default()
}

/// One parsed request.
#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// Why a request could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The peer closed or sent nothing before the timeout.
    Empty,
    /// Head larger than [`MAX_HEAD`] or body larger than [`MAX_BODY`].
    TooLarge,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn content_length(head: &str) -> Option<usize> {
    head.split("\r\n").skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

/// Read one request from `reader`. Reads until the head is complete, then
/// until `content-length` body bytes arrived (or EOF/timeout). A request
/// without `content-length` keeps whatever body bytes came with the head,
/// which is what the Oct 10 binary did with its single 8 KiB read.
pub fn read_request(reader: &mut dyn Read) -> Result<Request, ReadError> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break Some(i);
        }
        if buf.len() > MAX_HEAD {
            return Err(ReadError::TooLarge);
        }
        match reader.read(&mut tmp) {
            Ok(0) | Err(_) => break None,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    };
    if buf.is_empty() {
        return Err(ReadError::Empty);
    }
    let (head, mut body) = match head_end {
        Some(i) => (
            String::from_utf8_lossy(&buf[..i]).into_owned(),
            buf[i + 4..].to_vec(),
        ),
        None => (String::from_utf8_lossy(&buf).into_owned(), Vec::new()),
    };
    if let Some(want) = content_length(&head) {
        if want > MAX_BODY {
            return Err(ReadError::TooLarge);
        }
        while body.len() < want {
            match reader.read(&mut tmp) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&tmp[..n]),
            }
        }
        body.truncate(want);
    } else if body.len() > MAX_BODY {
        return Err(ReadError::TooLarge);
    }
    let start = head.split("\r\n").next().unwrap_or("");
    let mut parts = start.split_whitespace();
    Ok(Request {
        method: parts.next().unwrap_or("GET").to_string(),
        path: parts.next().unwrap_or("/").to_string(),
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// Reason phrase for the codes the desk sends.
pub fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        413 => "Payload Too Large",
        _ => "Not Found",
    }
}

/// Full response bytes: status line, the three historical headers, body.
pub fn response(code: u16, body: &str) -> String {
    format!(
        "HTTP/1.1 {code} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        reason(code),
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A reader that hands out one chunk per `read` call, like a socket.
    struct Chunks(Vec<Vec<u8>>);
    impl Read for Chunks {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let chunk = self.0.remove(0);
            let n = chunk.len().min(out.len());
            out[..n].copy_from_slice(&chunk[..n]);
            if n < chunk.len() {
                self.0.insert(0, chunk[n..].to_vec());
            }
            Ok(n)
        }
    }

    #[test]
    fn reads_simple_get() {
        let mut r = Cursor::new(b"GET /desk HTTP/1.1\r\nHost: x\r\n\r\n".to_vec());
        let req = read_request(&mut r).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/desk");
        assert_eq!(req.body, "");
    }

    #[test]
    fn body_split_across_reads_is_reassembled() {
        let mut r = Chunks(vec![
            b"POST /desk HTTP/1.1\r\nContent-Length: 26\r\n".to_vec(),
            b"\r\n{\"act\":\"note\",".to_vec(),
            b"\"text\":\"hi\"}".to_vec(),
            b"   trailing garbage".to_vec(),
        ]);
        let req = read_request(&mut r).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.body, "{\"act\":\"note\",\"text\":\"hi\"}");
    }

    #[test]
    fn missing_content_length_keeps_bytes_with_head() {
        let mut r = Cursor::new(b"POST /desk HTTP/1.1\r\n\r\n{\"act\":\"drop\"}".to_vec());
        assert_eq!(read_request(&mut r).unwrap().body, "{\"act\":\"drop\"}");
    }

    #[test]
    fn empty_and_oversized_requests_are_rejected() {
        assert_eq!(
            read_request(&mut Cursor::new(Vec::new())),
            Err(ReadError::Empty)
        );
        let big = format!(
            "POST /desk HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(
            read_request(&mut Cursor::new(big.into_bytes())),
            Err(ReadError::TooLarge)
        );
        let huge_head = vec![b'a'; MAX_HEAD + 10];
        assert_eq!(
            read_request(&mut Cursor::new(huge_head)),
            Err(ReadError::TooLarge)
        );
    }

    #[test]
    fn head_without_blank_line_still_parses_start_line() {
        let req = read_request(&mut Cursor::new(b"GET /health".to_vec())).unwrap();
        assert_eq!((req.method.as_str(), req.path.as_str()), ("GET", "/health"));
        let req = read_request(&mut Cursor::new(b"\r\n\r\n".to_vec())).unwrap();
        assert_eq!((req.method.as_str(), req.path.as_str()), ("GET", "/"));
    }

    #[test]
    fn response_layout_matches_live_desk() {
        assert_eq!(
            response(404, "{\"error\":\"not found\"}"),
            "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\ncontent-length: 21\r\nconnection: close\r\n\r\n{\"error\":\"not found\"}"
        );
        assert!(response(200, "{}").starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response(413, "{}").starts_with("HTTP/1.1 413 Payload Too Large\r\n"));
    }

    #[test]
    fn closed_and_unresolvable_targets_are_down() {
        assert!(!tcp_open_addr("127.0.0.1:1", Duration::from_millis(200)));
        assert!(!tcp_open_addr("not a host", Duration::from_millis(200)));
        assert_eq!(http_get("127.0.0.1:1", "/api/ps"), "");
        assert_eq!(http_post("127.0.0.1:1", "/api/generate", "{}"), "");
    }

    #[test]
    fn http_get_returns_body_from_a_local_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut tmp = [0u8; 1024];
            let _ = s.read(&mut tmp);
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"models\":[]}")
                .unwrap();
        });
        assert_eq!(http_get(&addr, "/api/ps"), "{\"models\":[]}");
        server.join().unwrap();
    }
}
