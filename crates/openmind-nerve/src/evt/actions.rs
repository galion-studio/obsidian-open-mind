// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `POST /actions`: action requests from the cortex (or an operator),
//! limited to the reflex allowlist and dry-run by default.
//!
//! Body: `{"action":"restart"|"note", "unit":"x.service", "reason":"...",
//! "request_id":"...", "source":"cortex", "dry_run":true}`.
//! - `restart`: unit must pass [`crate::reflex::permitted`] (allowlist and
//!   never-touch list). Executed only if ALL of: `NERVE_EVT_ACTIONS=live`,
//!   reflex mode is live (not `NERVE_DRY_RUN=1` / `mode` file), and the
//!   request says `"dry_run": false`. Otherwise the answer says what would
//!   run. Max 2 executed restarts per unit per hour, 6 per hour total.
//! - `note`: recorded on the stream only.
//!
//! Every request (accepted or refused) becomes an `action.result` event.

use super::{NerveEvent, Severity};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

pub const PER_UNIT_PER_HOUR: usize = 2;
pub const TOTAL_PER_HOUR: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub action: String,
    pub unit: Option<String>,
    pub reason: String,
    pub request_id: String,
    pub source: String,
    pub dry_run: bool,
}

pub fn parse(body: &str) -> Result<Request, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("json: {e}"))?;
    let st = |k: &str, max: usize| -> Option<String> {
        v.get(k)
            .and_then(Value::as_str)
            .map(|s| s.chars().take(max).collect())
    };
    let action = st("action", 32).ok_or("`action` required")?;
    if !["restart", "note"].contains(&action.as_str()) {
        return Err(format!("action {action:?} not allowed (restart|note)"));
    }
    let unit = st("unit", 128);
    if action == "restart" && unit.is_none() {
        return Err("`unit` required for restart".into());
    }
    if let Some(u) = &unit {
        if !u
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.@".contains(c))
        {
            return Err("bad unit name".into());
        }
    }
    Ok(Request {
        action,
        unit,
        reason: st("reason", 300).unwrap_or_default(),
        request_id: st("request_id", 64).unwrap_or_default(),
        source: st("source", 64).unwrap_or_else(|| "unknown".into()),
        dry_run: v.get("dry_run").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// Runs `systemctl <args>`: (ok, detail).
pub type Runner = Box<dyn FnMut(&[&str]) -> (bool, String) + Send>;

/// Executor: decides, rate-limits and (only when everything says live) runs.
pub struct Actions {
    pub live_enabled: bool,
    per_unit: HashMap<String, VecDeque<u64>>,
    total: VecDeque<u64>,
    /// Runs the command; injectable for tests. Returns (ok, detail).
    pub runner: Runner,
}

impl Actions {
    pub fn new(live_enabled: bool) -> Self {
        Actions {
            live_enabled,
            per_unit: HashMap::new(),
            total: VecDeque::new(),
            runner: Box::new(|a: &[&str]| {
                match crate::util::run("systemctl", a, std::time::Duration::from_secs(20)) {
                    Some(o) => (o.code == 0, format!("exit {}", o.code)),
                    None => (false, "systemctl not runnable".into()),
                }
            }),
        }
    }

    /// Handle one request. `reflex_dry` = the nerve's reflex dry-run state.
    pub fn handle(&mut self, r: &Request, reflex_dry: bool, now: u64) -> (u16, Value, NerveEvent) {
        let base = json!({"request_id": r.request_id, "action": r.action, "unit": r.unit, "source": r.source, "ts": crate::util::ts(now)});
        let mk = |sev: Severity, result: &str, detail: Value, msg: String| {
            let mut e = NerveEvent::new("actions", "action.result", sev, msg).at(now);
            if let Some(u) = &r.unit {
                e = e.unit(u);
                e.kind = "action.result".into();
            }
            e.key = format!("action.result:{}:{}", r.request_id, now);
            let mut d = base.clone();
            d["result"] = json!(result);
            if let (Some(o), Some(dd)) = (d.as_object_mut(), detail.as_object()) {
                o.extend(dd.clone());
            }
            e.data = d;
            e
        };
        if r.action == "note" {
            let e = mk(
                Severity::Info,
                "noted",
                json!({}),
                format!("note from {}: {}", r.source, r.reason),
            );
            return (200, e.data.clone(), e);
        }
        let unit = r.unit.clone().unwrap_or_default();
        if let Err(why) = crate::reflex::permitted(&unit) {
            let e = mk(
                Severity::Medium,
                "refused",
                json!({"error": why}),
                format!("refused restart {unit}: {why}"),
            );
            return (403, e.data.clone(), e);
        }
        let cmd = ["restart", "--no-block", unit.as_str()];
        let would = format!("systemctl {}", cmd.join(" "));
        let mut why_dry = Vec::new();
        if !self.live_enabled {
            why_dry.push("NERVE_EVT_ACTIONS is not live");
        }
        if reflex_dry {
            why_dry.push("reflex mode is dry-run");
        }
        if r.dry_run {
            why_dry.push("request dry_run");
        }
        if !why_dry.is_empty() {
            let e = mk(
                Severity::Info,
                "dry-run",
                json!({"would_run": would, "why": why_dry}),
                format!("dry-run: would run {would}"),
            );
            return (200, e.data.clone(), e);
        }
        let pu = self.per_unit.entry(unit.clone()).or_default();
        while pu.front().is_some_and(|&t| t + 3600 <= now) {
            pu.pop_front();
        }
        while self.total.front().is_some_and(|&t| t + 3600 <= now) {
            self.total.pop_front();
        }
        if pu.len() >= PER_UNIT_PER_HOUR || self.total.len() >= TOTAL_PER_HOUR {
            let e = mk(
                Severity::High,
                "rate-limited",
                json!({"would_run": would}),
                format!("rate-limited: {would}"),
            );
            return (429, e.data.clone(), e);
        }
        pu.push_back(now);
        self.total.push_back(now);
        let (ok, detail) = (self.runner)(&cmd);
        let e = mk(
            if ok { Severity::Medium } else { Severity::High },
            if ok { "executed" } else { "failed" },
            json!({"ran": would, "detail": detail}),
            format!("{} {would}", if ok { "ran" } else { "FAILED" }),
        );
        (if ok { 200 } else { 500 }, e.data.clone(), e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parse_rules() {
        assert!(parse(r#"{"action":"rm","unit":"x"}"#).is_err());
        assert!(parse(r#"{"action":"restart"}"#).is_err());
        assert!(parse(r#"{"action":"restart","unit":"a;b"}"#).is_err());
        let r = parse(r#"{"action":"restart","unit":"hermes-rs.service","request_id":"c-1","source":"cortex"}"#).unwrap();
        assert!(r.dry_run, "dry-run is the default");
    }

    #[test]
    fn dry_run_unless_all_live_and_limits() {
        let ran = Arc::new(Mutex::new(Vec::<String>::new()));
        let r2 = ran.clone();
        let mut a = Actions::new(false);
        a.runner = Box::new(move |c| {
            r2.lock().unwrap().push(c.join(" "));
            (true, "exit 0".into())
        });
        let mut req =
            parse(r#"{"action":"restart","unit":"hermes-rs.service","dry_run":false}"#).unwrap();
        let (c, v, e) = a.handle(&req, false, 1000);
        assert_eq!((c, v["result"].as_str()), (200, Some("dry-run")));
        assert_eq!(e.kind, "action.result");
        a.live_enabled = true;
        assert_eq!(
            a.handle(&req, true, 1000).1["result"],
            "dry-run",
            "reflex dry-run wins"
        );
        assert!(ran.lock().unwrap().is_empty());
        assert_eq!(a.handle(&req, false, 1000).1["result"], "executed");
        assert_eq!(a.handle(&req, false, 1001).1["result"], "executed");
        assert_eq!(a.handle(&req, false, 1002).0, 429);
        assert_eq!(ran.lock().unwrap().len(), 2);
        assert_eq!(
            ran.lock().unwrap()[0],
            "restart --no-block hermes-rs.service"
        );
        req.unit = Some("galion-nerve.service".into());
        assert_eq!(a.handle(&req, false, 1003).0, 403);
        req.unit = Some("ssh.service".into());
        assert_eq!(a.handle(&req, false, 1003).0, 403);
        let n = parse(r#"{"action":"note","reason":"cortex saw x"}"#).unwrap();
        assert_eq!(a.handle(&n, false, 1).1["result"], "noted");
    }
}
