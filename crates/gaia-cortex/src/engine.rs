// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The decision pipeline.
//!
//! ```text
//! events ──dedupe──> burst (coalesce window) ──> subjects ──> correlate
//!   (openmind_nerve::incident::Correlator, T3.2) ──> rules (deterministic)
//!   ──[ambiguous + high sev + advisor enabled]──> LLM advisor (allowlisted choices only)
//!   ──> action request to the nerve (dry-run by default) / alert sink / escalation queue
//! ```
//!
//! The decider thread blocks on its channel: with no events it does no work
//! and makes no calls (no timers). Every burst is one `batch`.

use crate::config::Config;
use crate::guard::Guard;
use crate::llm::{Advisor, Choice};
use crate::nerve::NerveLink;
use crate::rules::{subject_key, Limits, Outcome, Rules, Subject, Verdict};
use crate::sink::{Escalations, Jsonl};
use nerve_cortex_contract::{
    ActionKind, ActionRequest, ActionStatus, DecidedBy, Event, EventKind, CONTRACT_VERSION,
    CORTEX_SOURCE,
};
use openmind_nerve::incident::{correlate_with, Correlator, Explain, Sev, Signal};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SEEN_MAX: usize = 8192;

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub started: String,
    pub events_in: u64,
    pub events_duplicate: u64,
    pub events_ignored: u64,
    pub events_housekeeping: u64,
    pub events_invalid: u64,
    pub batches: u64,
    pub decisions: u64,
    pub decisions_suppressed: u64,
    pub actions_requested: u64,
    pub actions_simulated: u64,
    pub actions_executed: u64,
    pub actions_refused: u64,
    pub actions_rate_limited: u64,
    pub actions_circuit_open: u64,
    pub actions_failed: u64,
    /// Requests sent whose reply was lost (hung / slow nerve).
    pub actions_unknown: u64,
    /// Nerve restarts seen by the stream subscriber (seq went back / new instance).
    pub nerve_restarts: u64,
    pub alerts: u64,
    pub alerts_to_nerve: u64,
    pub escalations: u64,
    pub llm_calls: u64,
    pub llm_refused: u64,
    pub llm_skipped_disabled: u64,
    pub last_event: Option<String>,
    pub last_batch: Option<String>,
}

pub struct Cortex {
    pub cfg: Config,
    pub rules: Rules,
    pub correlator: Correlator,
    pub advisor: Advisor,
    pub nerve: Box<dyn NerveLink>,
    pub decisions: Jsonl,
    pub alerts: Jsonl,
    pub outbox: Option<std::path::PathBuf>,
    pub escalations: Escalations,
    pub stats: Stats,
    seen: HashSet<u64>,
    seen_order: VecDeque<u64>,
    last_said: BTreeMap<(String, &'static str), u64>,
    seq: u64,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds; every printed time goes through zagreb_time::from_unix)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn ts(t: u64) -> String {
    zagreb_time::from_unix(t as i64)
}

impl Cortex {
    pub fn new(cfg: Config, guard: Guard, nerve: Box<dyn NerveLink>) -> Self {
        let dir = cfg.data_dir.clone();
        let file = |n: &str| Some(dir.join(n));
        let rules = Rules::new(
            guard,
            &cfg.restart_allow,
            Limits {
                restarts_per_unit_hour: cfg.restarts_per_unit_hour,
                flap_failures: cfg.flap_failures.max(1),
                flap_window_s: cfg.flap_window_s,
                circuit_open_s: cfg.circuit_open_s,
            },
        );
        Cortex {
            advisor: Advisor::new(cfg.llm.clone(), &dir),
            rules,
            correlator: Correlator::default(),
            nerve,
            decisions: Jsonl::new(file("decisions.jsonl")),
            alerts: Jsonl::new(file("alerts.jsonl")),
            outbox: cfg.outbox.clone(),
            escalations: Escalations::new(file("escalations.jsonl")),
            stats: Stats {
                started: ts(now_secs()),
                ..Stats::default()
            },
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            last_said: BTreeMap::new(),
            seq: 0,
            cfg,
        }
    }

    fn next_id(&mut self, p: &str) -> String {
        self.seq += 1;
        format!("{p}-{}", self.seq)
    }

    /// The nerve behind the stream restarted: its `seq` numbering starts
    /// again, so the seq-based fingerprints of the old instance must not
    /// swallow the new instance's events (T5.1 chaos S8).
    pub fn nerve_restarted(&mut self) {
        self.stats.nerve_restarts += 1;
        self.seen.clear();
        self.seen_order.clear();
    }

