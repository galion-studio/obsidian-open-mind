// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! V2 hub: the single ingest point for pushed signals (POST /signal).
//!
//! Wire format (one JSON object, <= 8 KiB):
//! `{"source":"hermes-rs","kind":"heartbeat|metric|event|error","name":"x","value":1.5,"msg":"text"}`
//! `source` and `kind` are required; `ts` is assigned by the nerve.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

pub const MAX_BODY: usize = 8 * 1024;
const PER_SOURCE_KEEP: usize = 64;
const PER_SOURCE_PER_MIN: usize = 120;
const MAX_SOURCES: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub t: u64,
    pub source: String,
    pub kind: String,
    pub name: String,
    pub value: Option<f64>,
    pub msg: String,
}

impl Signal {
    pub fn to_json(&self) -> Value {
        json!({"ts": crate::util::ts(self.t), "source": self.source, "kind": self.kind,
               "name": self.name, "value": self.value, "msg": self.msg})
    }
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// Parse and validate one signal body.
pub fn parse_signal(body: &str, now: u64) -> Result<Signal, String> {
    if body.len() > MAX_BODY {
        return Err("body too large".into());
    }
    let v: Value = serde_json::from_str(body).map_err(|e| format!("bad json: {e}"))?;
    let source = v
        .get("source")
        .and_then(Value::as_str)
        .ok_or("missing source")?;
    let source = clean(source, 64);
    if source.is_empty()
        || !source
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return Err("source must be [A-Za-z0-9-_.]{1,64}".into());
    }
    let kind = v
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("missing kind")?;
    if !["heartbeat", "metric", "event", "error"].contains(&kind) {
        return Err("kind must be heartbeat|metric|event|error".into());
    }
    Ok(Signal {
        t: now,
        source,
        kind: kind.into(),
        name: clean(v.get("name").and_then(Value::as_str).unwrap_or(""), 64),
        value: v
            .get("value")
            .and_then(Value::as_f64)
            .filter(|f| f.is_finite()),
        msg: clean(v.get("msg").and_then(Value::as_str).unwrap_or(""), 300),
    })
}

#[derive(Debug, Default, Clone)]
pub struct SourceState {
    pub recent: VecDeque<Signal>,
    pub last_seen: u64,
    pub total: u64,
    pub dropped: u64,
    pub metrics: HashMap<String, f64>,
    window: VecDeque<u64>,
}

/// All pushed signals, bounded per source and in source count.
#[derive(Debug, Default, Clone)]
pub struct Hub {
    pub sources: HashMap<String, SourceState>,
    pub rejected: u64,
}

impl Hub {
    /// Accept a signal. Err on rate limit or source-table overflow.
    pub fn ingest(&mut self, s: Signal) -> Result<(), String> {
        if !self.sources.contains_key(&s.source) && self.sources.len() >= MAX_SOURCES {
            self.rejected += 1;
            return Err("too many sources".into());
        }
        let st = self.sources.entry(s.source.clone()).or_default();
        while st.window.front().is_some_and(|t| *t + 60 <= s.t) {
            st.window.pop_front();
        }
        if st.window.len() >= PER_SOURCE_PER_MIN {
            st.dropped += 1;
            return Err("rate limited".into());
        }
        st.window.push_back(s.t);
        st.last_seen = s.t;
        st.total += 1;
        if s.kind == "metric" {
            if let Some(v) = s.value {
                if st.metrics.len() < 32 || st.metrics.contains_key(&s.name) {
                    st.metrics.insert(s.name.clone(), v);
                }
            }
        }
        if st.recent.len() >= PER_SOURCE_KEEP {
            st.recent.pop_front();
        }
        st.recent.push_back(s);
        Ok(())
    }

    /// Pushed errors from `source` since `since`.
    pub fn errors_since(&self, source: &str, since: u64) -> u32 {
        self.sources
            .get(source)
            .map(|s| {
                s.recent
                    .iter()
                    .filter(|x| x.kind == "error" && x.t >= since)
                    .count() as u32
            })
            .unwrap_or(0)
    }

    /// Seconds since the source last pushed anything.
    pub fn age(&self, source: &str, now: u64) -> Option<u64> {
        self.sources
            .get(source)
            .map(|s| now.saturating_sub(s.last_seen))
    }

    pub fn to_json(&self, now: u64) -> Value {
        let mut names: Vec<&String> = self.sources.keys().collect();
        names.sort();
        let srcs: Vec<Value> = names
            .iter()
            .map(|n| {
                let s = &self.sources[*n];
                json!({"source": n, "last_seen": crate::util::ts(s.last_seen),
                       "age_s": now.saturating_sub(s.last_seen), "total": s.total, "dropped": s.dropped,
                       "metrics": s.metrics,
                       "recent": s.recent.iter().rev().take(5).map(Signal::to_json).collect::<Vec<_>>()})
            })
            .collect();
        json!({"sources": srcs, "rejected": self.rejected})
    }
}

const PUSH_WINDOW: &str = "the last 2 min";

/// Where a pushed source belongs: (correlation group, component name).
pub type SourceMap<'a> = &'a dyn Fn(&str) -> (String, String);

