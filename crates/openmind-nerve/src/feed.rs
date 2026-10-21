// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The cortex event feed (cortex plan T2.10): [`crate::cortex::Cortex::nerve_events`]
//! of the last cycle, served on the loopback API as B33 `NerveEvent` lines.
//!
//! `GET /api/v1/cortex/events` (also `/cortex/events`):
//! - default: JSON Lines (`application/x-ndjson`), one B33 `NerveEvent` per
//!   line (`ts`, `source`, `kind`, `severity`, `data`), field for field
//!   `openmind_schema::NerveEvent`;
//! - `?format=json`: `{"cycle": N, "ts": .., "events": [..]}` (same events);
//! - `?since=C`: nothing (empty body / empty `events`) unless the nerve has
//!   finished a cycle after `C`; every event carries `data.cycle`, so a poller
//!   keeps the highest one as its cursor.
//!
//! Counts and ids only: `data` = `{cycle, findings, ids, ids_more, top}` with
//! `top` = `{id, component, cause, evidence_n}`. Finding titles, fixes and
//! evidence lines (which can name addresses, hostnames and paths) never
//! leave through the feed. Every string is cut to a short token.
//!
//! Loopback only: the API's accept loop ([`crate::server::serve`]) drops
//! every non-loopback peer before a request is read, and the feed adds no
//! listener of its own.

use serde_json::{json, Value};

/// At most this many finding ids per event (the rest are counted).
pub const MAX_IDS: usize = 32;
/// Longest string token kept in a feed event.
pub const MAX_TOKEN: usize = 96;

/// Feed query: `since=C`, `format=json|jsonl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Query {
    pub since: Option<u64>,
    pub json: bool,
}

impl Query {
    /// Parse the URL query (unknown keys are ignored).
    pub fn parse(q: &str) -> Self {
        let mut out = Query::default();
        for kv in q.split('&') {
            match kv.split_once('=') {
                Some(("since", v)) => out.since = v.parse().ok(),
                Some(("format", v)) => out.json = v == "json",
                _ => {}
            }
        }
        out
    }
}

/// A short, printable token: control characters dropped, cut at
/// [`MAX_TOKEN`] characters.
fn token(v: &Value) -> Value {
    match v.as_str() {
        Some(s) => Value::String(
            s.chars()
                .filter(|c| !c.is_control())
                .take(MAX_TOKEN)
                .collect(),
        ),
        None => Value::Null,
    }
}

/// One `nerve_events` event as a feed event (counts and ids only).
pub fn project(ev: &Value, cycle: u64) -> Value {
    let d = &ev["data"];
    let all: Vec<Value> = d["ids"].as_array().cloned().unwrap_or_default();
    let ids: Vec<Value> = all.iter().take(MAX_IDS).map(token).collect();
    let top = &d["top"];
    json!({
        "ts": ev["ts"],
        "source": token(&ev["source"]),
        "kind": token(&ev["kind"]),
        "severity": token(&ev["severity"]),
        "data": {
            "cycle": cycle,
            "findings": d["findings"].as_u64().unwrap_or(all.len() as u64),
            "ids": ids,
            "ids_more": all.len().saturating_sub(MAX_IDS),
            "top": {
                "id": token(&top["id"]),
                "component": token(&top["component"]),
                "cause": token(&top["cause"]),
                "evidence_n": top["evidence_n"].as_u64().unwrap_or(0),
            },
        },
    })
}

