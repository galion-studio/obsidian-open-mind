// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Memory provider: the Rust port of the Python Hermes `memory_tencentdb`
//! plugin (hermes-plugin/memory/memory_tencentdb in the memory-tencentdb
//! package). It talks to the memory gateway (openmind-memory, or the old
//! Node gateway) on loopback:
//!
//! * before a turn: `POST /recall {query, session_key, user_id}`; a non-empty
//!   `context` is injected into the system prompt as
//!   `## memory-tencentdb Memory\n<context>` (same header as the plugin);
//! * after a turn: `POST /capture {user_content, assistant_content,
//!   session_key, user_id}` on a background thread (at most 4 in flight);
//! * at session close: `POST /session/end {session_key, user_id}` once a
//!   session has been idle for `idle_end_secs` (hermes-rs has no explicit
//!   session close on the API server), and for every open session on
//!   [`MemoryProvider::shutdown`].
//!
//! Env (same names as the plugin): `MEMORY_TENCENTDB_GATEWAY_HOST`
//! (127.0.0.1), `MEMORY_TENCENTDB_GATEWAY_PORT` (8420), bearer from
//! `MEMORY_TENCENTDB_GATEWAY_API_KEY`, falling back to `TDAI_GATEWAY_API_KEY`.
//! The key is read once and never logged. Every failure is non-fatal: the
//! turn just runs without memory. A circuit breaker (5 failures -> 60 s
//! pause) keeps a dead gateway from adding latency to every turn.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PROVIDER_NAME: &str = "memory_tencentdb";
pub const CONTEXT_HEADER: &str = "## memory-tencentdb Memory";
pub const KEY_ENVS: [&str; 2] = ["MEMORY_TENCENTDB_GATEWAY_API_KEY", "TDAI_GATEWAY_API_KEY"];

const BREAKER_THRESHOLD: usize = 5;
const BREAKER_COOLDOWN: Duration = Duration::from_secs(60);
const MAX_INFLIGHT_CAPTURES: usize = 4;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Memory settings resolved from config.yaml (`memory.provider`) and env.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryCfg {
    pub host: String,
    pub port: u16,
    pub user_id: String,
    pub recall_timeout: Duration,
    pub capture_timeout: Duration,
    pub idle_end_secs: u64,
}

impl Default for MemoryCfg {
    fn default() -> Self {
        MemoryCfg {
            host: "127.0.0.1".into(),
            port: 8420,
            user_id: "default".into(),
            recall_timeout: Duration::from_millis(2000),
            capture_timeout: Duration::from_millis(5000),
            idle_end_secs: 1800,
        }
    }
}

impl MemoryCfg {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Read the bearer key by env name (plugin order). Returns the value and
/// the name it came from; the value must never be printed.
pub fn resolve_key(env: &dyn Fn(&str) -> Option<String>) -> Option<(String, &'static str)> {
    KEY_ENVS.iter().find_map(|n| {
        env(n)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(|v| (v, *n))
    })
}

/// Minimal HTTP/1.1 JSON POST to a loopback gateway. Plain http only.
pub fn post_json(
    host: &str,
    port: u16,
    path: &str,
    key: Option<&str>,
    body: &Value,
    timeout: Duration,
) -> Result<(u16, Value), String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("resolve {host}: no address"))?;
    let mut s = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect: {e}"))?;
    let deadline = Instant::now() + timeout;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    let b = body.to_string();
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n",
        b.len()
    );
    if let Some(k) = key {
        head.push_str("Authorization: Bearer ");
        head.push_str(k);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())
        .and_then(|_| s.write_all(b.as_bytes()))
        .map_err(|e| format!("write: {e}"))?;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        if Instant::now() > deadline {
            return Err("timeout".into());
        }
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                if out.len() > MAX_RESPONSE_BYTES {
                    return Err("response too large".into());
                }
                if response_complete(&out) {
                    break;
                }
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    parse_response(&out)
}

fn split_head(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    raw.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (&raw[..i], &raw[i + 4..]))
}

