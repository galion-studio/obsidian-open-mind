// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--stt` (owner order t362u, 2026-10-29 CET +01:00): live dictation for
//! the `/hermes` mic button through the console.
//!
//! - `GET /api/stt/health` asks galion-stt's `/health` and answers
//!   `{"ok":bool,...}` (200 either way, so the page can say "STT offline").
//! - `GET /api/stt/stream` (WebSocket upgrade) is tunnelled byte for byte to
//!   galion-stt's `WS /stream`: the browser sends binary 16 kHz mono PCM16 LE
//!   frames and `{"type":"stop"}`; the service answers
//!   `{"type":"partial"|"final"|"error","line":N,"text":"..."}`.
//!
//! Both routes sit behind the console API gate (pairing cookie or loopback
//! Host + peer; unpaired is 401 before this module runs). The tunnel also
//! requires a same-origin `Origin` header (browsers always send it on a
//! WebSocket; a cross-site page could otherwise ride the pairing cookie),
//! caps concurrent sessions, session length, idle time and upload bytes,
//! and never forwards the browser's Cookie or Authorization upstream.

use crate::http::RequestHead;
use console_ui::client::{self, Forward};
use console_ui::gate;
use console_ui::json::Value;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub const HEALTH: &str = "/api/stt/health";
pub const STREAM: &str = "/api/stt/stream";
/// Longest single dictation session.
pub const MAX_SESSION: Duration = Duration::from_secs(300);
/// A session with no bytes either way for this long is closed.
pub const IDLE: Duration = Duration::from_secs(45);
/// Upload cap per session: 300 s of 16 kHz PCM16 is 9.6 MB, plus framing.
pub const MAX_UP_BYTES: u64 = 10 * 1024 * 1024;
const CONNECT: Duration = Duration::from_secs(2);
const HANDSHAKE: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(500);

/// True for a path this module answers.
pub fn owns(path: &str) -> bool {
    path.starts_with("/api/stt/")
}

pub struct Stt {
    pub addr: SocketAddr,
    pub max_sessions: usize,
    active: Arc<AtomicUsize>,
}

/// A reply this module writes as JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct Out {
    pub status: u16,
    pub body: String,
}

pub fn error(status: u16, msg: &str) -> Out {
    let v = Value::Obj(vec![
        ("ok".into(), Value::Bool(false)),
        ("error".into(), Value::Str(msg.into())),
    ]);
    Out {
        status,
        body: v.to_json(),
    }
}

/// Counts a live session; frees its slot when dropped.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Browsers always send `Origin` on a WebSocket. It must name the same
/// host the request came to. No `Origin` is accepted only on a loopback
/// Host (a local, non-browser client already past the loopback-peer gate).
pub fn origin_ok(head: &RequestHead) -> bool {
    let host = gate::normalize_host(head.header("host").unwrap_or(""));
    match head.header("origin") {
        Some(o) => {
            let o = o.trim();
            let authority = o
                .strip_prefix("https://")
                .or_else(|| o.strip_prefix("http://"));
            match authority {
                Some(a) => !a.contains('/') && !host.is_empty() && gate::normalize_host(a) == host,
                None => false,
            }
        }
        None => gate::is_loopback_host(&host),
    }
}

fn has_token(v: Option<&str>, token: &str) -> bool {
    v.is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)))
}

