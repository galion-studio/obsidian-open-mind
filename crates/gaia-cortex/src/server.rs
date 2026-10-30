// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Loopback API.
//!
//! | route | |
//! |---|---|
//! | `GET /health` | `{ok, mode, llm_enabled, ...}` |
//! | `GET /stats` | counters: events in, incidents, decisions, actions simulated/executed, alerts, escalations, llm calls, spend |
//! | `GET /incidents` | open + recently resolved incidents, per-unit circuit/caps |
//! | `GET /decisions?n=N` | newest decisions first |
//! | `GET /alerts?n=N` | newest alerts first |
//! | `GET /escalations` | open escalate-to-human items |
//! | `POST /escalations/ack?id=ID` | acknowledge one item |
//! | `POST /v1/events` | ingest: one B33 event, a JSON array, or NDJSON |
//!
//! Reads never wait for the decider (slot 236). Since slot 247 the decider
//! holds the engine lock only for in-memory work (plan, commit), never
//! across nerve I/O, so reads are fresh; while a batch is in flight (e.g.
//! a POST /actions to a hung nerve, at most one 3 s timeout per batch) they
//! carry `busy: true` and `x-cortex-busy: 1`. If the lock is ever held, every GET answers from the snapshot published after the last
//! batch with `busy: true` and `snapshot_age_ms` (objects) and the headers
//! `x-cortex-busy: 1` / `x-cortex-snapshot-age-ms` (all routes). Ingest
//! never takes the engine lock. The one write that needs it,
//! `POST /escalations/ack`, waits at most [`ACK_WAIT`] and then answers 503
//! with `retry-after: 1`.

use crate::engine::{now_secs, ts, Cortex};
use crate::snapshot::{mark_busy, Route, Snapshot};
use galion_common::http::{Request, Response};
use nerve_cortex_contract::{parse_events, Event, CORTEX_INGEST_PATH};
use serde_json::{json, Value};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

/// Longest `POST /escalations/ack` waits for the engine before a 503.
pub const ACK_WAIT: Duration = Duration::from_secs(2);

/// What the API handler needs: the engine, the decider queue and the read
/// snapshot (shared with the engine, outside its lock).
pub struct Api {
    pub cx: Arc<Mutex<Cortex>>,
    pub tx: Mutex<Sender<Vec<Event>>>,
    pub snap: Arc<Snapshot>,
    /// Nerve reachability for `/health` from the snapshot path (slot 347):
    /// contact, configured, stale seconds.
    pub nerve: (Arc<crate::contact::NerveContact>, bool, u64),
}

impl Api {
    pub fn new(cx: Arc<Mutex<Cortex>>, tx: Sender<Vec<Event>>) -> Api {
        let (snap, nerve) = {
            let g = cx.lock().unwrap_or_else(|e| e.into_inner());
            (
                g.snap.clone(),
                (
                    g.contact.clone(),
                    g.cfg.nerve_url.is_some(),
                    g.cfg.nerve_stale_eff(),
                ),
            )
        };
        Api {
            cx,
            tx: Mutex::new(tx),
            snap,
            nerve,
        }
    }
}

fn q<'a>(query: &'a str, k: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|kv| kv.split_once('=').filter(|(a, _)| *a == k).map(|(_, v)| v))
}

/// Fresh view if the engine is free, else the snapshot.
fn read(api: &Api, route: Route, n: usize, fresh: impl Fn(&Cortex) -> Value) -> Response {
    let g = match api.cx.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    };
    if let Some(g) = g {
        let mut body = fresh(&g);
        drop(g);
        // Fresh data while the decider waits on the nerve outside the lock.
        if api.snap.busy() {
            if let Some(o) = body.as_object_mut() {
                o.insert("busy".into(), json!(true));
            }
            return Response::json(200, body.to_string()).with_header("x-cortex-busy", "1");
        }
        return Response::json(200, body.to_string());
    }
    let (body, age) = match api.snap.read(route, n) {
        Some(r) => (r.body, r.age_ms),
        // Before the first publish (cannot happen after `build`), say so.
        None => (json!({"ok": true, "service": "gaia-cortex"}), 0),
    };
    let mut body = mark_busy(body, age);
    if route == Route::Health {
        body["ts"] = json!(ts(now_secs()));
        // Reachability is read live, never from the snapshot.
        let (c, configured, stale) = &api.nerve;
        c.overlay(&mut body, *configured, *stale);
    }
    Response::json(200, body.to_string())
        .with_header("x-cortex-busy", "1")
        .with_header("x-cortex-snapshot-age-ms", &age.to_string())
}

