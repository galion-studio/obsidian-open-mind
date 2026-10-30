// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Heal verification: did the restart the cortex asked for actually fix
//! the unit? (gaia-heal-verify, 2026-10-30.)
//!
//! Before this module the heal loop was detect -> decide -> act and then
//! stopped: the nerve runs `systemctl restart --no-block`, answers
//! `executed`, and the cortex counted that as done. A unit that never came
//! back (or came back and sat in `failed` without a new crash event) was
//! never noticed, and nobody learned whether restarts work.
//!
//! Every **executed** (or lost-reply) live restart now arms one check:
//!
//! - **verified**: the nerve publishes the unit's held recovery
//!   (`unit.state` with `recovered: true` and an active state, or
//!   `incident.resolved` with `state: active`) after the restart was asked.
//!   The nerve only says that once the unit stayed up for its recover hold
//!   (30 s on the VPS), so a crash loop never verifies. No polling.
//! - **failed-again**: a new crash / OOM / start-limit / `failed` state for
//!   the unit after the restart. The rules already count it (flap breaker,
//!   caps), so this only records the outcome.
//! - at the deadline (`heal_verify_s`, default 180 s) with neither, ONE
//!   read of the nerve `/state` (GET, loopback) decides: unit `active` ->
//!   **verified** (`by: nerve-state`); any other state -> **failed**: the
//!   cortex alerts (only) and opens the unit's circuit
//!   breaker, so it stops restarting a unit that restarts do not fix; nerve
//!   unreachable / unit not in `/state` -> **unverified** (alert only).
//! - a **freeze heal** ([`crate::freeze`]: the unit was running but not
//!   answering) is judged by a fresh health probe instead: the nerve's
//!   recovery events only say the process runs again, the probe says it
//!   answers. Answer below 500 -> **verified** (`by: probe`), else
//!   **failed**. A new crash is still **failed-again**.
//!
//! A failed heal ALERTS only (nerve / ntfy path); it is not handed to the
//! human / Hermes escalation queue (Mate, 2026-10-30 11:27 CET).
//!
//! The verifier never acts on a service: it only reads, and its only
//! effect on the rules is to make them *more* conservative (circuit open).
//! Checks and outcomes are appended to `heals.jsonl`; checks still pending
//! when the cortex stops are re-armed at the next start.

use nerve_cortex_contract::{Event, EventKind};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;

/// Default verification window (seconds after the restart was asked).
pub const DEFAULT_VERIFY_S: u64 = 180;
/// Allowed window range when set (0 = off).
pub const VERIFY_S_RANGE: (u64, u64) = (30, 3600);
/// Finished checks kept in memory for `GET /heals`.
const KEEP_DONE: usize = 100;
/// Lines of `heals.jsonl` read back at start to re-arm pending checks.
const RELOAD_TAIL: usize = 500;
/// A check re-armed at start gets at least this long before its deadline.
const RELOAD_GRACE_S: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Verified,
    FailedAgain,
    Failed,
    Unverified,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Verified => "verified",
            Verdict::FailedAgain => "failed-again",
            Verdict::Failed => "failed",
            Verdict::Unverified => "unverified",
        }
    }
}

/// One armed check.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub id: String,
    pub decision: String,
    pub incident: String,
    pub unit: String,
    /// Unix seconds the restart was asked.
    pub asked: u64,
    pub due: u64,
    /// Freeze heal: judged by probing this loopback port + path.
    pub probe: Option<(u16, String)>,
}

impl Check {
    fn to_json(&self) -> Value {
        let mut v = json!({"id": self.id, "decision": self.decision, "incident": self.incident, "unit": self.unit,
               "asked": crate::engine::ts(self.asked), "asked_unix": self.asked,
               "due": crate::engine::ts(self.due), "due_unix": self.due});
        if let Some((port, path)) = &self.probe {
            v["kind"] = json!("freeze");
            v["probe_port"] = json!(port);
            v["probe_path"] = json!(path);
        }
        v
    }
}

/// A finished check.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub check: Check,
    pub verdict: Verdict,
    /// What decided: `nerve-recovery`, `nerve-failure`, `nerve-state`, `nerve-unreachable`, ...
    pub by: String,
    pub detail: String,
    pub at: u64,
}

impl Done {
    pub fn to_json(&self) -> Value {
        let mut v = self.check.to_json();
        v["event"] = json!("finished");
        v["outcome"] = json!(self.verdict.as_str());
        v["by"] = json!(self.by);
        v["detail"] = json!(self.detail);
        v["at"] = json!(crate::engine::ts(self.at));
        v["heal_s"] = json!(self.at.saturating_sub(self.check.asked));
        v
    }
}