    /// Dedupe and drop our own echoes. Returns the events worth deciding on.
    pub fn admit(&mut self, evs: Vec<Event>) -> Vec<Event> {
        let mut out = vec![];
        for e in evs {
            if e.kind() == EventKind::Housekeeping {
                self.stats.events_housekeeping += 1;
                continue;
            }
            self.stats.events_in += 1;
            if e.source == CORTEX_SOURCE || e.kind() == EventKind::CortexAlert {
                self.stats.events_ignored += 1;
                continue;
            }
            let fp = e.fingerprint();
            if !self.seen.insert(fp) {
                self.stats.events_duplicate += 1;
                continue;
            }
            self.seen_order.push_back(fp);
            while self.seen_order.len() > SEEN_MAX {
                if let Some(o) = self.seen_order.pop_front() {
                    self.seen.remove(&o);
                }
            }
            out.push(e);
        }
        out
    }

    /// Decide on one coalesced burst.
    pub fn process(&mut self, evs: Vec<Event>, now: u64) -> Vec<Value> {
        let evs = self.admit(evs);
        if evs.is_empty() {
            return vec![];
        }
        self.stats.batches += 1;
        self.stats.last_event = evs.last().map(|e| {
            if e.ts.is_empty() {
                ts(now)
            } else {
                e.ts.clone()
            }
        });
        self.stats.last_batch = Some(ts(now));
        // Subjects.
        let mut subjects: BTreeMap<String, Subject> = BTreeMap::new();
        for e in evs {
            let (key, unit) = subject_key(&e);
            let host = e.host().map(str::to_string);
            let s = subjects.entry(key.clone()).or_insert(Subject {
                key,
                unit,
                host,
                events: vec![],
            });
            s.events.push(e);
        }
        // Signals -> correlation (reused T3.2 correlator).
        let sigs: Vec<Signal> = subjects
            .values()
            .map(|s| {
                let sev = if s.recovered() { Sev::Info } else { s.sev() };
                let lead = s.lead().map(|e| e.kind.clone()).unwrap_or_default();
                let mut g =
                    Signal::new(&s.key, &s.key, sev, format!("{} x{}", lead, s.events.len()));
                g.hard = s.failures() > 0;
                g
            })
            .collect();
        let (grouped, _) = correlate_with(sigs.clone(), &[]);
        let mut home: BTreeMap<String, String> = BTreeMap::new();
        for (cause, ss) in &grouped {
            for s in ss {
                home.insert(s.component.clone(), cause.clone());
            }
        }
        let explain = |cause: &str, ss: &[Signal]| Explain {
            title: format!("{cause} ({} signal(s))", ss.len()),
            why: ss
                .iter()
                .map(|s| s.msg.clone())
                .collect::<Vec<_>>()
                .join("; "),
            next: String::new(),
        };
        self.correlator.update(now, sigs, &explain);
        let mut out = vec![];
        let mut stack_wide_done = false;
        for s in subjects.values() {
            let cause = home.get(&s.key).cloned().unwrap_or_else(|| s.key.clone());
            let stack_wide = cause == "stack-wide";
            if stack_wide && stack_wide_done {
                continue;
            }
            stack_wide_done |= stack_wide;
            let incident = self
                .correlator
                .open
                .get(&cause)
                .map(|i| i.id.clone())
                .unwrap_or_else(|| format!("info:{}", s.key));
            let v = self.rules.decide(s, stack_wide, now);
            if let Some(d) = self.act(s, &incident, v, now) {
                out.push(d);
            }
        }
        out
    }

