// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Append-only JSONL logs: decisions, alerts, the escalate-to-human queue,
//! and the optional alert outbox. In-memory rings back the API.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};

const RING: usize = 200;

pub struct Jsonl {
    pub path: Option<PathBuf>,
    pub ring: VecDeque<Value>,
    pub total: u64,
}

impl Jsonl {
    pub fn new(path: Option<PathBuf>) -> Self {
        Jsonl {
            path,
            ring: VecDeque::new(),
            total: 0,
        }
    }

    pub fn push(&mut self, v: Value) {
        if let Some(p) = &self.path {
            let _ = append(p, &v);
        }
        self.total += 1;
        self.ring.push_back(v);
        while self.ring.len() > RING {
            self.ring.pop_front();
        }
    }

    pub fn recent(&self, n: usize) -> Value {
        Value::Array(self.ring.iter().rev().take(n).cloned().collect())
    }
}

pub fn append(p: &Path, v: &Value) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)?;
    writeln!(f, "{v}")
}

/// Escalate-to-human queue: `escalations.jsonl` + `GET /escalations`,
/// acknowledged with `POST /escalations/ack?id=`.
///
/// Wave-4 slot 417: before this, `ack` was the ONLY way an item left
/// `open` (besides the 200-item ring cap and a process restart, which
/// silently drops the whole in-memory queue). The live cortex never got an
/// ack, so `escalations_open` only grew (1 at 05:47 -> 11 at 15:37
/// +01:00 on 2026-10-28). [`Escalations::close_incident`] and
/// [`Escalations::expire`] close items when their incident resolves or when
/// they are older than a configured age; both are driven by the engine and
/// OFF by default (`escalation_autoclose`, `escalation_max_age_s`). A closed
/// item moves to the `closed` ring with `closed_why` / `closed_ts` and one
/// `{"close": id, ...}` line is appended to the log, the same way an ack is.
pub struct Escalations {
    pub log: Jsonl,
    pub open: VecDeque<Value>,
    /// Recently closed items (newest last, ring of [`RING`]).
    pub closed: VecDeque<Value>,
    /// Items closed by incident resolution or age since start.
    pub closed_total: u64,
    /// Escalation id -> unix seconds it was opened (for [`Escalations::expire`]).
    opened_at: HashMap<String, u64>,
}

impl Escalations {
    pub fn new(path: Option<PathBuf>) -> Self {
        Escalations {
            log: Jsonl::new(path),
            open: VecDeque::new(),
            closed: VecDeque::new(),
            closed_total: 0,
            opened_at: HashMap::new(),
        }
    }

    /// Open an item with no known open time (never expires by age).
    pub fn push(&mut self, v: Value) {
        self.log.push(v.clone());
        self.open.push_back(v);
        self.trim_open();
    }

    /// Open an item at unix second `now` (expires by age when enabled).
    pub fn push_at(&mut self, v: Value, now: u64) {
        if let Some(id) = v["id"].as_str() {
            self.opened_at.insert(id.to_string(), now);
        }
        self.push(v);
    }

    fn trim_open(&mut self) {
        while self.open.len() > RING {
            if let Some(old) = self.open.pop_front() {
                if let Some(id) = old["id"].as_str() {
                    self.opened_at.remove(id);
                }
            }
        }
    }

    pub fn ack(&mut self, id: &str, now_ts: &str) -> bool {
        let before = self.open.len();
        self.open.retain(|v| v["id"] != id);
        let hit = self.open.len() != before;
        if hit {
            self.opened_at.remove(id);
            if let Some(p) = &self.log.path {
                let _ = append(p, &json!({"ack": id, "ts": now_ts}));
            }
        }
        hit
    }

    /// Close every open item whose `incident` is `incident` (its incident
    /// resolved). Returns how many closed.
    pub fn close_incident(&mut self, incident: &str, why: &str, now_ts: &str) -> usize {
        self.close_where(|v, _| v["incident"] == incident, why, now_ts)
    }