/// A `Sec-WebSocket-Key`: 24 base64 characters (16 random bytes).
fn key_ok(k: &str) -> bool {
    k.len() == 24
        && k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// Why an upgrade request is refused (`None` = it is a valid upgrade).
pub fn refuse(head: &RequestHead) -> Option<Out> {
    if head.method != "GET" {
        return Some(error(405, "use GET with a WebSocket upgrade"));
    }
    if !has_token(head.header("upgrade"), "websocket")
        || !has_token(head.header("connection"), "upgrade")
    {
        return Some(error(426, "this route is a WebSocket (Upgrade: websocket)"));
    }
    if head.header("sec-websocket-version").map(str::trim) != Some("13")
        || !head
            .header("sec-websocket-key")
            .is_some_and(|k| key_ok(k.trim()))
    {
        return Some(error(400, "bad WebSocket handshake"));
    }
    if !origin_ok(head) {
        return Some(error(403, "cross-origin dictation is refused"));
    }
    None
}

fn clean(v: &str) -> String {
    v.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

impl Stt {
    pub fn new(addr: SocketAddr, max_sessions: usize) -> Stt {
        Stt {
            addr,
            max_sessions,
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// `GET /api/stt/health`.
    pub fn health(&self) -> Out {
        let started = Instant::now();
        let fwd = Forward {
            host: self.addr.to_string(),
            ..Forward::default()
        };
        let r = client::call(self.addr, "GET", "/health", &fwd, None);
        let ms = (started.elapsed().as_secs_f64() * 10_000.0).round() / 10.0;
        let (ok, status, raw) = match &r {
            Ok(r) => (
                r.status == 200,
                r.status,
                console_ui::json::parse(&r.body).unwrap_or(Value::Null),
            ),
            Err(_) => (false, 0, Value::Null),
        };
        let mut pairs: Vec<(String, Value)> = vec![
            ("ok".into(), Value::Bool(ok)),
            ("status".into(), Value::Num(f64::from(status))),
            ("ms".into(), Value::Num(ms)),
            ("sessions".into(), Value::Num(self.active() as f64)),
            ("max_sessions".into(), Value::Num(self.max_sessions as f64)),
        ];
        if !ok {
            pairs.push(("error".into(), Value::Str("STT offline".into())));
        }
        for k in ["model", "service", "version", "sample_rate"] {
            let v = raw.get(k);
            if !matches!(v, Value::Null) {
                pairs.push((k.into(), v.clone()));
            }
        }
        Out {
            status: 200,
            body: Value::Obj(pairs).to_json(),
        }
    }

    /// Take a session slot, or `None` when the cap is reached.
    fn slot(&self) -> Option<Slot> {
        if self.active.fetch_add(1, Ordering::SeqCst) >= self.max_sessions {
            self.active.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot(Arc::clone(&self.active)))
    }

    /// Handle `/api/stt/stream`. Returns the status for the access log;
    /// `Err(out)` = write this JSON refusal instead (nothing was tunnelled).
    pub fn tunnel(
        &self,
        client: &mut TcpStream,
        head: &RequestHead,
        carry: Vec<u8>,
    ) -> Result<u16, Out> {
        if let Some(out) = refuse(head) {
            return Err(out);
        }
        let Some(slot) = self.slot() else {
            return Err(error(
                429,
                "too many dictation sessions open; stop one and try again",
            ));
        };
        let mut up = TcpStream::connect_timeout(&self.addr, CONNECT)
            .map_err(|_| error(502, "STT offline"))?;
        let _ = up.set_nodelay(true);
        let _ = up.set_read_timeout(Some(HANDSHAKE));
        let _ = up.set_write_timeout(Some(HANDSHAKE));
        let key = clean(head.header("sec-websocket-key").unwrap_or("").trim());
        let mut req = format!(
            "GET /stream HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n",
            self.addr
        );
        if let Some(p) = head.header("sec-websocket-protocol") {
            req.push_str(&format!("Sec-WebSocket-Protocol: {}\r\n", clean(p)));
        }
        req.push_str("\r\n");
        up.write_all(req.as_bytes())
            .map_err(|_| error(502, "STT offline"))?;
        // Upstream handshake reply.
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            if buf.len() > 8192 {
                return Err(error(502, "STT answered a bad handshake"));
            }
            match up.read(&mut chunk) {
                Ok(0) | Err(_) => return Err(error(502, "STT closed the handshake")),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        };
        let text = String::from_utf8_lossy(&buf[..end]).to_string();
        let code: u16 = text
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        if code != 101 {
            return Err(error(502, &format!("STT refused the stream (HTTP {code})")));
        }
        let hdr = |name: &str| {
            text.lines().skip(1).find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim().eq_ignore_ascii_case(name).then(|| clean(v.trim()))
            })
        };
        let Some(accept) = hdr("sec-websocket-accept") else {
            return Err(error(502, "STT answered a bad handshake"));
        };
        let mut reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n"
        );
        if let Some(p) = hdr("sec-websocket-protocol") {
            reply.push_str(&format!("Sec-WebSocket-Protocol: {p}\r\n"));
        }
        reply.push_str("\r\n");
        if client.write_all(reply.as_bytes()).is_err() {
            return Ok(499);
        }
        if end < buf.len() && client.write_all(&buf[end..]).is_err() {
            return Ok(499);
        }
        if !carry.is_empty() && up.write_all(&carry).is_err() {
            return Ok(502);
        }
        pump(client, up, carry.len() as u64, slot);
        Ok(101)
    }
}

/// Copy both ways until either side closes, the session or idle limit
/// passes, or the upload cap is hit. The slot is freed at the end.
fn pump(client: &mut TcpStream, up: TcpStream, already_up: u64, slot: Slot) {
    let (Ok(mut c_read), Ok(mut u_write)) = (client.try_clone(), up.try_clone()) else {
        return;
    };
    let mut u_read = up;
    let started = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let last = Arc::new(AtomicUsize::new(0)); // ms since start of last byte
    for s in [&c_read, &u_read] {
        let _ = s.set_read_timeout(Some(TICK));
    }
    let _ = client.set_write_timeout(Some(Duration::from_secs(10)));
    let _ = u_write.set_write_timeout(Some(Duration::from_secs(10)));
    let stop = move |done: &AtomicBool, last: &AtomicUsize| {
        let now = started.elapsed();
        let idle = now.saturating_sub(Duration::from_millis(last.load(Ordering::SeqCst) as u64));
        done.load(Ordering::SeqCst) || now >= MAX_SESSION || idle >= IDLE
    };
    let (d2, l2) = (Arc::clone(&done), Arc::clone(&last));
    let upload = thread::Builder::new()
        .name("stt-up".into())
        .stack_size(64 * 1024)
        .spawn(move || {
            let mut sent = already_up;
            let mut b = [0u8; 16 * 1024];
            loop {
                if stop(&d2, &l2) {
                    break;
                }
                match c_read.read(&mut b) {
                    Ok(0) => break,
                    Ok(n) => {
                        sent += n as u64;
                        if sent > MAX_UP_BYTES || u_write.write_all(&b[..n]).is_err() {
                            break;
                        }
                        l2.store(started.elapsed().as_millis() as usize, Ordering::SeqCst);
                    }
                    Err(e) if timed_out(&e) => {}
                    Err(_) => break,
                }
            }
            d2.store(true, Ordering::SeqCst);
            let _ = u_write.shutdown(Shutdown::Both);
            let _ = c_read.shutdown(Shutdown::Read);
        });
    let mut b = [0u8; 16 * 1024];
    loop {
        if stop(&done, &last) {
            break;
        }
        match u_read.read(&mut b) {
            Ok(0) => break,
            Ok(n) => {
                if client.write_all(&b[..n]).is_err() {
                    break;
                }
                last.store(started.elapsed().as_millis() as usize, Ordering::SeqCst);
            }
            Err(e) if timed_out(&e) => {}
            Err(_) => break,
        }
    }
    done.store(true, Ordering::SeqCst);
    let _ = u_read.shutdown(Shutdown::Both);
    let _ = client.shutdown(Shutdown::Both);
    if let Ok(h) = upload {
        let _ = h.join();
    }
    drop(slot);
}

fn timed_out(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(raw: &str) -> RequestHead {
        let mut carry = raw.as_bytes().to_vec();
        let mut empty: &[u8] = &[];
        crate::http::read_head(&mut empty, &mut carry).unwrap()
    }

    const UP: &str = "Upgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n";

    #[test]
    fn origin_rules() {
        let h = |host: &str, origin: Option<&str>| {
            let o = origin
                .map(|o| format!("Origin: {o}\r\n"))
                .unwrap_or_default();
            head(&format!(
                "GET /api/stt/stream HTTP/1.1\r\nHost: {host}\r\n{o}{UP}\r\n"
            ))
        };
        assert!(origin_ok(&h(
            "console.galion.app",
            Some("https://console.galion.app")
        )));
        assert!(origin_ok(&h(
            "127.0.0.1:18080",
            Some("http://127.0.0.1:18080")
        )));
        assert!(origin_ok(&h("127.0.0.1:18080", None)), "local tool");
        assert!(
            !origin_ok(&h("console.galion.app", None)),
            "browser always sends one"
        );
        assert!(!origin_ok(&h(
            "console.galion.app",
            Some("https://evil.example")
        )));
        assert!(!origin_ok(&h(
            "console.galion.app",
            Some("https://console.galion.app.evil.example")
        )));
        assert!(!origin_ok(&h("console.galion.app", Some("null"))));
        assert!(!origin_ok(&h(
            "console.galion.app",
            Some("https://console.galion.app/x")
        )));
    }

    #[test]
    fn handshake_checks() {
        let ok = head(&format!(
            "GET /api/stt/stream HTTP/1.1\r\nHost: 127.0.0.1\r\n{UP}\r\n"
        ));
        assert_eq!(refuse(&ok), None);
        let plain = head("GET /api/stt/stream HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(refuse(&plain).unwrap().status, 426);
        let bad_key = head("GET /api/stt/stream HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: short\r\n\r\n");
        assert_eq!(refuse(&bad_key).unwrap().status, 400);
        let post = head(&format!(
            "POST /api/stt/stream HTTP/1.1\r\nHost: 127.0.0.1\r\n{UP}\r\n"
        ));
        assert_eq!(refuse(&post).unwrap().status, 405);
    }

    #[test]
    fn slots_cap_and_free() {
        let s = Stt::new("127.0.0.1:9".parse().unwrap(), 2);
        let a = s.slot().unwrap();
        let b = s.slot().unwrap();
        assert!(s.slot().is_none());
        drop(a);
        assert_eq!(s.active(), 1);
        let _c = s.slot().unwrap();
        drop(b);
        assert_eq!(s.active(), 1);
    }
}