    fn act(&mut self, s: &Subject, incident: &str, mut v: Verdict, now: u64) -> Option<Value> {
        // Duplicate alert/escalation for the same subject+rule within the cooldown.
        if v.outcome == Outcome::None && (v.alert || v.escalate) {
            let k = (s.key.clone(), v.rule);
            if let Some(t) = self.last_said.get(&k) {
                if now.saturating_sub(*t) < self.cfg.decision_cooldown_s {
                    self.stats.decisions_suppressed += 1;
                    return None;
                }
            }
            self.last_said.insert(k, now);
        }
        let mut llm = Value::Null;
        let mut decided_by = DecidedBy::Rules;
        if v.llm_candidate {
            let mut choices = vec![Choice::None, Choice::Alert, Choice::Escalate];
            if let Some(u) = &s.unit {
                if self.rules.restart_allowed(u) && !self.rules.circuit_open(u, now) {
                    choices.push(Choice::Restart(u.clone()));
                }
            }
            let summary = summary(s, incident, &v);
            match self.advisor.advise(v.sev, &summary, &choices, now) {
                Ok(a) => {
                    self.stats.llm_calls += 1;
                    llm = json!({"asked": true, "choice": a.choice.wire(), "reason": a.reason, "cost_usd": a.cost_usd});
                    match a.choice {
                        Choice::Restart(u)
                            if self.rules.restart_allowed(&u)
                                && !self.rules.circuit_open(&u, now) =>
                        {
                            v.outcome = Outcome::Restart(u);
                            v.escalate = false;
                            v.alert = true;
                            decided_by = DecidedBy::LlmAdvisor;
                        }
                        Choice::Restart(_) | Choice::Escalate => v.escalate = true,
                        Choice::Alert => {
                            v.escalate = false;
                            v.alert = true;
                        }
                        // "none" still leaves an alert: a high-severity
                        // incident is never silently dropped.
                        Choice::None => {
                            v.escalate = false;
                            v.alert = true;
                        }
                    }
                }
                Err(r) => {
                    if r.code() == "disabled" {
                        self.stats.llm_skipped_disabled += 1;
                    } else {
                        self.stats.llm_refused += 1;
                    }
                    llm = json!({"asked": false, "refused": r.code(), "why": r.text()});
                }
            }
        }
        let id = self.next_id("dec");
        let mut action = Value::Null;
        if let Outcome::Restart(unit) = &v.outcome {
            let req = ActionRequest {
                v: CONTRACT_VERSION,
                request_id: format!("{id}-{incident}"),
                source: CORTEX_SOURCE.to_string(),
                ts: ts(now),
                incident: incident.to_string(),
                action: ActionKind::Restart,
                unit: unit.clone(),
                dry_run: self.cfg.dry_run(),
                rule: v.rule.to_string(),
                decided_by,
                reason: v.reason.clone(),
            };
            self.stats.actions_requested += 1;
            let o = self.nerve.request_action(&req);
            match o.status {
                ActionStatus::Simulated => self.stats.actions_simulated += 1,
                ActionStatus::Executed => self.stats.actions_executed += 1,
                ActionStatus::Refused => self.stats.actions_refused += 1,
                ActionStatus::RateLimited => self.stats.actions_rate_limited += 1,
                ActionStatus::CircuitOpen => self.stats.actions_circuit_open += 1,
                ActionStatus::Failed => self.stats.actions_failed += 1,
            }
            // A request whose reply was lost (`by == "unknown"`) may still run:
            // it counts toward the cap like an executed one, and a human hears of it.
            let unknown = o.by == "unknown";
            if unknown {
                self.stats.actions_unknown += 1;
                v.escalate = true;
            }
            if unknown || matches!(o.status, ActionStatus::Simulated | ActionStatus::Executed) {
                self.rules.note_restart(unit, now);
            }
            if matches!(
                o.status,
                ActionStatus::RateLimited
                    | ActionStatus::CircuitOpen
                    | ActionStatus::Refused
                    | ActionStatus::Failed
            ) {
                v.escalate = true;
            }
            action = json!({"request": req, "status": o.status.as_str(), "by": o.by, "reason": o.reason});
        }
        self.stats.decisions += 1;
        let kinds: Vec<&str> = s.events.iter().map(|e| e.kind.as_str()).collect();
        let d = json!({
            "id": id, "ts": ts(now), "incident": incident, "subject": s.key, "unit": s.unit,
            "rule": v.rule, "severity": v.sev.as_str(), "reason": v.reason,
            "outcome": match &v.outcome { Outcome::Restart(u) => format!("restart:{u}"), Outcome::None => "none".into() },
            "decided_by": match decided_by { DecidedBy::Rules => "rules", DecidedBy::LlmAdvisor => "llm-advisor" },
            "dry_run": self.cfg.dry_run(), "action": action, "alert": v.alert, "escalate": v.escalate,
            "llm": llm, "events": s.events.len(), "kinds": kinds,
        });
        self.decisions.push(d.clone());
        if v.alert {
            self.alert(&id, s, incident, &v, now);
        }
        if v.escalate {
            self.escalate(&id, s, incident, &v, now);
        }
        eprintln!(
            "gaia-cortex {} {} {} {} -> {} ({})",
            ts(now),
            id,
            v.rule,
            s.key,
            d["outcome"].as_str().unwrap_or(""),
            d["action"]["status"].as_str().unwrap_or("no action")
        );
        Some(d)
    }

