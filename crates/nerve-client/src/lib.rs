// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! nerve-client: optional, fire-and-forget signal push to the nerve
//! (`openmind-nerve`, live unit `galion-nerve`).
//!
//! Set `NERVE_URL=http://127.0.0.1:8670` in a service's environment to turn it
//! on; unset (the default) makes every call a no-op that costs one atomic load.
//!
//! Signals go into a **bounded queue** (`NERVE_QUEUE`, default 256, 8..=4096)
//! drained by ONE background thread that does `POST /signal` with 300 ms
//! connect/read/write timeouts. `send` never blocks: when the queue is full
//! the signal is dropped and counted ([`stats`]). The heartbeat carries the
//! drop/fail counters to the nerve, so lost signals are visible there too.
//! Loopback `http://` only (no TLS, no deps). Never fails the caller.
//!
//! ```no_run
//! nerve_client::event("hermes-rs", "started", "v0.1.0");
//! nerve_client::metric("hermes-rs", "requests_total", 42.0);
//! nerve_client::error("hermes-rs", "http_5xx", "POST /v1/chat/completions -> 502");
//! nerve_client::observe_request("hermes-rs", "GET /health", 200, std::time::Duration::from_millis(3));
//! nerve_client::start_heartbeat("hermes-rs", 60);
//! ```

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_millis(300);
pub const DEFAULT_QUEUE: usize = 256;
pub const DEFAULT_SLOW_MS: u64 = 1000;

/// Counters of one client (all monotonic).
#[derive(Debug, Default)]
pub struct Counters {
    /// Accepted into the queue.
    pub queued: AtomicU64,
    /// Delivered: the nerve answered with a 2xx status.
    pub sent: AtomicU64,
    /// Dropped because the queue was full (or the worker is gone).
    pub dropped: AtomicU64,
    /// Taken from the queue but the POST failed (nerve down or slow).
    pub failed: AtomicU64,
    /// The nerve answered but refused the signal (non-2xx, e.g. 429 per-source
    /// rate limit or 400 bad body). Not a delivery; no back-off (the nerve is up).
    pub rejected: AtomicU64,
}

/// A snapshot of [`Counters`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub queued: u64,
    pub sent: u64,
    pub dropped: u64,
    pub failed: u64,
    pub rejected: u64,
}

impl Counters {
    pub fn snapshot(&self) -> Stats {
        Stats {
            queued: self.queued.load(Ordering::Relaxed),
            sent: self.sent.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            rejected: self.rejected.load(Ordering::Relaxed),
        }
    }
}

/// One queue + one worker thread posting to `addr`.
pub struct Client {
    tx: SyncSender<String>,
    pub counters: Arc<Counters>,
    pub addr: SocketAddr,
}

impl Client {
    /// Start the worker. `cap` is the queue bound (min 1).
    pub fn start(addr: SocketAddr, cap: usize) -> std::io::Result<Client> {
        let (tx, rx) = sync_channel::<String>(cap.max(1));
        let counters = Arc::new(Counters::default());
        let c2 = counters.clone();
        std::thread::Builder::new()
            .name("nerve-push".into())
            .stack_size(128 * 1024)
            .spawn(move || worker(addr, rx, c2))?;
        Ok(Client { tx, counters, addr })
    }

