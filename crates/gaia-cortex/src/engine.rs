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
    sev_rank, ActionKind, ActionRequest, ActionStatus, DecidedBy, Event, EventKind,
    CONTRACT_VERSION, CORTEX_SOURCE,
};
use openmind_nerve::incident::{correlate_with, Correlator, Explain, Sev, Signal};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SEEN_MAX: usize = 8192;
const KEEP_RESOLVED: usize = 50;

/// A subject whose last signal was above info and has not cleared yet.
#[derive(Debug, Clone)]
struct Active {
    sig: Signal,
    /// The incident cause it was last grouped into (its own key, or
    /// `stack-wide` when it failed together with 2+ others).
    cause: String,
    /// Last batch that carried an event of this subject.
    last: u64,
    /// Units, nerve incidents and thresholds have a "cleared" signal and wait
    /// for it; one-shot subjects expire after `incident_quiet_s`.
    sticky: bool,
}

/// Subjects with an explicit "it is fine again" signal.
fn sticky(key: &str) -> bool {
    key.starts_with("unit:") || key.starts_with("nerve-incident:") || key.starts_with("threshold:")
}

/// Does this burst clear the subject's own signal? Only an info-level
/// burst can, and for the stateful kinds only the matching "fine again"
/// event: a unit back to active/running (or cleanly stopped), a nerve
/// incident whose last event is `incident.resolved`, a threshold back under.
fn clears(s: &Subject, sev: Sev) -> bool {
    if sev > Sev::Info {
        return false;
    }
    if s.unit.is_some() {
        return s.recovered()
            || (s.failures() == 0
                && s.events.iter().any(|e| {
                    e.kind() == EventKind::UnitState
                        && matches!(e.to_state(), Some("inactive" | "dead"))
                }));
    }
    if s.key.starts_with("nerve-incident:") {
        return s
            .events
            .last()
            .is_some_and(|e| e.kind.ends_with("resolved"));
    }
    true
}