fn header_val<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn response_complete(raw: &[u8]) -> bool {
    let Some((head, body)) = split_head(raw) else {
        return false;
    };
    let head = String::from_utf8_lossy(head);
    match header_val(&head, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        Some(n) => body.len() >= n,
        None => false,
    }
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(i) = body.windows(2).position(|w| w == b"\r\n") {
        let size_txt = String::from_utf8_lossy(&body[..i]);
        let size =
            usize::from_str_radix(size_txt.split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        body = &body[i + 2..];
        if size == 0 || body.len() < size {
            out.extend_from_slice(&body[..size.min(body.len())]);
            break;
        }
        out.extend_from_slice(&body[..size]);
        body = body.get(size + 2..).unwrap_or(&[]);
    }
    out
}

fn parse_response(raw: &[u8]) -> Result<(u16, Value), String> {
    let (head, body) = split_head(raw).ok_or("malformed response")?;
    let head = String::from_utf8_lossy(head).to_string();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("malformed status line")?;
    let body = if header_val(&head, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        dechunk(body)
    } else if let Some(n) = header_val(&head, "content-length").and_then(|v| v.parse().ok()) {
        body[..body.len().min(n)].to_vec()
    } else {
        body.to_vec()
    };
    let v = if body.iter().all(|b| b.is_ascii_whitespace()) {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };
    Ok((status, v))
}

#[derive(Default)]
struct Breaker {
    failures: usize,
    open_until: Option<Instant>,
}

/// Counters for /health/detailed and tests.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MemoryStats {
    pub recalls: usize,
    pub recall_hits: usize,
    pub captures: usize,
    pub session_ends: usize,
    pub failures: usize,
    pub skipped_breaker: usize,
    pub dropped_captures: usize,
}

pub struct MemoryProvider {
    pub cfg: MemoryCfg,
    key: Option<String>,
    pub key_env: Option<&'static str>,
    breaker: Mutex<Breaker>,
    inflight: Arc<AtomicUsize>,
    sessions: Mutex<HashMap<String, Instant>>,
    stats: Mutex<MemoryStats>,
    log: Box<dyn Fn(&str) + Send + Sync>,
}

impl MemoryProvider {
    pub fn new(cfg: MemoryCfg, env: &dyn Fn(&str) -> Option<String>) -> Arc<Self> {
        let k = resolve_key(env);
        Arc::new(MemoryProvider {
            cfg,
            key_env: k.as_ref().map(|(_, n)| *n),
            key: k.map(|(v, _)| v),
            breaker: Mutex::new(Breaker::default()),
            inflight: Arc::new(AtomicUsize::new(0)),
            sessions: Mutex::new(HashMap::new()),
            stats: Mutex::new(MemoryStats::default()),
            log: Box::new(|m| {
                eprintln!(
                    "{} hermes-gateway memory: {m}",
                    crate::time::zagreb_rfc3339(crate::time::now_secs())
                )
            }),
        })
    }

    pub fn stats(&self) -> MemoryStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn status_json(&self) -> Value {
        let s = self.stats();
        json!({"provider": PROVIDER_NAME, "url": self.cfg.base_url(), "auth": self.key.is_some(),
               "breaker_open": self.breaker_open(), "recalls": s.recalls, "recall_hits": s.recall_hits,
               "captures": s.captures, "session_ends": s.session_ends, "failures": s.failures,
               "open_sessions": self.sessions.lock().map(|m| m.len()).unwrap_or(0)})
    }

    fn bump(&self, f: impl FnOnce(&mut MemoryStats)) {
        if let Ok(mut s) = self.stats.lock() {
            f(&mut s);
        }
    }

    fn breaker_open(&self) -> bool {
        let Ok(mut b) = self.breaker.lock() else {
            return false;
        };
        match b.open_until {
            Some(t) if Instant::now() < t => true,
            Some(_) => {
                b.open_until = None;
                b.failures = 0;
                false
            }
            None => false,
        }
    }