/// What the deadline read of the nerve `/state` found for one unit.
#[derive(Debug, Clone, PartialEq)]
pub enum StateRead {
    /// `unit_state` (`active`, `failed`, ...) and the nerve component status.
    Unit { state: String, status: String },
    /// The nerve answered but lists no component for this unit.
    Missing,
    /// No answer / not JSON.
    Unreachable(String),
    /// Freeze heal: the deadline health probe (`up` = answered below 500).
    Probe { up: bool, detail: String },
}

/// Find `unit` in a nerve `/state` body (`components[].unit`).
pub fn state_of(body: &Value, unit: &str) -> StateRead {
    let Some(cs) = body.get("components").and_then(Value::as_array) else {
        return StateRead::Unreachable("no components in /state".into());
    };
    let norm = |u: &str| {
        if u.contains('.') {
            u.to_string()
        } else {
            format!("{u}.service")
        }
    };
    let want = norm(unit);
    for c in cs {
        let hit = c
            .get("unit")
            .and_then(Value::as_str)
            .is_some_and(|u| norm(u) == want);
        if hit {
            let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            return StateRead::Unit {
                state: s("unit_state"),
                status: s("status"),
            };
        }
    }
    StateRead::Missing
}

/// Decide a due check from the `/state` read.
pub fn judge(read: &StateRead) -> (Verdict, String, String) {
    match read {
        StateRead::Unit { state, status } if matches!(state.as_str(), "active" | "running") => (
            Verdict::Verified,
            "nerve-state".into(),
            format!("unit {state}, component {status}"),
        ),
        StateRead::Unit { state, status } if state.is_empty() => (
            // the nerve lists the unit but cannot read systemd (no unit_state):
            // never open a circuit on a blind read
            Verdict::Unverified,
            "nerve-state".into(),
            format!(
                "nerve has no unit state, component {}",
                if status.is_empty() { "unknown" } else { status }
            ),
        ),
        StateRead::Unit { state, status } => (
            Verdict::Failed,
            "nerve-state".into(),
            format!(
                "unit {} after the restart, component {}",
                state,
                if status.is_empty() { "unknown" } else { status }
            ),
        ),
        StateRead::Missing => (
            Verdict::Unverified,
            "nerve-state".into(),
            "unit not listed in nerve /state".into(),
        ),
        StateRead::Unreachable(e) => (Verdict::Unverified, "nerve-unreachable".into(), e.clone()),
        StateRead::Probe { up: true, detail } => (
            Verdict::Verified,
            "probe".into(),
            format!("health probe answers ({detail})"),
        ),
        StateRead::Probe { up: false, detail } => (
            Verdict::Failed,
            "probe".into(),
            format!("still not answering after the restart ({detail})"),
        ),
    }
}

fn is_failure(e: &Event) -> bool {
    match e.kind() {
        EventKind::UnitCrash | EventKind::StartLimit => true,
        EventKind::Oom => e.unit().is_some(),
        EventKind::UnitState => e.to_state() == Some("failed"),
        _ => false,
    }
}