    fn alert(&mut self, dec: &str, s: &Subject, incident: &str, v: &Verdict, now: u64) {
        let id = self.next_id("al");
        let a = json!({"id": id, "ts": ts(now), "decision": dec, "incident": incident, "subject": s.key,
                       "rule": v.rule, "severity": v.sev.as_str(), "msg": v.reason});
        self.stats.alerts += 1;
        if let Some(p) = &self.outbox {
            let _ = crate::sink::append(p, &a);
        }
        let ev = Event::new(CORTEX_SOURCE, "cortex.alert", v.sev.as_str(), a.clone());
        if self.nerve.post_event(&ev) {
            self.stats.alerts_to_nerve += 1;
        }
        self.alerts.push(a);
    }

    fn escalate(&mut self, dec: &str, s: &Subject, incident: &str, v: &Verdict, now: u64) {
        let id = self.next_id("esc");
        let lead = s.lead().map(Event::to_json).unwrap_or(Value::Null);
        let e = json!({"id": id, "ts": ts(now), "decision": dec, "incident": incident, "subject": s.key,
                       "rule": v.rule, "severity": v.sev.as_str(), "reason": v.reason, "lead_event": lead,
                       "events": s.events.len()});
        self.stats.escalations += 1;
        let ev = Event::new(
            CORTEX_SOURCE,
            "cortex.escalation",
            v.sev.as_str(),
            json!({"id": id, "incident": incident, "subject": s.key, "rule": v.rule}),
        );
        let _ = self.nerve.post_event(&ev);
        self.escalations.push(e);
    }

    pub fn stats_json(&self) -> Value {
        let s = &self.stats;
        let llm = self.advisor.stats();
        json!({
            "started": s.started, "mode": if self.cfg.dry_run() { "dry-run" } else { "live" },
            "events_in": s.events_in, "events_duplicate": s.events_duplicate, "events_ignored": s.events_ignored, "events_housekeeping": s.events_housekeeping,
            "events_invalid": s.events_invalid, "batches": s.batches,
            "incidents_open": self.correlator.open.len(), "incidents_opened_total": self.correlator.opened_total,
            "decisions": s.decisions, "decisions_suppressed": s.decisions_suppressed,
            "actions_requested": s.actions_requested, "actions_simulated": s.actions_simulated,
            "actions_executed": s.actions_executed, "actions_refused": s.actions_refused,
            "actions_rate_limited": s.actions_rate_limited, "actions_circuit_open": s.actions_circuit_open, "actions_failed": s.actions_failed,
            "actions_unknown": s.actions_unknown, "nerve_restarts": s.nerve_restarts,
            "alerts": s.alerts, "alerts_to_nerve": s.alerts_to_nerve, "escalations": s.escalations,
            "escalations_open": self.escalations.open.len(),
            "llm_calls": s.llm_calls, "llm_refused": s.llm_refused, "llm_skipped_disabled": s.llm_skipped_disabled,
            "llm_spend_usd": llm["ledger"]["spent_usd"].clone(),
            "llm": llm,
            "last_event": s.last_event, "last_batch": s.last_batch,
        })
    }

    pub fn incidents_json(&self) -> Value {
        let mut v = self.correlator.to_json();
        v["units"] = self.rules.books_json(now_secs());
        v
    }
}

fn summary(s: &Subject, incident: &str, v: &Verdict) -> String {
    let evs: Vec<String> = s
        .events
        .iter()
        .take(8)
        .map(|e| {
            let d: String = e.data.to_string().chars().take(300).collect();
            format!("- {} {} {} {}", e.ts, e.kind, e.severity, d)
        })
        .collect();
    format!(
        "incident {incident}, subject {}, severity {}, rule {}: {}\nevents ({} total):\n{}",
        s.key,
        v.sev.as_str(),
        v.rule,
        v.reason,
        s.events.len(),
        evs.join("\n")
    )
}

/// Decider loop: block until an event arrives, then gather the burst for
/// `coalesce` and decide. No events = no wakeups.
pub fn run_decider(rx: Receiver<Vec<Event>>, cx: Arc<Mutex<Cortex>>, coalesce: Duration) {
    while let Ok(first) = rx.recv() {
        let mut burst = first;
        let deadline = Instant::now() + coalesce;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match rx.recv_timeout(left) {
                Ok(more) => burst.extend(more),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        let mut g = cx.lock().unwrap_or_else(|e| e.into_inner());
        g.process(burst, now_secs());
    }
}