    fn record(&self, ok: bool) {
        if let Ok(mut b) = self.breaker.lock() {
            if ok {
                b.failures = 0;
                b.open_until = None;
            } else {
                b.failures += 1;
                if b.failures >= BREAKER_THRESHOLD && b.open_until.is_none() {
                    b.open_until = Some(Instant::now() + BREAKER_COOLDOWN);
                    (self.log)("gateway failing; pausing memory calls for 60s");
                }
            }
        }
        if !ok {
            self.bump(|s| s.failures += 1);
        }
    }

    fn call(&self, path: &str, body: &Value, timeout: Duration) -> Result<Value, String> {
        if self.breaker_open() {
            self.bump(|s| s.skipped_breaker += 1);
            return Err("circuit breaker open".into());
        }
        let r = post_json(
            &self.cfg.host,
            self.cfg.port,
            path,
            self.key.as_deref(),
            body,
            timeout,
        );
        match r {
            Ok((200..=299, v)) => {
                self.record(true);
                Ok(v)
            }
            Ok((code, _)) => {
                self.record(false);
                Err(format!("{path} returned HTTP {code}"))
            }
            Err(e) => {
                self.record(false);
                Err(format!("{path}: {e}"))
            }
        }
    }

    fn body_with_user(&self, mut v: Value) -> Value {
        if !self.cfg.user_id.is_empty() {
            v["user_id"] = json!(self.cfg.user_id);
        }
        v
    }

    fn touch(&self, session_key: &str) {
        if let Ok(mut m) = self.sessions.lock() {
            m.insert(session_key.to_string(), Instant::now());
        }
    }

    /// Plugin `prefetch`: the block to inject, or None. Never fails.
    pub fn recall(&self, query: &str, session_key: &str) -> Option<String> {
        if query.trim().is_empty() || session_key.is_empty() {
            return None;
        }
        self.touch(session_key);
        self.bump(|s| s.recalls += 1);
        let body = self.body_with_user(json!({"query": query, "session_key": session_key}));
        match self.call("/recall", &body, self.cfg.recall_timeout) {
            Ok(v) => {
                let ctx = v["context"].as_str().unwrap_or("").trim().to_string();
                if ctx.is_empty() {
                    None
                } else {
                    self.bump(|s| s.recall_hits += 1);
                    Some(format!("{CONTEXT_HEADER}\n{ctx}"))
                }
            }
            Err(e) => {
                (self.log)(&format!("recall failed (non-fatal): {e}"));
                None
            }
        }
    }

    /// Plugin `sync_turn`: capture on a background thread. Returns the
    /// thread handle (tests join it); None when skipped.
    pub fn capture_async(
        self: &Arc<Self>,
        user: &str,
        assistant: &str,
        session_key: &str,
    ) -> Option<std::thread::JoinHandle<()>> {
        if user.trim().is_empty() || assistant.trim().is_empty() || session_key.is_empty() {
            return None;
        }
        self.touch(session_key);
        if self.inflight.fetch_add(1, Ordering::SeqCst) >= MAX_INFLIGHT_CAPTURES {
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            self.bump(|s| s.dropped_captures += 1);
            (self.log)("capture backlog full; dropping one turn (gateway slow?)");
            return None;
        }
        let me = self.clone();
        let body = self.body_with_user(json!({
            "user_content": user, "assistant_content": assistant, "session_key": session_key,
        }));
        let spawned = std::thread::Builder::new()
            .name("memory-capture".into())
            .spawn(move || {
                match me.call("/capture", &body, me.cfg.capture_timeout) {
                    Ok(_) => me.bump(|s| s.captures += 1),
                    Err(e) => (me.log)(&format!("capture failed (non-fatal): {e}")),
                }
                me.inflight.fetch_sub(1, Ordering::SeqCst);
            });
        match spawned {
            Ok(h) => Some(h),
            Err(_) => {
                self.inflight.fetch_sub(1, Ordering::SeqCst);
                None
            }
        }
    }