/// Bounded wait for the engine (writes only). `None` after `wait`.
fn lock_for(cx: &Mutex<Cortex>, wait: Duration) -> Option<MutexGuard<'_, Cortex>> {
    let t = Instant::now();
    loop {
        match cx.try_lock() {
            Ok(g) => return Some(g),
            Err(TryLockError::Poisoned(p)) => return Some(p.into_inner()),
            Err(TryLockError::WouldBlock) if t.elapsed() >= wait => return None,
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

pub fn handle(req: &Request, api: &Api) -> Response {
    if req.peer.is_some_and(|p| !p.ip().is_loopback()) {
        return Response::json(403, r#"{"ok":false,"error":"loopback only"}"#);
    }
    let n = q(req.query(), "n")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .min(200);
    match (req.method(), req.path()) {
        ("POST", p) if p == CORTEX_INGEST_PATH || p == "/events" => {
            let body = String::from_utf8_lossy(&req.body);
            let parsed = parse_events(&body);
            let (mut ok, mut bad) = (vec![], 0u64);
            for r in parsed {
                match r {
                    Ok(e) => ok.push(e),
                    Err(_) => bad += 1,
                }
            }
            let accepted = ok.len();
            if bad > 0 {
                api.snap.add_invalid(bad);
            }
            if !ok.is_empty() {
                let _ = api.tx.lock().unwrap_or_else(|e| e.into_inner()).send(ok);
            }
            let code = if accepted == 0 && bad > 0 { 400 } else { 202 };
            Response::json(
                code,
                json!({"ok": bad == 0, "accepted": accepted, "invalid": bad}).to_string(),
            )
        }
        ("GET", "/health") => read(api, Route::Health, n, Cortex::health_json),
        ("GET", "/stats") => read(api, Route::Stats, n, |g| {
            let mut v = g.stats_json();
            v["busy"] = json!(false);
            v
        }),
        ("GET", "/incidents") => read(api, Route::Incidents, n, Cortex::incidents_json),
        ("GET", "/decisions") => read(api, Route::Decisions, n, |g| g.decisions.recent(n)),
        ("GET", "/alerts") => read(api, Route::Alerts, n, |g| g.alerts.recent(n)),
        ("GET", "/escalations") => read(api, Route::Escalations, n, Cortex::escalations_json),
        ("GET", "/heals") => read(api, Route::Heals, n, Cortex::heals_json),
        ("POST", "/escalations/ack") => {
            let Some(id) = q(req.query(), "id") else {
                return Response::json(400, r#"{"ok":false,"error":"id required"}"#);
            };
            let Some(mut g) = lock_for(&api.cx, ACK_WAIT) else {
                return Response::json(
                    503,
                    r#"{"ok":false,"busy":true,"error":"decider busy, retry"}"#,
                )
                .with_header("retry-after", "1");
            };
            let hit = g.escalations.ack(id, &ts(now_secs()));
            if hit {
                g.publish();
            }
            Response::json(if hit { 200 } else { 404 }, json!({"ok": hit}).to_string())
        }
        ("GET", "/") => Response::json(
            200,
            r#"{"routes":["/health","/stats","/incidents","/decisions","/alerts","/escalations","/heals","POST /v1/events","POST /escalations/ack?id="]}"#,
        ),
        _ => Response::json(404, r#"{"ok":false,"error":"not found"}"#),
    }
}

/// Server limits for the cortex API: the shared defaults (32 live
/// connections, 5 s head deadline) plus idle eviction, so a flood of
/// connections that never send a request cannot lock `/health` and `/stats`
/// out (T5.1 F4: 32 idle connections gave 5 s of 503s).
pub fn api_limits() -> galion_common::http::Limits {
    galion_common::http::Limits {
        evict_idle: true,
        ..galion_common::http::Limits::default()
    }
}