fn is_held_recovery(e: &Event) -> bool {
    let active = |s: Option<&str>| matches!(s, Some("active" | "running"));
    if e.kind == "incident.resolved" {
        return active(e.data.get("state").and_then(Value::as_str));
    }
    e.kind() == EventKind::UnitState
        && e.data.get("recovered").and_then(Value::as_bool) == Some(true)
        && active(e.to_state())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Counts {
    pub armed: u64,
    pub verified: u64,
    pub failed_again: u64,
    pub failed: u64,
    pub unverified: u64,
    pub rearmed: u64,
}

/// The verifier book (lives in the engine, under its lock).
#[derive(Debug)]
pub struct Verifier {
    /// Window in seconds; 0 = off (nothing is armed).
    pub window_s: u64,
    pub pending: Vec<Check>,
    pub done: VecDeque<Done>,
    pub counts: Counts,
    log: Option<PathBuf>,
}

impl Verifier {
    pub fn new(window_s: u64, log: Option<PathBuf>) -> Self {
        Verifier {
            window_s,
            pending: vec![],
            done: VecDeque::new(),
            counts: Counts::default(),
            log,
        }
    }

    fn write(&self, v: &Value) {
        if let Some(p) = &self.log {
            let _ = crate::sink::append(p, v);
        }
    }

    /// Re-arm checks a previous process armed and never finished. Returns
    /// how many were re-armed.
    pub fn reload(&mut self, now: u64) -> usize {
        if self.window_s == 0 {
            return 0;
        }
        let Some(p) = &self.log else { return 0 };
        let Ok(text) = std::fs::read_to_string(p) else {
            return 0;
        };
        let lines: Vec<&str> = text.lines().collect();
        let tail = &lines[lines.len().saturating_sub(RELOAD_TAIL)..];
        let mut open: Vec<Check> = vec![];
        for l in tail {
            let Ok(v) = serde_json::from_str::<Value>(l) else {
                continue;
            };
            let id = v.get("id").and_then(Value::as_str).unwrap_or("");
            match v.get("event").and_then(Value::as_str) {
                Some("armed") => {
                    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
                    if !id.is_empty() && !s("unit").is_empty() {
                        open.retain(|c| c.id != id);
                        let probe = v
                            .get("probe_port")
                            .and_then(Value::as_u64)
                            .and_then(|p| u16::try_from(p).ok())
                            .filter(|p| *p > 0)
                            .map(|p| (p, s("probe_path")))
                            .filter(|(_, path)| path.starts_with('/'));
                        open.push(Check {
                            id: id.into(),
                            decision: s("decision"),
                            incident: s("incident"),
                            unit: s("unit"),
                            asked: n("asked_unix"),
                            due: n("due_unix"),
                            probe,
                        });
                    }
                }
                Some("finished") => open.retain(|c| c.id != id),
                _ => {}
            }
        }
        let n = open.len();
        for mut c in open {
            c.due = c.due.max(now + RELOAD_GRACE_S);
            self.pending.push(c);
        }
        self.counts.rearmed += n as u64;
        n
    }

    /// Arm a check for a restart of `unit` asked at `now`. No-op when off.
    /// A newer restart of the same unit replaces its pending check.
    pub fn arm(&mut self, decision: &str, incident: &str, unit: &str, now: u64) -> Option<Check> {
        self.arm_with(decision, incident, unit, None, now)
    }

    /// [`Verifier::arm`] for a freeze heal: judged by probing `probe`.
    pub fn arm_with(
        &mut self,
        decision: &str,
        incident: &str,
        unit: &str,
        probe: Option<(u16, String)>,
        now: u64,
    ) -> Option<Check> {
        if self.window_s == 0 {
            return None;
        }
        self.pending.retain(|c| c.unit != unit);
        let c = Check {
            id: format!("heal-{now}-{decision}"),
            decision: decision.into(),
            incident: incident.into(),
            unit: unit.into(),
            asked: now,
            due: now + self.window_s,
            probe,
        };
        let mut v = c.to_json();
        v["event"] = json!("armed");
        self.write(&v);
        self.counts.armed += 1;
        self.pending.push(c.clone());
        Some(c)
    }

    /// Event evidence (stream): recoveries and new failures of a pending unit.
    pub fn observe(&mut self, evs: &[Event], now: u64) -> Vec<Done> {
        let mut out = vec![];
        if self.pending.is_empty() {
            return out;
        }
        for e in evs {
            let Some(u) = e.unit() else { continue };
            let Some(i) = self.pending.iter().position(|c| c.unit == u) else {
                continue;
            };
            let at = e.t.unwrap_or(now);
            if at < self.pending[i].asked {
                continue; // the burst that caused the restart, or older
            }
            let found = if is_failure(e) {
                Some((Verdict::FailedAgain, "nerve-failure", e.kind.clone()))
            } else if is_held_recovery(e) && self.pending[i].probe.is_none() {
                // a freeze heal waits for its probe: running is not answering
                Some((Verdict::Verified, "nerve-recovery", e.kind.clone()))
            } else {
                None
            };
            if let Some((verdict, by, kind)) = found {
                let check = self.pending.remove(i);
                let msg = e.msg.clone().unwrap_or_default();
                out.push(self.finish(check, verdict, by, format!("{kind} {msg}").trim(), now));
            }
        }
        out
    }

    /// Checks past their deadline (removed from `pending`).
    pub fn take_due(&mut self, now: u64) -> Vec<Check> {
        let (due, keep): (Vec<Check>, Vec<Check>) =
            self.pending.drain(..).partition(|c| c.due <= now);
        self.pending = keep;
        due
    }

    /// Earliest pending deadline.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.iter().map(|c| c.due).min()
    }

    pub fn finish(
        &mut self,
        check: Check,
        verdict: Verdict,
        by: &str,
        detail: &str,
        now: u64,
    ) -> Done {
        match verdict {
            Verdict::Verified => self.counts.verified += 1,
            Verdict::FailedAgain => self.counts.failed_again += 1,
            Verdict::Failed => self.counts.failed += 1,
            Verdict::Unverified => self.counts.unverified += 1,
        }
        let d = Done {
            check,
            verdict,
            by: by.into(),
            detail: detail.into(),
            at: now,
        };
        self.write(&d.to_json());
        self.done.push_back(d.clone());
        while self.done.len() > KEEP_DONE {
            self.done.pop_front();
        }
        d
    }

    /// Share of finished checks that verified (None before the first).
    pub fn success_rate(&self) -> Option<f64> {
        let c = &self.counts;
        let n = c.verified + c.failed_again + c.failed + c.unverified;
        (n > 0).then(|| c.verified as f64 / n as f64)
    }

    pub fn summary_json(&self) -> Value {
        let c = &self.counts;
        json!({"enabled": self.window_s > 0, "window_s": self.window_s, "pending": self.pending.len(),
               "armed": c.armed, "rearmed": c.rearmed, "verified": c.verified, "failed_again": c.failed_again,
               "failed": c.failed, "unverified": c.unverified,
               "success_rate": self.success_rate().map(|r| (r * 1000.0).round() / 1000.0)})
    }

    pub fn to_json(&self) -> Value {
        let mut v = self.summary_json();
        v["pending_checks"] = Value::Array(self.pending.iter().map(Check::to_json).collect());
        v["recent"] = Value::Array(self.done.iter().rev().map(Done::to_json).collect());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, unit: &str, t: u64, data: Value) -> Event {
        let mut e = Event::new("nerve", kind, "info", data);
        e.unit = Some(unit.into());
        e.t = Some(t);
        e
    }

    fn v() -> Verifier {
        Verifier::new(180, None)
    }

    #[test]
    fn off_arms_nothing() {
        let mut v = Verifier::new(0, None);
        assert!(v.arm("dec-1", "inc", "a.service", 100).is_none());
        assert!(v.pending.is_empty());
        assert_eq!(v.summary_json()["enabled"], false);
    }

    #[test]
    fn held_recovery_verifies_plain_start_does_not() {
        let mut v = v();
        v.arm("dec-1", "inc", "a.service", 100);
        // journal Started alone is not held: no verdict yet
        assert!(v
            .observe(&[ev("unit.started", "a.service", 101, json!({}))], 101)
            .is_empty());
        let d = v.observe(
            &[ev(
                "unit.state",
                "a.service",
                135,
                json!({"to": "active", "recovered": true}),
            )],
            135,
        );
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].verdict, Verdict::Verified);
        assert_eq!(d[0].by, "nerve-recovery");
        assert_eq!(d[0].to_json()["heal_s"], 35);
        assert!(v.pending.is_empty());
        assert_eq!(v.success_rate(), Some(1.0));
    }

    #[test]
    fn incident_resolved_active_verifies_inactive_does_not() {
        let mut v = v();
        v.arm("dec-1", "inc", "a.service", 100);
        let mut e = Event::new(
            "nerve-recover",
            "incident.resolved",
            "info",
            json!({"unit": "a.service", "state": "inactive"}),
        );
        e.t = Some(140);
        assert!(
            v.observe(&[e.clone()], 140).is_empty(),
            "inactive is not healed"
        );
        e.data["state"] = json!("active");
        assert_eq!(v.observe(&[e], 141)[0].verdict, Verdict::Verified);
    }

    #[test]
    fn old_failure_ignored_new_failure_is_failed_again() {
        let mut v = v();
        v.arm("dec-1", "inc", "a.service", 100);
        assert!(v
            .observe(&[ev("unit.failed", "a.service", 99, json!({}))], 101)
            .is_empty());
        let d = v.observe(&[ev("oom.kill", "a.service", 120, json!({}))], 120);
        assert_eq!(d[0].verdict, Verdict::FailedAgain);
        assert_eq!(v.counts.failed_again, 1);
        assert_eq!(v.success_rate(), Some(0.0));
    }

    #[test]
    fn other_units_do_not_touch_a_check() {
        let mut v = v();
        v.arm("dec-1", "inc", "a.service", 100);
        assert!(v
            .observe(&[ev("unit.failed", "b.service", 120, json!({}))], 120)
            .is_empty());
        assert_eq!(v.pending.len(), 1);
    }

    #[test]
    fn deadline_and_next_due() {
        let mut v = v();
        v.arm("dec-1", "inc", "a.service", 100);
        v.arm("dec-2", "inc", "b.service", 150);
        assert_eq!(v.next_due(), Some(280));
        assert!(v.take_due(279).is_empty());
        let d = v.take_due(280);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].unit, "a.service");
        assert_eq!(v.next_due(), Some(330));
    }

    #[test]
    fn rearm_same_unit_replaces() {
        let mut v = v();
        v.arm("dec-1", "inc", "a.service", 100);
        v.arm("dec-2", "inc", "a.service", 200);
        assert_eq!(v.pending.len(), 1);
        assert_eq!(v.pending[0].decision, "dec-2");
    }

    #[test]
    fn state_reads_and_judge() {
        let body = json!({"components": [
            {"name": "console", "unit": "console.service", "unit_state": "active", "status": "healthy"},
            {"name": "x", "unit": "x.service", "unit_state": "failed", "status": "failing"}]});
        assert_eq!(
            judge(&state_of(&body, "console.service")).0,
            Verdict::Verified
        );
        assert_eq!(
            judge(&state_of(&body, "console")).0,
            Verdict::Verified,
            "suffix optional"
        );
        let (f, _, why) = judge(&state_of(&body, "x.service"));
        assert_eq!(f, Verdict::Failed);
        assert!(why.contains("failed"), "{why}");
        assert_eq!(
            judge(&state_of(&body, "nope.service")).0,
            Verdict::Unverified
        );
        assert_eq!(
            judge(&state_of(&json!({"x": 1}), "a")).0,
            Verdict::Unverified
        );
        assert_eq!(
            judge(&StateRead::Unreachable("timeout".into())).1,
            "nerve-unreachable"
        );
        // still activating 3 min after the restart is not healed
        let act = json!({"components": [{"unit": "a.service", "unit_state": "activating", "status": "failing"}]});
        assert_eq!(judge(&state_of(&act, "a.service")).0, Verdict::Failed);
        // a nerve without systemd (unit_state null) never fails a heal
        let blind =
            json!({"components": [{"unit": "a.service", "unit_state": null, "status": "healthy"}]});
        assert_eq!(judge(&state_of(&blind, "a.service")).0, Verdict::Unverified);
    }

    #[test]
    fn freeze_heal_waits_for_the_probe() {
        let mut v = v();
        let c = v
            .arm_with(
                "dec-1",
                "inc",
                "a.service",
                Some((23222, "/health".into())),
                100,
            )
            .unwrap();
        assert_eq!(c.to_json()["kind"], "freeze");
        // the held recovery after the restart does not verify a freeze heal
        assert!(v
            .observe(
                &[ev(
                    "unit.state",
                    "a.service",
                    135,
                    json!({"to": "active", "recovered": true})
                )],
                135
            )
            .is_empty());
        assert_eq!(v.pending.len(), 1);
        // a new crash still does
        let d = v.observe(&[ev("unit.failed", "a.service", 140, json!({}))], 140);
        assert_eq!(d[0].verdict, Verdict::FailedAgain);
        let up = StateRead::Probe {
            up: true,
            detail: "HTTP 200".into(),
        };
        assert_eq!(judge(&up).0, Verdict::Verified);
        assert_eq!(judge(&up).1, "probe");
        let down = StateRead::Probe {
            up: false,
            detail: "HTTP 503".into(),
        };
        assert_eq!(judge(&down).0, Verdict::Failed);
    }

    #[test]
    fn log_and_reload_rearms_only_unfinished() {
        let dir = std::env::temp_dir().join(format!("heal-verify-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("heals.jsonl");
        let _ = std::fs::write(&p, "");
        let mut v = Verifier::new(180, Some(p.clone()));
        v.arm_with(
            "dec-1",
            "inc",
            "a.service",
            Some((23222, "/health".into())),
            100,
        );
        let c = v.arm("dec-2", "inc", "b.service", 100).unwrap();
        v.pending.retain(|x| x.id != c.id);
        v.finish(c, Verdict::Verified, "nerve-recovery", "", 130);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .and_then(|mut f| std::io::Write::write_all(&mut f, b"{torn\n"))
            .unwrap();
        let mut w = Verifier::new(180, Some(p.clone()));
        assert_eq!(w.reload(1_000), 1);
        assert_eq!(w.pending[0].unit, "a.service");
        assert_eq!(w.pending[0].probe, Some((23222, "/health".to_string())));
        assert_eq!(w.pending[0].due, 1_030, "past deadline gets the grace");
        assert_eq!(w.counts.rearmed, 1);
        let mut off = Verifier::new(0, Some(p));
        assert_eq!(off.reload(1_000), 0);
    }
}