fn data_str<'a>(e: &'a Event, k: &str) -> Option<&'a str> {
    e.data
        .get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

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
    /// Nerve incidents synthesized from `GET /incidents` on (re)connect:
    /// opened before the cortex listened (`backfilled`), or gone from the
    /// nerve while the cortex was not listening (`reconciled`).
    pub nerve_incidents_backfilled: u64,
    pub nerve_incidents_reconciled: u64,
    /// `incident.opened` for a nerve incident id the cortex already holds.
    pub nerve_incidents_duplicate: u64,
    /// Same cause, new nerve id (nerve restart): followed, not re-alerted.
    pub nerve_incidents_remapped: u64,
    /// Cortex incidents resolved because their own signal cleared / their
    /// one-shot subject stayed quiet for `incident_quiet_s`.
    pub incidents_resolved_cleared: u64,
    pub incidents_resolved_expired: u64,
    /// Sticky subjects past `incident_sticky_max_s` (off by default).
    pub incidents_resolved_stale: u64,
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
    /// Last non-info signal per subject that has not cleared yet (F3).
    active: BTreeMap<String, Active>,
    /// Nerve incident id -> nerve cause, for every open nerve incident the
    /// cortex knows (`incident.resolved` carries only the id).
    nerve_incs: BTreeMap<String, String>,
    /// Why recent cortex incidents resolved (id -> cleared / expired /
    /// quiet), newest last.
    resolved_by: VecDeque<(String, &'static str)>,
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
            active: BTreeMap::new(),
            nerve_incs: BTreeMap::new(),
            resolved_by: VecDeque::new(),
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
            let Some(e) = self.track_nerve_incident(e) else {
                continue;
            };
            out.push(e);
        }
        out
    }

    /// Nerve engine incidents: remember id -> cause, give `incident.resolved`
    /// (which carries only the id) its cause so it lands on the same
    /// subject, drop an `incident.opened` the cortex already holds (stream
    /// replay after a backfill, or the reverse), and follow a nerve restart's
    /// new id for a cause that is still open without alerting again.
    fn track_nerve_incident(&mut self, mut e: Event) -> Option<Event> {
        if e.kind() != EventKind::NerveIncident {
            return Some(e);
        }
        let Some(id) = data_str(&e, "id").map(str::to_string) else {
            return Some(e);
        };
        let resolved = e.kind.ends_with("resolved");
        if resolved {
            let cause = match data_str(&e, "cause") {
                Some(c) => Some(c.to_string()),
                None => self.nerve_incs.get(&id).cloned(),
            };
            self.nerve_incs.remove(&id);
            if let (Some(c), Some(o)) = (cause, e.data.as_object_mut()) {
                o.entry("cause").or_insert(Value::String(c));
            }
            return Some(e);
        }
        if self.nerve_incs.contains_key(&id) {
            if e.kind.ends_with("opened") {
                self.stats.nerve_incidents_duplicate += 1;
                return None;
            }
            return Some(e);
        }
        let Some(cause) = data_str(&e, "cause").map(str::to_string) else {
            return Some(e);
        };
        let old: Vec<String> = self
            .nerve_incs
            .iter()
            .filter(|(_, c)| **c == cause)
            .map(|(i, _)| i.clone())
            .collect();
        self.nerve_incs.insert(id, cause);
        if !old.is_empty() {
            for o in old {
                self.nerve_incs.remove(&o);
            }
            self.stats.nerve_incidents_remapped += 1;
            return None;
        }
        Some(e)
    }

    /// Events that bring the cortex up to date with the nerve engine's open
    /// incidents (`snapshot` = the nerve's `GET /incidents`), read on every
    /// stream (re)connect:
    ///
    /// - an open nerve incident the cortex does not know -> a synthetic
    ///   `incident.opened` (`data.backfill = true`), decided by R11 like a
    ///   streamed one (warn/error alert, critical escalate, never an action);
    /// - a nerve incident the cortex holds open that the nerve no longer has
    ///   open (resolved while the stream was down, or a restarted nerve that
    ///   no longer sees it) -> a synthetic `incident.resolved`.
    ///
    /// New incidents are not recorded here: the events go through
    /// [`Cortex::admit`] like any other, so a streamed copy of the same
    /// incident is dropped by id. Only a new id for a cause the cortex already
    /// holds (restarted nerve) is followed here, silently.
    pub fn backfill(&mut self, snapshot: &Value, now: u64) -> Vec<Event> {
        let Some(open) = snapshot.get("open").and_then(Value::as_array) else {
            return vec![];
        };
        let mut out = vec![];
        let mut causes: HashSet<String> = HashSet::new();
        let mut ids: HashSet<String> = HashSet::new();
        for i in open.iter().take(512) {
            let (Some(id), Some(cause)) = (
                i.get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty()),
                i.get("cause")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty()),
            ) else {
                continue;
            };
            let clip = |s: &str, n: usize| {
                s.chars()
                    .filter(|c| !c.is_control())
                    .take(n)
                    .collect::<String>()
            };
            let (id, cause) = (clip(id, 64), clip(cause, 120));
            ids.insert(id.clone());
            causes.insert(cause.clone());
            if self.nerve_incs.contains_key(&id) {
                continue;
            }
            // Same cause under an older id (the nerve restarted): follow it.
            let old: Vec<String> = self
                .nerve_incs
                .iter()
                .filter(|(_, c)| **c == cause)
                .map(|(i, _)| i.clone())
                .collect();
            if !old.is_empty() {
                for o in old {
                    self.nerve_incs.remove(&o);
                }
                self.nerve_incs.insert(id, cause);
                self.stats.nerve_incidents_remapped += 1;
                continue;
            }
            let sev = i.get("severity").and_then(Value::as_str).unwrap_or("warn");
            let sev = match sev_rank(sev) {
                0 => "info",
                1 => "warn",
                2 => "error",
                _ => "critical",
            };
            let mut ev = Event::new(
                "nerve",
                "incident.opened",
                sev,
                json!({
                    "id": id, "cause": cause, "backfill": true,
                    "components": i.get("components").cloned().unwrap_or(Value::Null),
                    "opened": i.get("opened").cloned().unwrap_or(Value::Null),
                }),
            );
            ev.ts = ts(now);
            ev.msg = i.get("title").and_then(Value::as_str).map(|t| clip(t, 300));
            self.stats.nerve_incidents_backfilled += 1;
            out.push(ev);
        }
        let gone: Vec<(String, String)> = self
            .nerve_incs
            .iter()
            .filter(|(i, c)| !ids.contains(*i) && !causes.contains(*c))
            .map(|(i, c)| (i.clone(), c.clone()))
            .collect();
        for (id, cause) in gone {
            let mut ev = Event::new(
                "nerve",
                "incident.resolved",
                "info",
                json!({"id": id, "cause": cause, "backfill": true}),
            );
            ev.ts = ts(now);
            self.stats.nerve_incidents_reconciled += 1;
            out.push(ev);
        }
        out
    }

    /// Decide on one coalesced burst.
    ///
    /// Incident lifetime (T5.1 F3 fix): every subject whose last signal was
    /// above info stays in `active` until its own signal clears ([`clears`])
    /// or, for one-shot subjects, `incident_quiet_s` passes. Active subjects
    /// that have no event in this burst are carried into the correlator, so
    /// bursts of unrelated subjects no longer close an open incident; an
    /// incident whose subject cleared is resolved in that same burst.
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
        // Signals of this burst.
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
        // Which subjects are still failing (F3). `ended` = (incident cause,
        // why) of the subjects whose own signal ended in this burst.
        let mut updated: HashSet<String> = HashSet::new();
        let mut ended: Vec<(String, &'static str)> = vec![];
        for (s, g) in subjects.values().zip(&sigs) {
            if g.sev > Sev::Info {
                updated.insert(s.key.clone());
                let cause = self
                    .active
                    .get(&s.key)
                    .map(|a| a.cause.clone())
                    .unwrap_or_else(|| s.key.clone());
                self.active.insert(
                    s.key.clone(),
                    Active {
                        sig: g.clone(),
                        cause,
                        last: now,
                        sticky: sticky(&s.key),
                    },
                );
            } else if clears(s, g.sev) {
                if let Some(a) = self.active.remove(&s.key) {
                    ended.push((a.cause, "cleared"));
                }
            }
        }
        let (quiet, max) = (self.cfg.incident_quiet_s, self.cfg.incident_sticky_max_s);
        let expired: Vec<(String, &'static str)> = self
            .active
            .iter()
            .filter(|(k, _)| !updated.contains(*k))
            .filter_map(|(k, a)| {
                let age = now.saturating_sub(a.last);
                if !a.sticky && age >= quiet {
                    Some((k.clone(), "expired"))
                } else if a.sticky && max > 0 && age >= max {
                    Some((k.clone(), "stale"))
                } else {
                    None
                }
            })
            .collect();
        for (k, why) in expired {
            if let Some(a) = self.active.remove(&k) {
                ended.push((a.cause, why));
            }
        }
        // Grouping of this burst (decisions: stack-wide = 3+ subjects failing
        // in this burst, exactly as before).
        let (grouped, _) = correlate_with(sigs.clone(), &[]);
        let mut home: BTreeMap<String, String> = BTreeMap::new();
        for (cause, ss) in &grouped {
            for s in ss {
                home.insert(s.component.clone(), cause.clone());
                if let Some(a) = self.active.get_mut(&s.component) {
                    a.cause = cause.clone();
                }
            }
        }
        let burst_stack_wide: HashSet<String> = grouped
            .get("stack-wide")
            .map(|ss| ss.iter().map(|s| s.component.clone()).collect())
            .unwrap_or_default();
        // Still-failing subjects without an event in this burst keep their
        // incident open: their last signal goes to the correlator under the
        // cause it was grouped into (not hard: they do not fold anew).
        let carried: Vec<Signal> = self
            .active
            .iter()
            .filter(|(k, _)| !updated.contains(*k))
            .map(|(k, a)| {
                let mut g = a.sig.clone();
                g.cause = a.cause.clone();
                g.component = k.clone();
                g.hard = false;
                g
            })
            .collect();
        let mut fed: BTreeMap<String, bool> = BTreeMap::new(); // cause -> has a burst signal
        for g in &sigs {
            if g.sev > Sev::Info {
                let c = home.get(&g.component).cloned().unwrap_or_default();
                fed.insert(c, true);
            }
        }
        for g in &carried {
            fed.entry(g.cause.clone()).or_insert(false);
        }
        // Incidents fed only by carried signals saw nothing new.
        let keep: Vec<(String, u64, u32)> = fed
            .iter()
            .filter(|(_, b)| !**b)
            .filter_map(|(c, _)| {
                self.correlator
                    .open
                    .get(c)
                    .map(|i| (c.clone(), i.last_seen, i.cycles))
            })
            .collect();
        let before: HashSet<String> = self
            .correlator
            .open
            .values()
            .map(|i| i.id.clone())
            .collect();
        let explain = |cause: &str, ss: &[Signal]| Explain {
            title: format!("{cause} ({} signal(s))", ss.len()),
            why: ss
                .iter()
                .map(|s| s.msg.clone())
                .collect::<Vec<_>>()
                .join("; "),
            next: String::new(),
        };
        let mut all = sigs;
        all.extend(carried);
        self.correlator.update(now, all, &explain);
        for (c, last, cycles) in keep {
            if let Some(i) = self.correlator.open.get_mut(&c) {
                i.last_seen = last;
                i.cycles = cycles;
            }
        }
        // Resolved by the correlator's own quiet rule.
        let quiet_ids: Vec<String> = self
            .correlator
            .resolved
            .iter()
            .filter(|i| before.contains(&i.id) && i.resolved == Some(now))
            .map(|i| i.id.clone())
            .collect();
        for id in quiet_ids {
            self.note_resolved(id, "quiet");
        }
        // Resolve the incidents whose own subjects all cleared / expired now.
        let mut done: Vec<(String, &'static str)> = vec![];
        for (cause, why) in ended {
            if !fed.contains_key(&cause)
                && self.correlator.open.contains_key(&cause)
                && !done.iter().any(|(c, _)| *c == cause)
            {
                done.push((cause, why));
            }
        }
        for (key, why) in done {
            if let Some(mut i) = self.correlator.open.remove(&key) {
                i.resolved = Some(now);
                let id = i.id.clone();
                self.correlator.resolved.push_back(i);
                while self.correlator.resolved.len() > KEEP_RESOLVED {
                    self.correlator.resolved.pop_front();
                }
                match why {
                    "cleared" => self.stats.incidents_resolved_cleared += 1,
                    "stale" => self.stats.incidents_resolved_stale += 1,
                    _ => self.stats.incidents_resolved_expired += 1,
                }
                self.note_resolved(id, why);
            }
        }
        let mut out = vec![];
        let mut stack_wide_done = false;
        for s in subjects.values() {
            let cause = home.get(&s.key).cloned().unwrap_or_else(|| s.key.clone());
            let stack_wide = burst_stack_wide.contains(&s.key);
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

    fn note_resolved(&mut self, id: String, why: &'static str) {
        self.resolved_by.push_back((id, why));
        while self.resolved_by.len() > KEEP_RESOLVED * 2 {
            self.resolved_by.pop_front();
        }
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
            "nerve_incidents_known": self.nerve_incs.len(), "nerve_incidents_backfilled": s.nerve_incidents_backfilled,
            "nerve_incidents_reconciled": s.nerve_incidents_reconciled, "nerve_incidents_duplicate": s.nerve_incidents_duplicate,
            "nerve_incidents_remapped": s.nerve_incidents_remapped, "subjects_active": self.active.len(),
            "incidents_resolved_cleared": s.incidents_resolved_cleared, "incidents_resolved_expired": s.incidents_resolved_expired, "incidents_resolved_stale": s.incidents_resolved_stale,
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
        let by: BTreeMap<&str, &str> = self
            .resolved_by
            .iter()
            .map(|(i, w)| (i.as_str(), *w))
            .collect();
        if let Some(rs) = v.get_mut("resolved").and_then(Value::as_array_mut) {
            for r in rs {
                let w = r
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|i| by.get(i))
                    .copied();
                r["resolved_by"] = json!(w.unwrap_or("quiet"));
            }
        }
        let now = now_secs();
        v["active"] = Value::Array(
            self.active
                .iter()
                .map(|(k, a)| json!({"subject": k, "severity": a.sig.sev.as_str(), "last": ts(a.last), "sticky": a.sticky,
                                     "expires": match (a.sticky, self.cfg.incident_sticky_max_s) { (false, _) => json!(ts(a.last + self.cfg.incident_quiet_s)), (true, 0) => Value::Null, (true, m) => json!(ts(a.last + m)) },
                                     "quiet_s": now.saturating_sub(a.last)}))
                .collect(),
        );
        v["nerve_incidents"] = json!(self.nerve_incs);
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
