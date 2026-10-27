// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex -> Hermes boss handoff (Mate, 2026-10-21 22:50 +02:00, t217).
//!
//! When the fixed rules cannot settle an incident (every cortex escalation:
//! R0 stack-wide, R2 circuit open / start limit, R3 restart cap, R4 unknown
//! unit, R7/R8/R99 high severity, R10 nerve circuit, R11 critical, refused or
//! failed restarts) the incident is handed to the Hermes boss agent instead of
//! asking DeepSeek to code. The cortex itself never calls Hermes or any paid
//! model here: it only drops one JSON file per incident into an inbox
//! directory (atomic rename). A systemd `.path` unit (inotify, no timer, no
//! poll) starts `gaia-cortex hermes-dispatch` (see [`crate::dispatch`]),
//! which enforces the $20 Hermes ceiling and asks the boss for a diagnosis
//! and a proposed fix, to be approved by Mate before any change.
//!
//! Caps: one handoff per incident (persisted), one per subject+rule per
//! `subject_window_s`, at most `per_hour` (hard max 6) per rolling hour, and
//! none at all while the dispatcher's budget stop file exists.

use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// Written by the dispatcher into the inbox when the Hermes budget is spent.
pub const STOP_FILE: &str = "BUDGET-STOP";
/// Never more handoffs per hour than this, whatever the config says.
pub const HARD_MAX_PER_HOUR: u32 = 6;
const INCIDENTS_MAX: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub enum Offer {
    Disabled,
    Written(String),
    Duplicate,
    RateLimited,
    BudgetStopped,
    Failed(String),
}

impl Offer {
    pub fn code(&self) -> &'static str {
        match self {
            Offer::Disabled => "disabled",
            Offer::Written(_) => "handed-to-hermes",
            Offer::Duplicate => "duplicate",
            Offer::RateLimited => "rate-limited",
            Offer::BudgetStopped => "budget-stopped",
            Offer::Failed(_) => "failed",
        }
    }
}

pub struct Handoff {
    pub enabled: bool,
    pub inbox: PathBuf,
    state_path: Option<PathBuf>,
    per_hour: u32,
    subject_window_s: u64,
    incidents: HashSet<String>,
    incident_order: VecDeque<String>,
    subjects: BTreeMap<String, u64>,
    sent: VecDeque<u64>,
    seq: u64,
}

fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if t.is_empty() {
        "incident".into()
    } else {
        t
    }
}

impl Handoff {
    pub fn new(
        enabled: bool,
        inbox: PathBuf,
        state_path: Option<PathBuf>,
        per_hour: u32,
        subject_window_s: u64,
    ) -> Self {
        let mut h = Handoff {
            enabled,
            inbox,
            state_path,
            per_hour: per_hour.min(HARD_MAX_PER_HOUR),
            subject_window_s,
            incidents: HashSet::new(),
            incident_order: VecDeque::new(),
            subjects: BTreeMap::new(),
            sent: VecDeque::new(),
            seq: 0,
        };
        h.load();
        if h.enabled {
            let _ = std::fs::create_dir_all(&h.inbox);
        }
        h
    }

    pub fn disabled() -> Self {
        Handoff::new(false, PathBuf::new(), None, 0, 0)
    }

    fn load(&mut self) {
        let Some(p) = &self.state_path else { return };
        let Ok(t) = std::fs::read_to_string(p) else {
            return;
        };
        let Ok(v) = serde_json::from_str::<Value>(&t) else {
            return;
        };
        for i in v["incidents"].as_array().into_iter().flatten() {
            if let Some(s) = i.as_str() {
                self.remember(s);
            }
        }
        if let Some(o) = v["subjects"].as_object() {
            for (k, t) in o {
                if let Some(t) = t.as_u64() {
                    self.subjects.insert(k.clone(), t);
                }
            }
        }
        self.sent = v["sent"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        self.seq = v["seq"].as_u64().unwrap_or(0);
    }

    fn save(&self) {
        let Some(p) = &self.state_path else { return };
        let v = json!({"incidents": self.incident_order, "subjects": self.subjects,
                       "sent": self.sent, "seq": self.seq});
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }

    fn remember(&mut self, incident: &str) {
        if self.incidents.insert(incident.to_string()) {
            self.incident_order.push_back(incident.to_string());
            while self.incident_order.len() > INCIDENTS_MAX {
                if let Some(o) = self.incident_order.pop_front() {
                    self.incidents.remove(&o);
                }
            }
        }
    }

    pub fn budget_stopped(&self) -> bool {
        self.inbox.join(STOP_FILE).exists()
    }

    pub fn sent_last_hour(&self, now: u64) -> usize {
        self.sent
            .iter()
            .filter(|t| now.saturating_sub(**t) < 3600)
            .count()
    }

    /// Offer one escalation (with its context) for handoff.
    pub fn offer(&mut self, ticket: &Value, now: u64) -> Offer {
        if !self.enabled {
            return Offer::Disabled;
        }
        let incident = ticket["incident"].as_str().unwrap_or("").to_string();
        let skey = format!(
            "{}|{}",
            ticket["subject"].as_str().unwrap_or(""),
            ticket["rule"].as_str().unwrap_or("")
        );
        if self.incidents.contains(&incident) {
            return Offer::Duplicate;
        }
        if let Some(t) = self.subjects.get(&skey) {
            if now.saturating_sub(*t) < self.subject_window_s {
                self.remember(&incident);
                self.save();
                return Offer::Duplicate;
            }
        }
        if self.budget_stopped() {
            return Offer::BudgetStopped;
        }
        while self
            .sent
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 3600)
        {
            self.sent.pop_front();
        }
        if self.sent.len() >= self.per_hour as usize {
            return Offer::RateLimited;
        }
        self.seq += 1;
        let id = format!("hoff-{now}-{}", self.seq);
        let name = format!("{id}-{}.json", sanitize(&incident));
        let mut t = ticket.clone();
        t["handoff_id"] = json!(id);
        if let Err(e) = write_atomic(&self.inbox, &name, &t) {
            return Offer::Failed(e);
        }
        self.remember(&incident);
        self.subjects.insert(skey, now);
        self.subjects
            .retain(|_, t| now.saturating_sub(*t) < 7 * 24 * 3600);
        self.sent.push_back(now);
        self.save();
        Offer::Written(id)
    }