/// The feed body for a nerve `state` (`cortex_events`, `cycle`, `ts`):
/// `(content type, body)`.
pub fn render(state: &Value, q: Query) -> (&'static str, String) {
    let cycle = state["cycle"].as_u64().unwrap_or(0);
    let fresh = q.since.is_none_or(|c| cycle > c);
    let events: Vec<Value> = if fresh {
        state["cortex_events"]
            .as_array()
            .map(|a| a.iter().map(|e| project(e, cycle)).collect())
            .unwrap_or_default()
    } else {
        vec![]
    };
    if q.json {
        let body = json!({"cycle": cycle, "ts": state["ts"], "events": events});
        ("application/json", body.to_string())
    } else {
        let mut body = String::new();
        for e in &events {
            body.push_str(&e.to_string());
            body.push('\n');
        }
        ("application/x-ndjson", body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortex::{Cortex, Finding, FindingKind};
    use crate::incident::Sev;
    use crate::sensors::Sink;

    fn state() -> Value {
        let mut cx = Cortex::default();
        let mut s = Sink::new(&mut cx, "listeners");
        s.finding(
            Finding::new(
                FindingKind::Vulnerability,
                "public-listener-5173",
                Sev::Error,
                "host",
                "port 5173 listens on 203.0.113.9 (not loopback)",
            )
            .with_evidence(vec!["LISTEN 203.0.113.9:5173".into()])
            .with_fix("bind it to 127.0.0.1 on secret-host.example"),
        );
        let mut s = Sink::new(&mut cx, "hades");
        for i in 0..40 {
            s.finding(Finding::new(
                FindingKind::Bleeding,
                format!("b-{i:02}"),
                Sev::Warn,
                "hades-gate",
                "x",
            ));
        }
        json!({"cycle": 7, "ts": "2026-10-20T23:30:00+02:00",
               "cortex_events": cx.nerve_events(1_792_531_800)})
    }

    #[test]
    fn query_parse() {
        assert_eq!(Query::parse(""), Query::default());
        assert_eq!(
            Query::parse("since=12&format=json&x=1"),
            Query {
                since: Some(12),
                json: true
            }
        );
        assert_eq!(Query::parse("since=-1").since, None);
    }

    #[test]
    fn jsonl_lines_are_counts_and_ids_only() {
        let (ct, body) = render(&state(), Query::default());
        assert_eq!(ct, "application/x-ndjson");
        let lines: Vec<Value> = body
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{body}");
        for leak in [
            "203.0.113.9",
            "secret-host",
            "LISTEN",
            "listens on",
            "title",
            "fix",
        ] {
            assert!(!body.contains(leak), "{leak} leaked: {body}");
        }
        let b = &lines[0];
        assert_eq!(
            (b["source"].as_str(), b["kind"].as_str()),
            (Some("hades"), Some("bleeding"))
        );
        assert_eq!(b["data"]["findings"], 40);
        assert_eq!(b["data"]["ids"].as_array().unwrap().len(), MAX_IDS);
        assert_eq!(b["data"]["ids_more"], 8);
        let l = &lines[1];
        assert_eq!(l["severity"], "high");
        assert_eq!(l["data"]["cycle"], 7);
        assert_eq!(l["data"]["top"]["id"], "public-listener-5173");
        assert_eq!(l["data"]["top"]["evidence_n"], 1);
        assert_eq!(l["ts"], "2026-10-20T23:30:00+02:00");
    }

    #[test]
    fn since_cursor_and_json_envelope() {
        let st = state();
        let q = |since, json| Query { since, json };
        assert_eq!(render(&st, q(Some(7), false)).1, "");
        assert_eq!(render(&st, q(Some(6), false)).1.lines().count(), 2);
        let (ct, b) = render(&st, q(Some(7), true));
        assert_eq!(ct, "application/json");
        let v: Value = serde_json::from_str(&b).unwrap();
        assert_eq!(
            (v["cycle"].as_u64(), v["events"].as_array().map(Vec::len)),
            (Some(7), Some(0))
        );
        let v: Value = serde_json::from_str(&render(&st, q(None, true)).1).unwrap();
        assert_eq!(v["events"].as_array().unwrap().len(), 2);
        // A nerve that has not run a cycle yet: empty feed, not an error.
        assert_eq!(render(&json!({}), Query::default()).1, "");
    }

    #[test]
    fn tokens_are_cut_and_cleaned() {
        let ev = json!({"ts": "t", "source": "a\nb", "kind": "ddos", "severity": "crit",
                        "data": {"findings": 1, "ids": ["x".repeat(500)], "top": {"id": 5}}});
        let p = project(&ev, 1);
        assert_eq!(p["source"], "ab");
        assert_eq!(p["data"]["ids"][0].as_str().unwrap().len(), MAX_TOKEN);
        assert!(p["data"]["top"]["id"].is_null());
        assert_eq!(p["data"]["top"]["evidence_n"], 0);
    }
}
