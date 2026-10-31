// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Pending approvals (`CORTEX_ACTION_MODE=approve_each`, owner 2026-10-31).
//!
//! In `approve_each` the engine never acts on its own: each restart the
//! rules want (and each Hermes handoff, when the handoff is enabled) is
//! parked here as `{id, incident, unit, action, expires}` and paged to the
//! owner. `POST /approvals/<id>/approve` (loopback + bearer token, see
//! [`crate::server`]) queues it for the decider, which sends it (live only
//! with `actions = "live"`, still inside every cap, circuit and guard);
//! `reject` drops it; anything not decided within `approval_ttl_s` expires.
//! Every change is one line in `<data_dir>/pending-approvals.jsonl`
//! (`decided_by: "owner"` for decisions); still-pending items are reloaded
//! from it at start.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// At most this many parked actions; more are refused (alert only).
pub const MAX_PENDING: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct Parked {
    pub id: String,
    pub ts: u64,
    pub incident: String,
    pub subject: String,
    pub unit: Option<String>,
    /// `restart` or `handoff`.
    pub action: String,
    pub rule: String,
    pub reason: String,
    pub expires: u64,
    /// The handoff ticket (action `handoff`), else null.
    pub payload: Value,
}

impl Parked {
    pub fn to_json(&self, status: &str) -> Value {
        json!({"id": self.id, "ts": crate::engine::ts(self.ts), "t": self.ts, "incident": self.incident,
               "subject": self.subject, "unit": self.unit, "action": self.action, "rule": self.rule,
               "reason": self.reason, "expires": crate::engine::ts(self.expires), "expires_t": self.expires,
               "status": status, "payload": self.payload})
    }

    fn from_json(v: &Value) -> Option<Parked> {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        Some(Parked {
            id: s("id")?,
            ts: v["t"].as_u64()?,
            incident: s("incident").unwrap_or_default(),
            subject: s("subject").unwrap_or_default(),
            unit: s("unit"),
            action: s("action")?,
            rule: s("rule").unwrap_or_default(),
            reason: s("reason").unwrap_or_default(),
            expires: v["expires_t"].as_u64()?,
            payload: v["payload"].clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Reject,
}

#[derive(Debug, Default)]
pub struct Approvals {
    path: Option<PathBuf>,
    pub ttl_s: u64,
    pending: BTreeMap<String, Parked>,
    /// Approved, waiting for the decider.
    approved: Vec<Parked>,
    epoch: u64,
    seq: u64,
    pub parked_total: u64,
    pub approved_total: u64,
    pub rejected_total: u64,
    pub expired_total: u64,
    pub refused_full: u64,
}

impl Approvals {
    pub fn new(path: Option<PathBuf>, ttl_s: u64, now: u64) -> Self {
        let mut a = Approvals {
            path,
            ttl_s,
            epoch: now,
            ..Default::default()
        };
        a.reload(now);
        a
    }

    /// Rebuild the still-pending items from the log (last line per id wins).
    fn reload(&mut self, now: u64) {
        let Some(p) = &self.path else { return };
        let Ok(t) = std::fs::read_to_string(p) else {
            return;
        };
        let mut last: BTreeMap<String, Value> = BTreeMap::new();
        for l in t.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(l) {
                if let Some(id) = v["id"].as_str() {
                    last.insert(id.to_string(), v);
                }
            }
        }
        for v in last.values() {
            if v["status"] == "pending" {
                if let Some(p) = Parked::from_json(v) {
                    if p.expires > now {
                        self.pending.insert(p.id.clone(), p);
                    }
                }
            }
        }
    }

    fn log(&self, v: &Value) {
        if let Some(p) = &self.path {
            let _ = crate::sink::append(p, v);
        }
    }

    /// Park one action. `None` when the book is full (the caller alerts).
    #[allow(clippy::too_many_arguments)]
    pub fn park(
        &mut self,
        incident: &str,
        subject: &str,
        unit: Option<&str>,
        action: &str,
        rule: &str,
        reason: &str,
        payload: Value,
        now: u64,
    ) -> Option<String> {
        self.expire(now);
        if self.pending.len() >= MAX_PENDING {
            self.refused_full += 1;
            return None;
        }
        // One pending item per incident and action: a repeat is the same ask.
        if let Some(p) = self
            .pending
            .values()
            .find(|p| p.incident == incident && p.action == action && p.unit.as_deref() == unit)
        {
            return Some(p.id.clone());
        }
        self.seq += 1;
        let p = Parked {
            id: format!("apr-{}-{}", self.epoch, self.seq),
            ts: now,
            incident: incident.into(),
            subject: subject.into(),
            unit: unit.map(str::to_string),
            action: action.into(),
            rule: rule.into(),
            reason: reason.chars().take(500).collect(),
            expires: now + self.ttl_s,
            payload,
        };
        self.log(&p.to_json("pending"));
        let id = p.id.clone();
        self.pending.insert(id.clone(), p);
        self.parked_total += 1;
        Some(id)
    }

    /// Expire overdue items. Returns how many.
    pub fn expire(&mut self, now: u64) -> usize {
        let gone: Vec<String> = self
            .pending
            .values()
            .filter(|p| p.expires <= now)
            .map(|p| p.id.clone())
            .collect();
        for id in &gone {
            if let Some(p) = self.pending.remove(id) {
                let mut v = p.to_json("expired");
                v["decided_ts"] = json!(crate::engine::ts(now));
                v["decided_by"] = json!("ttl");
                self.log(&v);
                self.expired_total += 1;
            }
        }
        gone.len()
    }

    /// The owner's decision. Err = unknown or expired id.
    pub fn decide(&mut self, id: &str, d: Decision, now: u64) -> Result<Parked, String> {
        self.expire(now);
        let p = self
            .pending
            .remove(id)
            .ok_or_else(|| format!("no pending approval {id:?} (unknown, decided or expired)"))?;
        let status = match d {
            Decision::Approve => "approved",
            Decision::Reject => "rejected",
        };
        let mut v = p.to_json(status);
        v["decided_ts"] = json!(crate::engine::ts(now));
        v["decided_by"] = json!("owner");
        self.log(&v);
        match d {
            Decision::Approve => {
                self.approved_total += 1;
                self.approved.push(p.clone());
            }
            Decision::Reject => self.rejected_total += 1,
        }
        Ok(p)
    }

    /// Approved items for the decider (drained).
    pub fn take_approved(&mut self) -> Vec<Parked> {
        std::mem::take(&mut self.approved)
    }

    pub fn has_approved(&self) -> bool {
        !self.approved.is_empty()
    }

    pub fn to_json(&self, now: u64) -> Value {
        let items: Vec<Value> = self
            .pending
            .values()
            .filter(|p| p.expires > now)
            .map(|p| p.to_json("pending"))
            .collect();
        json!({"pending": items, "ttl_s": self.ttl_s, "parked_total": self.parked_total,
               "approved_total": self.approved_total, "rejected_total": self.rejected_total,
               "expired_total": self.expired_total, "refused_full": self.refused_full,
               "queued_for_decider": self.approved.len()})
    }
}

/// Read the bearer token (first line, trimmed; at least 16 chars). The
/// file must not be readable by group/other on unix.
pub fn read_token(path: &std::path::Path) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(path)
            .map_err(|e| format!("approval token {}: {e}", path.display()))?;
        if m.permissions().mode() & 0o077 != 0 {
            return Err(format!(
                "approval token {} must be mode 0600 or stricter",
                path.display()
            ));
        }
    }
    let t = std::fs::read_to_string(path)
        .map_err(|e| format!("approval token {}: {e}", path.display()))?;
    let t = t.lines().next().unwrap_or("").trim().to_string();
    if t.len() < 16 {
        return Err("approval token too short (min 16 chars)".into());
    }
    Ok(t)
}

