// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tiny blocking HTTP/1.1 client and a fixture stub server. Loopback only,
//! no TLS. The request goes out in ONE write: several stack services read a
//! request with a single `read()` call, so a split write would be a harness
//! bug, not a service bug.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// One parsed response.
#[derive(Debug, Clone, Default)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    /// First header with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.body).map_err(|e| format!("body is not JSON: {e}"))
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = find(data, b"\r\n")?;
        let size_line = String::from_utf8_lossy(&data[..nl]).into_owned();
        let size = usize::from_str_radix(size_line.split(';').next()?.trim(), 16).ok()?;
        data = &data[nl + 2..];
        if size == 0 {
            return Some(out);
        }
        if data.len() < size {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..).unwrap_or(&[]);
    }
}

/// Parse a raw response (status line, headers, body).
pub fn parse_response(raw: &[u8]) -> Result<Resp, String> {
    let sep = find(raw, b"\r\n\r\n").ok_or("no header terminator")?;
    let head = String::from_utf8_lossy(&raw[..sep]).into_owned();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad status line {status_line:?}"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let raw_body = &raw[sep + 4..];
    let chunked = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });
    let body = if chunked {
        dechunk(raw_body).ok_or("bad chunked body")?
    } else {
        raw_body.to_vec()
    };
    Ok(Resp {
        status,
        headers,
        body,
    })
}

fn expected_len(raw: &[u8]) -> Option<usize> {
    let sep = find(raw, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..sep]).to_ascii_lowercase();
    let len = head
        .split("\r\n")
        .find_map(|l| l.strip_prefix("content-length:"))?
        .trim()
        .parse::<usize>()
        .ok()?;
    Some(sep + 4 + len)
}

/// Send one request and read the whole response.
pub fn request(
    addr: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> Result<Resp, String> {
    let sock: SocketAddr = addr
        .to_socket_addrs()
        .map_err(|e| format!("{addr}: {e}"))?
        .next()
        .ok_or_else(|| format!("{addr}: no address"))?;
    let mut s =
        TcpStream::connect_timeout(&sock, timeout).map_err(|e| format!("connect {addr}: {e}"))?;
    s.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut msg = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: galion-realtest\r\nConnection: close\r\n");
    for (k, v) in headers {
        msg.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() || method == "POST" {
        msg.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    msg.push_str("\r\n");
    let mut out = msg.into_bytes();
    out.extend_from_slice(body);
    s.write_all(&out)
        .map_err(|e| format!("write {addr}: {e}"))?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let deadline = Instant::now() + timeout;
    loop {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if expected_len(&raw).is_some_and(|want| raw.len() >= want) {
                    break;
                }
            }
            Err(e) if raw.is_empty() => return Err(format!("read {addr}: {e}")),
            Err(_) => break,
        }
        if Instant::now() > deadline {
            break;
        }
    }
    parse_response(&raw)
}

pub fn get(addr: &str, path: &str) -> Result<Resp, String> {
    request(addr, "GET", path, &[], b"", Duration::from_secs(5))
}

pub fn post_json(
    addr: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<Resp, String> {
    let mut h = vec![("Content-Type", "application/json")];
    h.extend_from_slice(headers);
    request(
        addr,
        "POST",
        path,
        &h,
        body.as_bytes(),
        Duration::from_secs(5),
    )
}

/// True when something accepts a TCP connection on `addr`.
pub fn port_open(addr: &str) -> bool {
    addr.to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
}

/// Fixture upstream for the stack: stands in for Ollama (`/api/ps`,
/// `/api/tags`), Hermes (`/health`) and any model/TTS endpoint, so no
/// service under test ever reaches a real (or paid) upstream.
pub struct Stub {
    pub addr: String,
    /// When false, `/health` answers 503 (simulates Hermes going down).
    pub hermes_up: Arc<AtomicBool>,
    /// Requests served (all paths).
    pub hits: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

pub const STUB_MODEL: &str = "galion-harness";
pub const STUB_CTX: u64 = 4096;

fn stub_reply(path: &str, method: &str, hermes_up: bool) -> (u16, &'static str, String) {
    let json = "application/json";
    match (method, path) {
        ("GET", "/api/ps") => (
            200,
            json,
            format!("{{\"models\":[{{\"name\":\"{STUB_MODEL}\",\"context_length\":{STUB_CTX},\"size_vram\":0}}]}}"),
        ),
        ("GET", "/api/tags") => (200, json, format!("{{\"models\":[{{\"name\":\"{STUB_MODEL}\"}}]}}")),
        ("GET", "/health") if hermes_up => (200, json, "{\"status\":\"ok\",\"implementation\":\"realtest-stub\"}".into()),
        ("GET", "/health") => (503, json, "{\"status\":\"down\"}".into()),
        // Every model / speech call is refused: the harness never spends.
        _ => (503, json, "{\"error\":\"realtest stub: upstream disabled\"}".into()),
    }
}

impl Stub {
    pub fn start(addr: &str) -> Result<Stub, String> {
        let listener = TcpListener::bind(addr).map_err(|e| format!("stub bind {addr}: {e}"))?;
        let hermes_up = Arc::new(AtomicBool::new(true));
        let hits = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, n, st) = (hermes_up.clone(), hits.clone(), stop.clone());
        let handle = thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut s) = conn else { continue };
                n.fetch_add(1, Ordering::SeqCst);
                let up = h.load(Ordering::SeqCst);
                thread::spawn(move || {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
                    let mut buf = [0u8; 8192];
                    let got = s.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..got]).into_owned();
                    let mut parts = req.split_whitespace();
                    let method = parts.next().unwrap_or("GET").to_string();
                    let path = parts
                        .next()
                        .unwrap_or("/")
                        .split('?')
                        .next()
                        .unwrap_or("/")
                        .to_string();
                    let (code, kind, body) = stub_reply(&path, &method, up);
                    let reason = if code == 200 {
                        "OK"
                    } else {
                        "Service Unavailable"
                    };
                    let msg = format!(
                        "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(msg.as_bytes());
                });
            }
        });
        Ok(Stub {
            addr: addr.to_string(),
            hermes_up,
            hits,
            stop,
            handle: Some(handle),
        })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = port_open(&self.addr); // wake the blocking accept
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length_and_chunked() {
        let r =
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-A: b\r\n\r\nok").unwrap();
        assert_eq!(
            (r.status, r.text().as_str(), r.header("x-a")),
            (200, "ok", Some("b"))
        );
        let c = parse_response(b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n1\r\n!\r\n0\r\n\r\n").unwrap();
        assert_eq!(c.text(), "hi!");
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn stub_never_serves_model_calls() {
        assert_eq!(stub_reply("/v1/chat/completions", "POST", true).0, 503);
        assert_eq!(stub_reply("/health", "GET", false).0, 503);
        assert!(stub_reply("/api/ps", "GET", true).2.contains(STUB_MODEL));
    }

    /// Uses the slot-14 port range (19140-19149) only.
    #[test]
    fn stub_round_trip_on_slot_port() {
        let mut stub = Stub::start("127.0.0.1:19149").expect("bind 19149");
        let r = get("127.0.0.1:19149", "/health").unwrap();
        assert_eq!(r.status, 200);
        stub.hermes_up.store(false, Ordering::SeqCst);
        assert_eq!(get("127.0.0.1:19149", "/health").unwrap().status, 503);
        let p = post_json("127.0.0.1:19149", "/v1/chat/completions", &[], "{}").unwrap();
        assert_eq!(p.status, 503);
        stub.stop();
        assert!(!port_open("127.0.0.1:19149"));
    }
}