impl Hub {
    /// Pushed signals since `since` (the engine passes now - 120 s) as incident signals (the service told us,
    /// rather than a probe finding out):
    ///
    /// - `error` (e.g. `http_5xx`) -> Error, cause = the component's group;
    /// - event `slow_request` -> Warn, cause = the group (latency is the
    ///   component's own story);
    /// - event `spend_cap` -> Warn, cause `spend-cap:<group>` (a budget stop,
    ///   not a fault: kept apart from 5xx);
    /// - event `conn_cap` -> Warn, cause = the group;
    /// - event `started` -> Info (shown inside an incident, never opens one);
    /// - heartbeat with `dropped=N>0` -> Info.
    pub fn incident_signals(&self, since: u64, map: SourceMap) -> Vec<crate::incident::Signal> {
        use crate::incident::{Sev, Signal as ISig};
        let mut out = vec![];
        let mut names: Vec<&String> = self.sources.keys().collect();
        names.sort();
        for n in names {
            let st = &self.sources[n];
            let (group, comp) = map(n);
            let new: Vec<&Signal> = st.recent.iter().filter(|s| s.t >= since).collect();
            let mut push = |cause: String, sev: Sev, filt: &dyn Fn(&Signal) -> bool, what: &str| {
                let hits: Vec<&&Signal> = new.iter().filter(|s| filt(s)).collect();
                if let Some(last) = hits.last() {
                    out.push(ISig::new(
                        &cause,
                        &comp,
                        sev,
                        format!(
                            "pushed {} {what} in {PUSH_WINDOW}; last: {}",
                            hits.len(),
                            last.msg
                        ),
                    ));
                }
            };
            push(
                group.clone(),
                Sev::Error,
                &|s| s.kind == "error",
                "error(s)",
            );
            push(
                group.clone(),
                Sev::Warn,
                &|s| s.kind == "event" && s.name == "slow_request",
                "slow request(s)",
            );
            push(
                format!("spend-cap:{group}"),
                Sev::Warn,
                &|s| s.kind == "event" && s.name == "spend_cap",
                "spend-cap hit report(s)",
            );
            push(
                group.clone(),
                Sev::Warn,
                &|s| s.kind == "event" && s.name == "conn_cap",
                "connection-cap hit report(s)",
            );
            push(
                group.clone(),
                Sev::Info,
                &|s| s.kind == "event" && s.name == "started",
                "start(s)",
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_and_invalid() {
        let s = parse_signal(
            r#"{"source":"hermes-rs","kind":"metric","name":"rss_kb","value":2048}"#,
            10,
        )
        .unwrap();
        assert_eq!(
            (s.source.as_str(), s.kind.as_str(), s.value),
            ("hermes-rs", "metric", Some(2048.0))
        );
        assert!(parse_signal(r#"{"kind":"metric"}"#, 0).is_err());
        assert!(parse_signal(r#"{"source":"a b","kind":"metric"}"#, 0).is_err());
        assert!(parse_signal(r#"{"source":"a","kind":"rm -rf"}"#, 0).is_err());
        assert!(parse_signal("not json", 0).is_err());
        let big = format!(
            r#"{{"source":"a","kind":"event","msg":"{}"}}"#,
            "x".repeat(MAX_BODY)
        );
        assert!(parse_signal(&big, 0).is_err());
        let s = parse_signal(
            "{\"source\":\"a\",\"kind\":\"event\",\"msg\":\"l1\\nl2\"}",
            0,
        )
        .unwrap();
        assert_eq!(s.msg, "l1l2");
    }

    #[test]
    fn rate_limit_and_errors() {
        let mut h = Hub::default();
        let mk = |t, kind: &str| Signal {
            t,
            source: "m".into(),
            kind: kind.into(),
            name: "n".into(),
            value: Some(1.0),
            msg: String::new(),
        };
        for i in 0..PER_SOURCE_PER_MIN {
            h.ingest(mk(100, if i % 2 == 0 { "error" } else { "metric" }))
                .unwrap();
        }
        assert!(h.ingest(mk(100, "error")).is_err());
        assert_eq!(h.sources["m"].dropped, 1);
        assert!(h.ingest(mk(161, "heartbeat")).is_ok());
        assert_eq!(h.age("m", 170), Some(9));
        assert!(h.errors_since("m", 0) > 0);
        assert_eq!(h.sources["m"].recent.len(), PER_SOURCE_KEEP);
        assert_eq!(h.sources["m"].metrics.get("n"), Some(&1.0));
    }

    #[test]
    fn pushed_events_become_incident_signals() {
        use crate::incident::Sev;
        let mut h = Hub::default();
        let mk = |t, kind: &str, name: &str, msg: &str| Signal {
            t,
            source: "hermes-rs".into(),
            kind: kind.into(),
            name: name.into(),
            value: None,
            msg: msg.into(),
        };
        h.ingest(mk(90, "error", "http_5xx", "old")).unwrap();
        h.ingest(mk(100, "event", "started", "listening")).unwrap();
        h.ingest(mk(101, "error", "http_5xx", "POST /x -> 502"))
            .unwrap();
        h.ingest(mk(102, "event", "slow_request", "GET /y in 1500 ms"))
            .unwrap();
        h.ingest(mk(
            103,
            "event",
            "spend_cap",
            "daily provider call cap reached (3 of 3)",
        ))
        .unwrap();
        let map = |s: &str| (format!("g-{s}"), s.to_string());
        let v = h.incident_signals(100, &map);
        let find = |c: &str, sev: Sev| v.iter().find(|s| s.cause == c && s.sev == sev).cloned();
        let e = find("g-hermes-rs", Sev::Error).unwrap();
        assert!(
            e.msg.contains("1 error(s)") && e.msg.contains("502"),
            "{}",
            e.msg
        );
        assert!(find("g-hermes-rs", Sev::Warn).unwrap().msg.contains("slow"));
        assert!(find("spend-cap:g-hermes-rs", Sev::Warn)
            .unwrap()
            .msg
            .contains("call cap"));
        assert!(find("g-hermes-rs", Sev::Info).is_some());
        assert!(h.incident_signals(200, &map).is_empty());
    }
}