/// Constant-time-ish comparison (no early exit on the first mismatch).
pub fn token_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cortex-apr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn park_decide_expire_and_reload() {
        let d = tmp("cycle");
        let f = d.join("pending-approvals.jsonl");
        let mut a = Approvals::new(Some(f.clone()), 600, 1000);
        let id = a
            .park(
                "inc-1",
                "unit:x.service",
                Some("x.service"),
                "restart",
                "R3-known-crash",
                "x crashed",
                Value::Null,
                1000,
            )
            .unwrap();
        // same incident + action: same ask
        assert_eq!(
            a.park(
                "inc-1",
                "unit:x.service",
                Some("x.service"),
                "restart",
                "R3-known-crash",
                "again",
                Value::Null,
                1001
            ),
            Some(id.clone())
        );
        let id2 = a
            .park(
                "inc-2",
                "unit:y.service",
                Some("y.service"),
                "restart",
                "R3-known-crash",
                "y",
                Value::Null,
                1002,
            )
            .unwrap();
        let id3 = a
            .park(
                "inc-3",
                "unit:z.service",
                Some("z.service"),
                "handoff",
                "R4-unknown-unit",
                "z",
                json!({"incident": "inc-3"}),
                1003,
            )
            .unwrap();
        assert_eq!(a.to_json(1004)["pending"].as_array().unwrap().len(), 3);
        // restart: pending items survive
        let mut a = Approvals::new(Some(f.clone()), 600, 1005);
        assert_eq!(a.to_json(1005)["pending"].as_array().unwrap().len(), 3);
        let p = a.decide(&id, Decision::Approve, 1006).unwrap();
        assert_eq!(
            (p.action.as_str(), p.unit.as_deref()),
            ("restart", Some("x.service"))
        );
        assert!(
            a.decide(&id, Decision::Approve, 1007).is_err(),
            "decided once"
        );
        a.decide(&id2, Decision::Reject, 1008).unwrap();
        assert_eq!(a.take_approved().len(), 1);
        assert!(a.take_approved().is_empty());
        // id3 expires after the ttl
        assert!(a.decide(&id3, Decision::Approve, 1003 + 600).is_err());
        assert_eq!(a.expired_total, 1);
        let log = std::fs::read_to_string(&f).unwrap();
        assert!(log.contains("\"decided_by\":\"owner\""));
        assert!(log.contains("\"status\":\"rejected\"") && log.contains("\"status\":\"expired\""));
        let a = Approvals::new(Some(f), 600, 1700);
        assert!(a.to_json(1700)["pending"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn full_book_refuses() {
        let mut a = Approvals::new(None, 600, 0);
        for i in 0..MAX_PENDING {
            assert!(a
                .park(
                    &format!("i{i}"),
                    "s",
                    Some("u"),
                    "restart",
                    "r",
                    "",
                    Value::Null,
                    1
                )
                .is_some());
        }
        assert!(a
            .park("over", "s", Some("u"), "restart", "r", "", Value::Null, 1)
            .is_none());
        assert_eq!(a.refused_full, 1);
    }

    #[test]
    fn token_file_must_be_private_and_long() {
        let d = tmp("tok");
        let f = d.join("token");
        std::fs::write(&f, "0123456789abcdef0123\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_token(&f).is_err());
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(read_token(&f).unwrap(), "0123456789abcdef0123");
        std::fs::write(&f, "short").unwrap();
        assert!(read_token(&f).is_err());
        assert!(token_eq("abc", "abc") && !token_eq("abc", "abd") && !token_eq("abc", "abcd"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