    /// Close every open item opened at least `max_age_s` seconds before
    /// `now`. Items without a known open time are kept. 0 = never.
    pub fn expire(&mut self, now: u64, max_age_s: u64, now_ts: &str) -> usize {
        if max_age_s == 0 {
            return 0;
        }
        self.close_where(
            |_, at| at.is_some_and(|t| now.saturating_sub(t) >= max_age_s),
            "expired",
            now_ts,
        )
    }

    fn close_where(
        &mut self,
        pick: impl Fn(&Value, Option<u64>) -> bool,
        why: &str,
        now_ts: &str,
    ) -> usize {
        let mut keep = VecDeque::with_capacity(self.open.len());
        let mut n = 0;
        for mut v in std::mem::take(&mut self.open) {
            let id = v["id"].as_str().unwrap_or("").to_string();
            if !pick(&v, self.opened_at.get(&id).copied()) {
                keep.push_back(v);
                continue;
            }
            n += 1;
            self.opened_at.remove(&id);
            if let Some(p) = &self.log.path {
                let _ = append(
                    p,
                    &json!({"close": id, "incident": v["incident"], "why": why, "ts": now_ts}),
                );
            }
            v["closed_why"] = json!(why);
            v["closed_ts"] = json!(now_ts);
            self.closed.push_back(v);
            while self.closed.len() > RING {
                self.closed.pop_front();
            }
        }
        self.open = keep;
        self.closed_total += n as u64;
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(id: &str, inc: &str) -> Value {
        json!({"id": id, "incident": inc, "subject": "unit:x.service"})
    }

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gaia-cortex-417-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn close_incident_moves_only_matching_items_and_logs_once() {
        let p = tmp("inc");
        let mut q = Escalations::new(Some(p.clone()));
        q.push_at(esc("esc-1", "inc-a"), 100);
        q.push_at(esc("esc-2", "inc-b"), 100);
        q.push_at(esc("esc-3", "inc-a"), 110);
        assert_eq!(q.close_incident("inc-a", "incident-cleared", "T"), 2);
        assert_eq!(q.open.len(), 1);
        assert_eq!(q.open[0]["id"], "esc-2");
        assert_eq!(q.closed.len(), 2);
        assert_eq!(q.closed[0]["closed_why"], "incident-cleared");
        assert_eq!(q.closed_total, 2);
        assert_eq!(q.close_incident("inc-a", "incident-cleared", "T"), 0);
        let log = std::fs::read_to_string(&p).unwrap();
        assert_eq!(log.lines().filter(|l| l.contains("\"close\"")).count(), 2);
        assert_eq!(log.lines().count(), 5);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn expire_uses_open_time_and_zero_is_off() {
        let mut q = Escalations::new(None);
        q.push_at(esc("esc-1", "inc-a"), 1_000);
        q.push_at(esc("esc-2", "inc-b"), 5_000);
        q.push(esc("esc-3", "inc-c")); // no open time: never expires by age
        assert_eq!(q.expire(1_000_000, 0, "T"), 0);
        assert_eq!(q.expire(4_999, 4_000, "T"), 0);
        assert_eq!(q.expire(5_000, 4_000, "T"), 1);
        assert_eq!(q.open.len(), 2);
        assert_eq!(q.expire(9_000, 4_000, "T"), 1);
        assert_eq!(q.open.len(), 1);
        assert_eq!(q.open[0]["id"], "esc-3");
        assert_eq!(q.closed[1]["closed_why"], "expired");
    }

    #[test]
    fn ack_still_works_and_forgets_open_time() {
        let mut q = Escalations::new(None);
        q.push_at(esc("esc-1", "inc-a"), 10);
        assert!(q.ack("esc-1", "T"));
        assert!(!q.ack("esc-1", "T"));
        assert!(q.opened_at.is_empty());
        assert_eq!(q.expire(1_000, 1, "T"), 0);
        assert_eq!(q.closed_total, 0);
    }
}
