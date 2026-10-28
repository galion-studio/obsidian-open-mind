// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The mock HTTP server. Blocking std networking, one thread per connection,
//! one request per connection. It only answers from fixtures; it never opens
//! an outbound connection.
//!
//! Admin endpoints (reserved prefix `/__bench/`):
//! * `GET  /__bench/health`  -> `{"ok":true,"fixtures":N,"at":"<Zagreb>"}`
//! * `GET  /__bench/journal` -> every hit so far, oldest first
//! * `POST /__bench/reset`   -> clear journal and per-fixture hit counters

use crate::fixture::{FixtureSet, ADMIN_PREFIX};
use crate::http::{read_request, write_response, Request};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// One request the mock answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Europe/Zagreb RFC 3339 receive time.
    pub at: String,
    /// Request method.
    pub method: String,
    /// Request path (no query).
    pub path: String,
    /// Raw query string.
    pub query: String,
    /// Request body.
    pub body: String,
    /// The fixture that answered, or `None` for the 404 fallback.
    pub fixture: Option<String>,
    /// Status sent.
    pub status: u16,
}

impl Hit {
    fn to_json(&self) -> Value {
        json!({
            "at": self.at, "method": self.method, "path": self.path, "query": self.query,
            "body": self.body, "fixture": self.fixture, "status": self.status,
        })
    }
}

#[derive(Default)]
struct State {
    counts: HashMap<String, u32>,
    journal: Vec<Hit>,
}

struct Shared {
    fixtures: FixtureSet,
    state: Mutex<State>,
    stop: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A panicking handler thread must not wedge the whole benchmark.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// A running mock server. Dropping it shuts it down.
pub struct MockServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl MockServer {
    /// Start on `bind` (e.g. `127.0.0.1:19500`, or `127.0.0.1:0` for any free
    /// port). Refuses non-loopback addresses; use [`MockServer::start_any`] if
    /// you really mean it.
    pub fn start(bind: impl ToSocketAddrs, fixtures: FixtureSet) -> io::Result<Self> {
        Self::start_inner(bind, fixtures, false)
    }

    /// Like [`MockServer::start`] but allows non-loopback binds.
    pub fn start_any(bind: impl ToSocketAddrs, fixtures: FixtureSet) -> io::Result<Self> {
        Self::start_inner(bind, fixtures, true)
    }

    fn start_inner(bind: impl ToSocketAddrs, fixtures: FixtureSet, any: bool) -> io::Result<Self> {
        let listener = TcpListener::bind(bind)?;
        let addr = listener.local_addr()?;
        if !any && !addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing non-loopback bind {addr}; the mock is local-only by default"),
            ));
        }
        let shared = Arc::new(Shared {
            fixtures,
            state: Mutex::new(State::default()),
            stop: AtomicBool::new(false),
        });
        let s2 = Arc::clone(&shared);
        let accept = std::thread::Builder::new()
            .name("galion-bench-accept".into())
            .spawn(move || accept_loop(listener, s2))?;
        Ok(Self {
            addr,
            shared,
            accept: Some(accept),
        })
    }

    /// The bound address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://<addr>` base URL.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Snapshot of the journal, oldest first.
    pub fn hits(&self) -> Vec<Hit> {
        self.shared.lock().journal.clone()
    }

    /// How many times fixture `id` has answered.
    pub fn count(&self, id: &str) -> u32 {
        self.shared.lock().counts.get(id).copied().unwrap_or(0)
    }

    /// Requests no fixture matched (answered with the 404 fallback).
    pub fn unmatched(&self) -> Vec<Hit> {
        self.hits()
            .into_iter()
            .filter(|h| h.fixture.is_none())
            .collect()
    }

    /// Clear the journal and hit counters.
    pub fn reset(&self) {
        *self.shared.lock() = State::default();
    }

    /// Stop accepting and join the accept thread. Idempotent.
    pub fn shutdown(&mut self) {
        if self.shared.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        // Wake the blocking accept() with a throwaway local connection.
        let mut wake = self.addr;
        if wake.ip().is_unspecified() {
            wake.set_ip(match wake.ip() {
                IpAddr::V4(_) => IpAddr::from([127, 0, 0, 1]),
                IpAddr::V6(_) => IpAddr::from([0u16, 0, 0, 0, 0, 0, 0, 1]),
            });
        }
        let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
    }

    /// Block the calling thread until the server stops (used by the binary).
    pub fn wait(mut self) {
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for conn in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = conn else { continue };
        let s = Arc::clone(&shared);
        let _ = std::thread::Builder::new()
            .name("galion-bench-conn".into())
            .spawn(move || handle(stream, &s));
    }
}

