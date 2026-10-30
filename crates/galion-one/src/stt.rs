// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Live dictation for the Galion One V2 page (owner order t431u,
//! 2026-10-30 CET). Ported from console-static `--stt` (stt.rs, t362u).
//!
//! - `GET /stt/health` asks galion-stt's `/health` and answers
//!   `{"ok":bool,...}` (200 either way, so the page can say "offline").
//! - `GET /stt/stream` (WebSocket upgrade) is tunnelled byte for byte to
//!   galion-stt's `WS /stream`: the browser sends binary 16 kHz mono PCM16
//!   LE frames and `{"type":"stop"}`; the service answers
//!   `{"type":"partial"|"final"|"error","line":N,"text":"..."}`.
//!
//! Both routes need the pairing cookie (checked by main.rs before this
//! module runs). The tunnel also requires a same-origin `Origin` header
//! (a cross-site page could otherwise ride the cookie), caps concurrent
//! sessions, session length, idle time and upload bytes, and never
//! forwards the browser's Cookie upstream.
//!
//! `GALION_STT` overrides the service address (default 127.0.0.1:8651).

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const HEALTH: &str = "/stt/health";
pub const STREAM: &str = "/stt/stream";
/// Longest single dictation session.
pub const MAX_SESSION: Duration = Duration::from_secs(300);
/// A session with no bytes either way for this long is closed.
pub const IDLE: Duration = Duration::from_secs(45);
/// Upload cap per session: 300 s of 16 kHz PCM16 is 9.6 MB, plus framing.
pub const MAX_UP_BYTES: u64 = 10 * 1024 * 1024;
/// Sessions open at once (galion-stt itself allows 2).
pub const MAX_SESSIONS: usize = 2;
const CONNECT: Duration = Duration::from_secs(2);
const HANDSHAKE: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(500);

/// The request headers this module reads (lower-case names).
#[derive(Default, Debug, Clone)]
pub struct Head {
    pub method: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A JSON refusal: (status, body).
pub type Out = (u16, String);

fn error(status: u16, msg: &str) -> Out {
    (
        status,
        format!("{{\"ok\":false,\"error\":{}}}", crate::quote(msg)),
    )
}

pub fn addr() -> SocketAddr {
    static SLOT: OnceLock<SocketAddr> = OnceLock::new();
    *SLOT.get_or_init(|| {
        let raw = std::env::var("GALION_STT").unwrap_or_default();
        std::net::ToSocketAddrs::to_socket_addrs(raw.trim())
            .ok()
            .and_then(|mut a| a.next())
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 8651)))
    })
}

fn active() -> &'static Arc<AtomicUsize> {
    static N: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
    N.get_or_init(|| Arc::new(AtomicUsize::new(0)))
}

/// Counts a live session; frees its slot when dropped.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn slot(counter: &Arc<AtomicUsize>, max: usize) -> Option<Slot> {
    if counter.fetch_add(1, Ordering::SeqCst) >= max {
        counter.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    Some(Slot(Arc::clone(counter)))
}

/// Lower-case host without port.
pub fn normalize_host(h: &str) -> String {
    let h = h.trim().to_ascii_lowercase();
    if let Some(rest) = h.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("").to_string();
    }
    h.rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit()))
        .map(|(host, _)| host.to_string())
        .unwrap_or(h)
}