    pub fn to_json(&self, now: u64) -> Value {
        json!({"enabled": self.enabled, "inbox": self.inbox, "per_hour": self.per_hour,
               "sent_last_hour": self.sent_last_hour(now), "incidents_handed": self.incidents.len(),
               "budget_stopped": self.enabled && self.budget_stopped()})
    }
}

/// Write `<dir>/.<name>.tmp`, then rename to `<dir>/<name>`: the `.path`
/// unit (glob `*.json`) only ever sees complete files.
pub fn write_atomic(dir: &Path, name: &str, v: &Value) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("inbox {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{name}.tmp"));
    let dst = dir.join(name);
    let body = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &dst).map_err(|e| format!("rename {}: {e}", dst.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cortex-handoff-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn ticket(inc: &str, subj: &str) -> Value {
        json!({"incident": inc, "subject": subj, "rule": "R3-cap", "severity": "error"})
    }

    fn json_files(d: &Path) -> usize {
        std::fs::read_dir(d)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .count()
    }

    #[test]
    fn once_per_incident_capped_and_persisted() {
        let d = tmpdir("once");
        let inbox = d.join("inbox");
        let st = d.join("state.json");
        let mut h = Handoff::new(true, inbox.clone(), Some(st.clone()), 50, 3600);
        assert_eq!(h.per_hour, HARD_MAX_PER_HOUR, "clamped to 6");
        assert!(matches!(
            h.offer(&ticket("inc-1", "a"), 1000),
            Offer::Written(_)
        ));
        assert_eq!(h.offer(&ticket("inc-1", "a"), 1001), Offer::Duplicate);
        // same subject+rule, new incident id within the window: still one
        assert_eq!(h.offer(&ticket("inc-2", "a"), 1002), Offer::Duplicate);
        for i in 0..5 {
            assert!(matches!(
                h.offer(&ticket(&format!("x{i}"), &format!("s{i}")), 1003),
                Offer::Written(_)
            ));
        }
        assert_eq!(h.offer(&ticket("x9", "s9"), 1004), Offer::RateLimited);
        assert_eq!(json_files(&inbox), 6);
        // an hour later the cap frees; the rate-limited incident goes once
        assert!(matches!(
            h.offer(&ticket("x9", "s9"), 4700),
            Offer::Written(_)
        ));
        // restart: dedupe survives
        let mut h2 = Handoff::new(true, inbox.clone(), Some(st), 6, 3600);
        assert_eq!(h2.offer(&ticket("inc-1", "zz"), 9000), Offer::Duplicate);
        assert_eq!(h2.sent_last_hour(4700), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn budget_stop_and_disabled() {
        let d = tmpdir("stop");
        let inbox = d.join("inbox");
        let mut h = Handoff::new(true, inbox.clone(), None, 6, 3600);
        std::fs::write(inbox.join(STOP_FILE), "spent").unwrap();
        assert_eq!(h.offer(&ticket("i", "s"), 10), Offer::BudgetStopped);
        assert_eq!(json_files(&inbox), 0);
        let mut off = Handoff::disabled();
        assert_eq!(off.offer(&ticket("i", "s"), 10), Offer::Disabled);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names_are_safe() {
        assert_eq!(sanitize("../etc/x y"), "___etc_x_y");
        assert_eq!(sanitize(""), "incident");
    }
}