fn handle(stream: TcpStream, shared: &Shared) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let req = match read_request(&stream) {
        Ok(r) => r,
        Err(e) => {
            let body =
                json!({"error": "bad_request", "detail": e.to_string(), "at": crate::now_zagreb()});
            let _ = write_response(&stream, 400, &json_ct(), &body.to_string());
            return;
        }
    };
    if shared.stop.load(Ordering::SeqCst) {
        return;
    }
    let (status, headers, body, delay) = answer(&req, shared);
    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay));
    }
    let _ = write_response(&stream, status, &headers, &body);
}

fn json_ct() -> Vec<(String, String)> {
    vec![("Content-Type".into(), "application/json".into())]
}

type Answer = (u16, Vec<(String, String)>, String, u64);

fn answer(req: &Request, shared: &Shared) -> Answer {
    let now = crate::now_zagreb();
    if let Some(admin) = req.path.strip_prefix(ADMIN_PREFIX) {
        return admin_answer(admin, req, shared, &now);
    }
    let mut st = shared.lock();
    let pick = shared.fixtures.fixtures.iter().find(|f| {
        let used = st.counts.get(&f.id).copied().unwrap_or(0);
        f.times.is_none_or(|t| used < t)
            && f.after
                .as_ref()
                .is_none_or(|a| st.counts.get(a).copied().unwrap_or(0) > 0)
            && f.matches(req)
    });
    let mut hit = Hit {
        at: now.clone(),
        method: req.method.clone(),
        path: req.path.clone(),
        query: req.query.clone(),
        body: req.body.clone(),
        fixture: None,
        status: 404,
    };
    let out = match pick {
        Some(f) => {
            let n = st.counts.entry(f.id.clone()).or_insert(0);
            *n += 1;
            let (body, ct) = f.render_body(*n, &now);
            let mut headers = f.response_headers.clone();
            if !headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                headers.push(("Content-Type".into(), ct.into()));
            }
            headers.push(("X-Bench-Fixture".into(), f.id.clone()));
            hit.fixture = Some(f.id.clone());
            hit.status = f.status;
            (f.status, headers, body, f.delay_ms)
        }
        None => {
            let body = json!({
                "error": "no_fixture", "method": req.method, "path": req.path,
                "query": req.query, "at": now,
            });
            (404, json_ct(), body.to_string(), 0)
        }
    };
    st.journal.push(hit);
    out
}

fn admin_answer(admin: &str, req: &Request, shared: &Shared, now: &str) -> Answer {
    let ok = |v: Value| (200, json_ct(), v.to_string(), 0);
    match (req.method.as_str(), admin) {
        ("GET", "health") => ok(json!({
            "ok": true, "fixtures": shared.fixtures.fixtures.len(), "at": now,
        })),
        ("GET", "journal") => {
            let st = shared.lock();
            ok(json!({
                "at": now,
                "hits": st.journal.iter().map(Hit::to_json).collect::<Vec<_>>(),
            }))
        }
        ("POST", "reset") => {
            *shared.lock() = State::default();
            ok(json!({"ok": true, "at": now}))
        }
        _ => (
            404,
            json_ct(),
            json!({"error": "unknown_admin_endpoint", "path": req.path, "at": now}).to_string(),
            0,
        ),
    }
}
