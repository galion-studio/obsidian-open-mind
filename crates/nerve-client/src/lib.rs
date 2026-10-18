// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! nerve-client: optional, fire-and-forget signal push to `galion-nerve`.
//!
//! Set `NERVE_URL=http://127.0.0.1:8670` in a service's environment to turn it
//! on; unset (the default) makes every call a no-op that costs one atomic load.
//! Each signal is one `POST /signal` on its own short-lived thread with 500 ms
//! timeouts; at most 4 are in flight, extra signals are dropped. Loopback
//! `http://` only (no TLS, no deps). Never blocks or fails the caller.
//!
//! ```no_run
//! nerve_client::event("hermes-rs", "started", "v0.1.0");
//! nerve_client::metric("hermes-rs", "requests_total", 42.0);
//! nerve_client::error("hermes-rs", "http_5xx", "POST /v1/chat/completions -> 502");
//! nerve_client::start_heartbeat("hermes-rs", 60);
//! ```

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const MAX_IN_FLIGHT: usize = 4;
const TIMEOUT: Duration = Duration::from_millis(500);
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static TARGET: OnceLock<Option<SocketAddr>> = OnceLock::new();

/// Parse `http://host:port[/...]` into a loopback socket address. Non-loopback
/// targets are refused so a mis-set env var cannot ship signals off the box.
pub fn parse_url(url: &str) -> Option<SocketAddr> {
    let rest = url.trim().strip_prefix("http://")?;
    let hostport = rest.split('/').next()?;
    let addr = hostport.to_socket_addrs().ok()?.next()?;
    addr.ip().is_loopback().then_some(addr)
}

fn target() -> Option<SocketAddr> {
    *TARGET.get_or_init(|| std::env::var("NERVE_URL").ok().and_then(|u| parse_url(&u)))
}

/// Is pushing enabled (NERVE_URL set to a loopback http URL)?
pub fn enabled() -> bool {
    target().is_some()
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars().take(300) {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// The JSON body for one signal (exposed for tests and other transports).
pub fn body(source: &str, kind: &str, name: &str, value: Option<f64>, msg: &str) -> String {
    let v = match value {
        Some(f) if f.is_finite() => format!(",\"value\":{f}"),
        _ => String::new(),
    };
    format!(
        "{{\"source\":\"{}\",\"kind\":\"{}\",\"name\":\"{}\"{v},\"msg\":\"{}\"}}",
        esc(source),
        esc(kind),
        esc(name),
        esc(msg)
    )
}

fn post(addr: SocketAddr, body: &str) -> std::io::Result<()> {
    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    s.set_write_timeout(Some(TIMEOUT))?;
    s.set_read_timeout(Some(TIMEOUT))?;
    write!(
        s,
        "POST /signal HTTP/1.0\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut b = [0u8; 64];
    let _ = s.read(&mut b);
    Ok(())
}

/// Send one signal. Returns immediately; no-op when disabled or saturated.
pub fn send(source: &str, kind: &str, name: &str, value: Option<f64>, msg: &str) {
    let Some(addr) = target() else { return };
    if IN_FLIGHT.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let b = body(source, kind, name, value, msg);
    let spawned = std::thread::Builder::new()
        .name("nerve-push".into())
        .stack_size(64 * 1024)
        .spawn(move || {
            let _ = post(addr, &b);
            IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn event(source: &str, name: &str, msg: &str) {
    send(source, "event", name, None, msg)
}

pub fn metric(source: &str, name: &str, value: f64) {
    send(source, "metric", name, Some(value), "")
}

pub fn error(source: &str, name: &str, msg: &str) {
    send(source, "error", name, None, msg)
}

/// Background heartbeat every `secs` (min 10) carrying the process RSS in KiB
/// when /proc is available. No thread is started when pushing is disabled.
pub fn start_heartbeat(source: &'static str, secs: u64) {
    if !enabled() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("nerve-heartbeat".into())
        .stack_size(64 * 1024)
        .spawn(move || loop {
            let rss = std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("VmRSS:"))
                        .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
                });
            send(source, "heartbeat", "rss_kb", rss, "");
            std::thread::sleep(Duration::from_secs(secs.max(10)));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn url_parsing_is_loopback_only() {
        assert_eq!(
            parse_url("http://127.0.0.1:8670"),
            Some("127.0.0.1:8670".parse().unwrap())
        );
        assert_eq!(
            parse_url("http://127.0.0.1:8670/signal"),
            Some("127.0.0.1:8670".parse().unwrap())
        );
        assert_eq!(parse_url("https://127.0.0.1:8670"), None);
        assert_eq!(parse_url("http://192.0.2.1:8670"), None);
        assert_eq!(parse_url("garbage"), None);
    }

    #[test]
    fn body_is_json_escaped() {
        let b = body("hermes-rs", "error", "x", Some(1.5), "a\"b\nc");
        assert_eq!(
            b,
            r#"{"source":"hermes-rs","kind":"error","name":"x","value":1.5,"msg":"a\"b c"}"#
        );
        assert!(!body("s", "event", "n", Some(f64::NAN), "").contains("value"));
    }

    #[test]
    fn post_reaches_a_listener() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = vec![0u8; 1024];
            let n = s.read(&mut buf).unwrap();
            let _ = s.write_all(b"HTTP/1.0 202 Accepted\r\n\r\n");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        post(addr, &body("t", "heartbeat", "", None, "")).unwrap();
        let got = h.join().unwrap();
        assert!(got.starts_with("POST /signal HTTP/1.0"));
        assert!(got.contains(r#""kind":"heartbeat""#));
    }

    #[test]
    fn disabled_is_noop() {
        // NERVE_URL is not set in the test environment.
        if std::env::var("NERVE_URL").is_err() {
            assert!(!enabled());
            send("x", "event", "n", None, "m");
            assert_eq!(IN_FLIGHT.load(Ordering::SeqCst), 0);
        }
    }
}