    /// Plugin `on_session_end` / `shutdown`.
    pub fn end_session(&self, session_key: &str) -> bool {
        if let Ok(mut m) = self.sessions.lock() {
            m.remove(session_key);
        }
        let body = self.body_with_user(json!({"session_key": session_key}));
        match self.call("/session/end", &body, self.cfg.capture_timeout) {
            Ok(_) => {
                self.bump(|s| s.session_ends += 1);
                true
            }
            Err(e) => {
                (self.log)(&format!("session/end failed (non-fatal): {e}"));
                false
            }
        }
    }

    /// End every session idle for at least `idle`. Returns how many ended.
    pub fn end_idle(&self, idle: Duration) -> usize {
        let now = Instant::now();
        let due: Vec<String> = match self.sessions.lock() {
            Ok(m) => m
                .iter()
                .filter(|(_, t)| now.duration_since(**t) >= idle)
                .map(|(k, _)| k.clone())
                .collect(),
            Err(_) => return 0,
        };
        due.iter().filter(|k| self.end_session(k)).count()
    }

    /// Background reaper: session/end for idle sessions, once a minute.
    pub fn spawn_reaper(self: &Arc<Self>) {
        if self.cfg.idle_end_secs == 0 {
            return;
        }
        let me = self.clone();
        let idle = Duration::from_secs(self.cfg.idle_end_secs);
        let tick = Duration::from_secs(60.min(self.cfg.idle_end_secs.max(1)));
        let _ = std::thread::Builder::new()
            .name("memory-reaper".into())
            .spawn(move || loop {
                std::thread::sleep(tick);
                me.end_idle(idle);
            });
    }

    /// session/end for every open session (process shutdown).
    pub fn shutdown(&self) -> usize {
        self.end_idle(Duration::ZERO)
    }

    /// Startup probe (`/health`, no auth). Informational only.
    pub fn probe(&self) -> Result<(), String> {
        let addr = (self.cfg.host.as_str(), self.cfg.port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("no address")?;
        let mut s =
            TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(2))).ok();
        s.write_all(
            format!(
                "GET /health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                self.cfg.host
            )
            .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let _ = s.take(65536).read_to_end(&mut out);
        match parse_response(&out)? {
            (200, _) => Ok(()),
            (c, _) => Err(format!("HTTP {c}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_resolution_order_and_blank() {
        let env = |k: &str| match k {
            "MEMORY_TENCENTDB_GATEWAY_API_KEY" => Some("  ".to_string()),
            "TDAI_GATEWAY_API_KEY" => Some(" abc \n".to_string()),
            _ => None,
        };
        assert_eq!(
            resolve_key(&env),
            Some(("abc".into(), "TDAI_GATEWAY_API_KEY"))
        );
        let env2 = |k: &str| (k == "MEMORY_TENCENTDB_GATEWAY_API_KEY").then(|| "x".to_string());
        assert_eq!(
            resolve_key(&env2).unwrap().1,
            "MEMORY_TENCENTDB_GATEWAY_API_KEY"
        );
        assert_eq!(resolve_key(&|_| None), None);
    }

    #[test]
    fn parses_length_and_chunked() {
        let r = b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"context\":1}";
        assert!(response_complete(r));
        assert_eq!(parse_response(r).unwrap(), (200, json!({"context": 1})));
        let c = b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"a\":\r\n3\r\n 2}\r\n0\r\n\r\n";
        assert_eq!(parse_response(c).unwrap(), (201, json!({"a": 2})));
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn dead_gateway_is_non_fatal_and_trips_breaker() {
        // Port 1 on loopback: connection refused immediately.
        let cfg = MemoryCfg {
            port: 1,
            recall_timeout: Duration::from_millis(200),
            capture_timeout: Duration::from_millis(200),
            ..MemoryCfg::default()
        };
        let m = MemoryProvider::new(cfg, &|_| None);
        for _ in 0..7 {
            assert_eq!(m.recall("hi", "s1"), None);
        }
        let s = m.stats();
        assert_eq!(s.failures, BREAKER_THRESHOLD);
        assert_eq!(s.skipped_breaker, 2);
        assert!(m.status_json()["breaker_open"].as_bool().unwrap());
    }
}