    /// Queue one JSON body. Never blocks; returns false when dropped.
    pub fn push(&self, body: String) -> bool {
        match self.tx.try_send(body) {
            Ok(()) => {
                self.counters.queued.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
}

fn worker(addr: SocketAddr, rx: Receiver<String>, c: Arc<Counters>) {
    // After a failure, skip connecting for a short back-off so a dead nerve
    // costs one refused connect per 2 s, not one per signal.
    let mut down_until: Option<Instant> = None;
    for body in rx {
        if down_until.is_some_and(|t| Instant::now() < t) {
            c.failed.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        match post(addr, &body) {
            Ok(code) if (200..300).contains(&code) => {
                down_until = None;
                c.sent.fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => {
                down_until = None;
                c.rejected.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                down_until = Some(Instant::now() + Duration::from_secs(2));
                c.failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Status code from the first bytes of an HTTP reply (`HTTP/1.x NNN ...`); 0 if absent.
pub fn status_of(head: &[u8]) -> u16 {
    let s = String::from_utf8_lossy(head);
    let line = s.lines().next().unwrap_or("");
    if !line.starts_with("HTTP/") {
        return 0;
    }
    line.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

/// Parse `http://host:port[/...]` into a loopback socket address. Non-loopback
/// targets are refused so a mis-set env var cannot ship signals off the box.
pub fn parse_url(url: &str) -> Option<SocketAddr> {
    let rest = url.trim().strip_prefix("http://")?;
    let hostport = rest.split('/').next()?;
    let addr = hostport.to_socket_addrs().ok()?.next()?;
    addr.ip().is_loopback().then_some(addr)
}

fn env_num(k: &str, d: u64) -> u64 {
    std::env::var(k)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(d)
}

static CLIENT: OnceLock<Option<Client>> = OnceLock::new();
static SLOW_MS: OnceLock<u64> = OnceLock::new();

fn client() -> Option<&'static Client> {
    CLIENT
        .get_or_init(|| {
            let addr = std::env::var("NERVE_URL")
                .ok()
                .and_then(|u| parse_url(&u))?;
            let cap = env_num("NERVE_QUEUE", DEFAULT_QUEUE as u64).clamp(8, 4096) as usize;
            Client::start(addr, cap).ok()
        })
        .as_ref()
}

/// Is pushing enabled (NERVE_URL set to a loopback http URL)?
pub fn enabled() -> bool {
    client().is_some()
}

/// Counters of the process-wide client (all zero when disabled).
pub fn stats() -> Stats {
    client().map(|c| c.counters.snapshot()).unwrap_or_default()
}

/// Requests slower than this (ms) are reported: `NERVE_SLOW_MS`, default 1000.
pub fn slow_ms() -> u64 {
    *SLOW_MS.get_or_init(|| env_num("NERVE_SLOW_MS", DEFAULT_SLOW_MS).max(1))
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

/// POST one body; the HTTP status of the reply (0 when it has no status line).
fn post(addr: SocketAddr, body: &str) -> std::io::Result<u16> {
    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    s.set_write_timeout(Some(TIMEOUT))?;
    s.set_read_timeout(Some(TIMEOUT))?;
    write!(
        s,
        "POST /signal HTTP/1.0\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    // The status line can arrive in pieces (the nerve writes its reply with
    // several small writes): read until a newline, 64 bytes or EOF.
    let mut b = [0u8; 64];
    let mut n = 0;
    while n < b.len() && !b[..n].contains(&b'\n') {
        match s.read(&mut b[n..])? {
            0 => break,
            k => n += k,
        }
    }
    if n == 0 {
        return Err(std::io::Error::other("empty reply"));
    }
    Ok(status_of(&b[..n]))
}

/// Queue one signal. Returns immediately; no-op when disabled, counted drop
/// when the queue is full.
pub fn send(source: &str, kind: &str, name: &str, value: Option<f64>, msg: &str) {
    if let Some(c) = client() {
        c.push(body(source, kind, name, value, msg));
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

/// Minimum gap between two pushes of the same cap event (per source+name).
pub const CAP_EVERY: Duration = Duration::from_secs(5);
static CAP_LAST: Mutex<Vec<(String, Instant, u64)>> = Mutex::new(Vec::new());

/// Throttle state for [`cap_hit`]: `Some(suppressed_since_last)` when this
/// hit should be pushed now, `None` when it is folded into the next one.
fn cap_gate(key: &str, now: Instant, every: Duration) -> Option<u64> {
    let mut v = CAP_LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = v.iter_mut().find(|e| e.0 == key) {
        if now.duration_since(e.1) < every {
            e.2 += 1;
            return None;
        }
        let n = e.2;
        e.1 = now;
        e.2 = 0;
        return Some(n);
    }
    if v.len() < 64 {
        v.push((key.to_string(), now, 0));
    }
    Some(0)
}

/// A cap was hit (spend cap, connection cap): event `name` (e.g.
/// `spend_cap`). At most one push per [`CAP_EVERY`] per source+name; the
/// hits in between are counted into the next push's msg and value.
pub fn cap_hit(source: &str, name: &str, msg: &str) {
    if !enabled() {
        return;
    }
    if let Some(n) = cap_gate(&format!("{source}/{name}"), Instant::now(), CAP_EVERY) {
        let m = if n > 0 {
            format!("{msg} (+{n} more since the last report)")
        } else {
            msg.to_string()
        };
        send(source, "event", name, Some((n + 1) as f64), &m);
    }
}

/// What [`observe_request`] reports for one finished request (pure; tested).
pub fn classify(status: u16, elapsed: Duration, slow_ms: u64) -> (bool, bool) {
    (status >= 500, elapsed.as_millis() as u64 >= slow_ms)
}

/// Call after each request: 5xx -> error `http_5xx`; slower than
/// [`slow_ms`] -> event `slow_request` with the duration in ms as value.
pub fn observe_request(source: &str, what: &str, status: u16, elapsed: Duration) {
    if !enabled() {
        return;
    }
    let (err, slow) = classify(status, elapsed, slow_ms());
    if err {
        error(source, "http_5xx", &format!("{what} -> {status}"));
    }
    if slow {
        let ms = elapsed.as_millis() as f64;
        send(
            source,
            "event",
            "slow_request",
            Some(ms),
            &format!(
                "{what} -> {status} in {ms:.0} ms (threshold {} ms)",
                slow_ms()
            ),
        );
    }
}

/// Background heartbeat every `secs` (or `NERVE_HEARTBEAT_S`; min 10) carrying the process RSS in KiB
/// (when /proc is available) and the client's own queue counters, so the
/// nerve sees dropped and failed signals. No thread when disabled.
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
            let st = stats();
            send(
                source,
                "heartbeat",
                "rss_kb",
                rss,
                &format!(
                    "queued={} sent={} dropped={} failed={} rejected={}",
                    st.queued, st.sent, st.dropped, st.failed, st.rejected
                ),
            );
            send(
                source,
                "metric",
                "nerve_client_dropped",
                Some(st.dropped as f64),
                "",
            );
            std::thread::sleep(Duration::from_secs(
                env_num("NERVE_HEARTBEAT_S", secs).max(10),
            ));
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

    fn read_req(s: &mut TcpStream) -> String {
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        while !got.ends_with(b"}") {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&got).into_owned()
    }

    #[test]
    fn post_reaches_a_listener() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let got = read_req(&mut s);
            let _ = s.write_all(b"HTTP/1.0 202 Accepted\r\n\r\n");
            got
        });
        assert_eq!(
            post(addr, &body("t", "heartbeat", "", None, "")).unwrap(),
            202
        );
        let got = h.join().unwrap();
        assert!(got.starts_with("POST /signal HTTP/1.0"));
        assert!(got.contains(r#""kind":"heartbeat""#));
    }

    #[test]
    fn queue_delivers_in_order() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let mut v = vec![];
            for _ in 0..3 {
                let (mut s, _) = l.accept().unwrap();
                v.push(read_req(&mut s));
                let _ = s.write_all(b"HTTP/1.0 202 Accepted\r\n\r\n");
            }
            v
        });
        let c = Client::start(addr, 16).unwrap();
        for i in 0..3 {
            assert!(c.push(body("t", "event", &format!("e{i}"), None, "")));
        }
        let got = h.join().unwrap();
        assert!(got[0].contains("\"e0\"") && got[2].contains("\"e2\""));
        let t = Instant::now();
        while c.counters.snapshot().sent < 3 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(c.counters.snapshot().sent, 3);
    }

    #[test]
    fn full_queue_drops_and_counts_without_blocking() {
        // A listener that accepts nothing: the worker blocks in its first POST
        // (until the 300 ms timeout), so the queue fills.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let c = Client::start(addr, 4).unwrap();
        let t = Instant::now();
        let mut ok = 0;
        for i in 0..1000 {
            if c.push(body("t", "event", &format!("e{i}"), None, "")) {
                ok += 1;
            }
        }
        assert!(
            t.elapsed() < Duration::from_millis(200),
            "push must not block"
        );
        let s = c.counters.snapshot();
        assert!(ok <= 5 + 1, "queue bound respected, accepted {ok}");
        assert_eq!(s.queued + s.dropped, 1000);
        assert!(s.dropped >= 994);
        drop(l);
    }

    #[test]
    fn dead_nerve_counts_failures() {
        let addr = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        }; // closed: connects are refused
        let c = Client::start(addr, 8).unwrap();
        for _ in 0..5 {
            c.push(body("t", "event", "x", None, ""));
        }
        let t = Instant::now();
        while c.counters.snapshot().failed < 5 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let s = c.counters.snapshot();
        assert_eq!((s.failed, s.sent), (5, 0));
    }

    #[test]
    fn status_of_reads_the_status_line() {
        assert_eq!(status_of(b"HTTP/1.1 202 Accepted\r\n"), 202);
        assert_eq!(status_of(b"HTTP/1.0 429 Too Many"), 429);
        assert_eq!(status_of(b"HTTP/1.1"), 0);
        assert_eq!(status_of(b"garbage 200"), 0);
        assert_eq!(status_of(b""), 0);
    }

    #[test]
    fn non_2xx_reply_is_rejected_not_sent() {
        // T3.6: a 429 (per-source rate limit) or 400 used to count as `sent`.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            for reply in [
                &b"HTTP/1.1 429 Too Many Requests\r\n\r\n"[..],
                b"HTTP/1.1 400 Bad Request\r\n\r\n",
                b"HTTP/1.1 202 Accepted\r\n\r\n",
            ] {
                let (mut s, _) = l.accept().unwrap();
                let _ = read_req(&mut s);
                let _ = s.write_all(reply);
            }
        });
        let c = Client::start(addr, 8).unwrap();
        for i in 0..3 {
            assert!(c.push(body("t", "event", &format!("e{i}"), None, "")));
        }
        h.join().unwrap();
        let t = Instant::now();
        while c.counters.snapshot().sent + c.counters.snapshot().rejected < 3
            && t.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let s = c.counters.snapshot();
        assert_eq!((s.sent, s.rejected, s.failed), (1, 2, 0), "{s:?}");
    }

    #[test]
    fn classify_5xx_and_slow() {
        assert_eq!(
            classify(200, Duration::from_millis(5), 1000),
            (false, false)
        );
        assert_eq!(classify(502, Duration::from_millis(5), 1000), (true, false));
        assert_eq!(
            classify(200, Duration::from_millis(1500), 1000),
            (false, true)
        );
    }

    #[test]
    fn cap_gate_throttles_and_counts() {
        let t = Instant::now();
        let e = Duration::from_secs(5);
        assert_eq!(cap_gate("t/cap", t, e), Some(0));
        assert_eq!(cap_gate("t/cap", t + Duration::from_secs(1), e), None);
        assert_eq!(cap_gate("t/cap", t + Duration::from_secs(2), e), None);
        assert_eq!(cap_gate("t/other", t, e), Some(0));
        assert_eq!(cap_gate("t/cap", t + Duration::from_secs(6), e), Some(2));
    }

    #[test]
    fn disabled_is_noop() {
        // NERVE_URL is not set in the test environment.
        if std::env::var("NERVE_URL").is_err() {
            assert!(!enabled());
            send("x", "event", "n", None, "m");
            observe_request("x", "GET /", 500, Duration::from_secs(9));
            assert_eq!(stats(), Stats::default());
        }
    }
}