fn is_loopback_host(h: &str) -> bool {
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

/// Browsers always send `Origin` on a WebSocket; it must name the host the
/// request came to. No `Origin` is accepted only on a loopback Host.
pub fn origin_ok(head: &Head) -> bool {
    let host = normalize_host(head.header("host").unwrap_or(""));
    match head.header("origin") {
        Some(o) => {
            let o = o.trim();
            let authority = o
                .strip_prefix("https://")
                .or_else(|| o.strip_prefix("http://"));
            match authority {
                Some(a) => !a.contains('/') && !host.is_empty() && normalize_host(a) == host,
                None => false,
            }
        }
        None => is_loopback_host(&host),
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
pub fn refuse(head: &Head) -> Option<Out> {
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

/// `GET /stt/health`: always 200 with `ok` true or false.
pub fn health() -> Vec<u8> {
    let started = Instant::now();
    let r = crate::http_to(addr(), "GET", "/health", &[], b"", Duration::from_secs(3));
    let ms = started.elapsed().as_millis();
    let (ok, status, body) = match r {
        Ok((code, body)) => (code == 200, code, body),
        Err(_) => (false, 0, Vec::new()),
    };
    let engine = crate::json_string(&body, "engine");
    let mut out = format!(
        "{{\"ok\":{ok},\"status\":{status},\"ms\":{ms},\"sessions\":{},\"max_sessions\":{MAX_SESSIONS}",
        active().load(Ordering::SeqCst)
    );
    if !engine.is_empty() {
        out.push_str(&format!(",\"engine\":{}", crate::quote(&engine)));
    }
    if !ok {
        out.push_str(",\"error\":\"STT offline\"");
    }
    out.push('}');
    out.into_bytes()
}

/// Handle `/stt/stream`. `Err(out)` = write this JSON refusal instead
/// (nothing was tunnelled). `carry` is any bytes read after the head.
pub fn tunnel(client: &mut TcpStream, head: &Head, carry: &[u8]) -> Result<(), Out> {
    tunnel_to(client, head, carry, addr(), active(), MAX_SESSIONS)
}

fn tunnel_to(
    client: &mut TcpStream,
    head: &Head,
    carry: &[u8],
    to: SocketAddr,
    counter: &Arc<AtomicUsize>,
    max: usize,
) -> Result<(), Out> {
    if let Some(out) = refuse(head) {
        return Err(out);
    }
    let Some(slot) = slot(counter, max) else {
        return Err(error(
            429,
            "too many dictation sessions open; stop one and try again",
        ));
    };
    let mut up = TcpStream::connect_timeout(&to, CONNECT).map_err(|_| error(502, "STT offline"))?;
    let _ = up.set_nodelay(true);
    let _ = up.set_read_timeout(Some(HANDSHAKE));
    let _ = up.set_write_timeout(Some(HANDSHAKE));
    let key = clean(head.header("sec-websocket-key").unwrap_or("").trim());
    let mut req = format!(
        "GET /stream HTTP/1.1\r\nHost: {to}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
    );
    if let Some(p) = head.header("sec-websocket-protocol") {
        req.push_str(&format!("Sec-WebSocket-Protocol: {}\r\n", clean(p)));
    }
    req.push_str("\r\n");
    up.write_all(req.as_bytes())
        .map_err(|_| error(502, "STT offline"))?;
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
        return Ok(());
    }
    if end < buf.len() && client.write_all(&buf[end..]).is_err() {
        return Ok(());
    }
    if !carry.is_empty() && up.write_all(carry).is_err() {
        return Ok(());
    }
    pump(client, up, carry.len() as u64, slot);
    Ok(())
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
    use std::net::TcpListener;

    const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";

    fn head(method: &str, host: &str, origin: Option<&str>, upgrade: bool) -> Head {
        let mut headers = vec![("host".to_string(), host.to_string())];
        if let Some(o) = origin {
            headers.push(("origin".into(), o.into()));
        }
        if upgrade {
            headers.push(("upgrade".into(), "websocket".into()));
            headers.push(("connection".into(), "keep-alive, Upgrade".into()));
            headers.push(("sec-websocket-version".into(), "13".into()));
            headers.push(("sec-websocket-key".into(), KEY.into()));
        }
        Head {
            method: method.into(),
            headers,
        }
    }

    #[test]
    fn origin_rules() {
        let h = |host: &str, o: Option<&str>| origin_ok(&head("GET", host, o, true));
        assert!(h("one.galion.app", Some("https://one.galion.app")));
        assert!(h("127.0.0.1:8091", Some("http://127.0.0.1:8091")));
        assert!(h("127.0.0.1:8091", None), "local tool");
        assert!(!h("one.galion.app", None), "browser always sends one");
        assert!(!h("one.galion.app", Some("https://evil.example")));
        assert!(!h(
            "one.galion.app",
            Some("https://one.galion.app.evil.example")
        ));
        assert!(!h("one.galion.app", Some("null")));
        assert!(!h("one.galion.app", Some("https://one.galion.app/x")));
        assert!(!h("one.galion.app", Some("https://galion.app")));
    }

    #[test]
    fn handshake_checks() {
        assert!(refuse(&head("GET", "127.0.0.1", None, true)).is_none());
        assert_eq!(
            refuse(&head("GET", "127.0.0.1", None, false)).unwrap().0,
            426
        );
        assert_eq!(
            refuse(&head("POST", "127.0.0.1", None, true)).unwrap().0,
            405
        );
        let mut bad = head("GET", "127.0.0.1", None, true);
        bad.headers.retain(|(k, _)| k != "sec-websocket-key");
        bad.headers
            .push(("sec-websocket-key".into(), "short".into()));
        assert_eq!(refuse(&bad).unwrap().0, 400);
        assert_eq!(
            refuse(&head(
                "GET",
                "one.galion.app",
                Some("https://x.example"),
                true
            ))
            .unwrap()
            .0,
            403
        );
    }

    #[test]
    fn slots_cap_and_free() {
        let c = Arc::new(AtomicUsize::new(0));
        let a = slot(&c, 2).unwrap();
        let b = slot(&c, 2).unwrap();
        assert!(slot(&c, 2).is_none());
        drop(a);
        assert_eq!(c.load(Ordering::SeqCst), 1);
        let _c2 = slot(&c, 2).unwrap();
        drop(b);
        assert_eq!(c.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn hosts_normalize() {
        assert_eq!(normalize_host("One.Galion.App"), "one.galion.app");
        assert_eq!(normalize_host("127.0.0.1:8091"), "127.0.0.1");
        assert_eq!(normalize_host("[::1]:8091"), "::1");
    }

    /// End to end over loopback: a fake galion-stt answers the upgrade and
    /// echoes one text frame; the tunnel relays the 101 and the frame, and
    /// frees its slot when the client closes.
    #[test]
    fn tunnel_relays_both_ways() {
        let fake = TcpListener::bind("127.0.0.1:0").unwrap();
        let fake_addr = fake.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = fake.accept().unwrap();
            let mut got = Vec::new();
            let mut b = [0u8; 1024];
            while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut b).unwrap();
                got.extend_from_slice(&b[..n]);
            }
            let req = String::from_utf8_lossy(&got).to_string();
            assert!(req.starts_with("GET /stream HTTP/1.1"));
            assert!(!req.to_ascii_lowercase().contains("cookie"));
            s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n").unwrap();
            // read the browser's carry bytes, answer with a frame
            let n = s.read(&mut b).unwrap();
            assert_eq!(&b[..n], b"PCM!");
            s.write_all(b"\x81\x02hi").unwrap();
            let _ = s.read(&mut b);
        });
        let front = TcpListener::bind("127.0.0.1:0").unwrap();
        let front_addr = front.local_addr().unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&counter);
        let relay = thread::spawn(move || {
            let (mut s, _) = front.accept().unwrap();
            let h = head("GET", "127.0.0.1", None, true);
            tunnel_to(&mut s, &h, b"PCM!", fake_addr, &c2, 2).unwrap();
        });
        let mut browser = TcpStream::connect(front_addr).unwrap();
        browser
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut got = Vec::new();
        let mut b = [0u8; 1024];
        while !got.ends_with(b"\x81\x02hi") {
            let n = browser.read(&mut b).unwrap();
            assert!(n > 0, "closed early: {:?}", String::from_utf8_lossy(&got));
            got.extend_from_slice(&b[..n]);
        }
        let text = String::from_utf8_lossy(&got);
        assert!(text.starts_with("HTTP/1.1 101"));
        assert!(text.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        drop(browser);
        relay.join().unwrap();
        server.join().unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn offline_is_502() {
        // Port 9 (discard) is closed on loopback in CI and here.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let dead = l.local_addr().unwrap();
        drop(l);
        let front = TcpListener::bind("127.0.0.1:0").unwrap();
        let fa = front.local_addr().unwrap();
        let t = thread::spawn(move || {
            let (mut s, _) = front.accept().unwrap();
            let counter = Arc::new(AtomicUsize::new(0));
            let r = tunnel_to(
                &mut s,
                &head("GET", "127.0.0.1", None, true),
                b"",
                dead,
                &counter,
                2,
            );
            assert_eq!(r.unwrap_err().0, 502);
            assert_eq!(counter.load(Ordering::SeqCst), 0);
        });
        let _c = TcpStream::connect(fa).unwrap();
        t.join().unwrap();
    }
}
