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

use crate::engine::{now_secs, ts, Cortex};
use galion_common::http::{Request, Response};
use nerve_cortex_contract::{parse_events, Event, CORTEX_INGEST_PATH};
use serde_json::json;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

fn q<'a>(query: &'a str, k: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|kv| kv.split_once('=').filter(|(a, _)| *a == k).map(|(_, v)| v))
}

pub fn handle(req: &Request, cx: &Arc<Mutex<Cortex>>, tx: &Mutex<Sender<Vec<Event>>>) -> Response {
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
                let mut g = cx.lock().unwrap_or_else(|e| e.into_inner());
                g.stats.events_invalid += bad;
            }
            if !ok.is_empty() {
                let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(ok);
            }
            let code = if accepted == 0 && bad > 0 { 400 } else { 202 };
            Response::json(
                code,
                json!({"ok": bad == 0, "accepted": accepted, "invalid": bad}).to_string(),
            )
        }
        ("GET", "/health") => {
            let g = cx.lock().unwrap_or_else(|e| e.into_inner());
            Response::json(
                200,
                json!({"ok": true, "service": "gaia-cortex", "version": env!("CARGO_PKG_VERSION"), "ts": ts(now_secs()),
                       "mode": if g.cfg.dry_run() { "dry-run" } else { "live" }, "llm_enabled": g.cfg.llm.enabled,
                       "nerve": g.cfg.nerve_url.is_some(), "events_in": g.stats.events_in,
                       "incidents_open": g.correlator.open.len(), "escalations_open": g.escalations.open.len()})
                .to_string(),
            )
        }
        ("GET", "/stats") => Response::json(
            200,
            cx.lock()
                .unwrap_or_else(|e| e.into_inner())
                .stats_json()
                .to_string(),
        ),
        ("GET", "/incidents") => Response::json(
            200,
            cx.lock()
                .unwrap_or_else(|e| e.into_inner())
                .incidents_json()
                .to_string(),
        ),
        ("GET", "/decisions") => Response::json(
            200,
            cx.lock()
                .unwrap_or_else(|e| e.into_inner())
                .decisions
                .recent(n)
                .to_string(),
        ),
        ("GET", "/alerts") => Response::json(
            200,
            cx.lock()
                .unwrap_or_else(|e| e.into_inner())
                .alerts
                .recent(n)
                .to_string(),
        ),
        ("GET", "/escalations") => {
            let g = cx.lock().unwrap_or_else(|e| e.into_inner());
            Response::json(
                200,
                json!({"open": g.escalations.open, "total": g.escalations.log.total}).to_string(),
            )
        }
        ("POST", "/escalations/ack") => {
            let Some(id) = q(req.query(), "id") else {
                return Response::json(400, r#"{"ok":false,"error":"id required"}"#);
            };
            let mut g = cx.lock().unwrap_or_else(|e| e.into_inner());
            let hit = g.escalations.ack(id, &ts(now_secs()));
            Response::json(if hit { 200 } else { 404 }, json!({"ok": hit}).to_string())
        }
        ("GET", "/") => Response::json(
            200,
            r#"{"routes":["/health","/stats","/incidents","/decisions","/alerts","/escalations","POST /v1/events","POST /escalations/ack?id="]}"#,
        ),
        _ => Response::json(404, r#"{"ok":false,"error":"not found"}"#),
    }
}
