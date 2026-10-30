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
//!
//! Nerve I/O never runs under the engine lock (slot 247): plan (lock) ->
//! call the nerve (no lock, one lost reply stops the rest of the batch) ->
//! commit (lock); alert/escalation posts go to a courier thread.

use crate::config::Config;
use crate::guard::Guard;
use crate::handoff::{Handoff, Offer};
use crate::llm::{Advisor, Choice};
use crate::nerve::{NerveLink, Outcome as NerveOutcome};
use crate::rules::{subject_key, Limits, Outcome, Rules, Subject, Verdict};
use crate::sink::{Escalations, Jsonl};
use crate::snapshot::Snapshot;
use nerve_cortex_contract::{
    sev_rank, ActionKind, ActionRequest, ActionStatus, DecidedBy, Event, EventKind,
    CONTRACT_VERSION, CORTEX_SOURCE,
};
use openmind_nerve::incident::{correlate_with, Correlator, Explain, Sev, Signal};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
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
    /// Repeat events from the nerve ring (folded duplicates, slot 235).
    pub events_repeat: u64,
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
    pub escalations: u64,
    /// Open escalations closed because their incident resolved (slot 417,
    /// `escalation_autoclose`) / because they reached `escalation_max_age_s`.
    pub escalations_closed_incident: u64,
    pub escalations_closed_expired: u64,
    /// Slot 431: open escalations rebuilt from `escalations.jsonl` at start,
    /// and lines skipped there (torn last line / not a JSON object).
    pub escalations_reloaded: u64,
    pub escalations_reload_skipped: u64,
    pub handoffs: u64,
    pub handoffs_duplicate: u64,
    pub handoffs_rate_limited: u64,
    pub handoffs_budget_stopped: u64,
    pub handoffs_failed: u64,
    pub handoffs_excluded: u64,
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
    pub handoff: Handoff,
    pub stats: Stats,
    /// API read snapshot, shared outside the engine lock (see [`crate::snapshot`]).
    pub snap: Arc<Snapshot>,
    /// Real nerve reachability (slot 347), shared with the nerve links and the API.
    pub contact: Arc<crate::contact::NerveContact>,
    /// Re-reads the nerve ring for folded repeats (set by `serve` when
    /// `repeat_poll_s` > 0 and the stream is on).
    pub repeats: Option<Arc<crate::repeat::RepeatWatch>>,
    /// Alert/escalation post counters, shared with the courier thread.
    pub posts: Arc<PostCounters>,
    /// Heal verification (did an executed restart fix the unit?).
    pub heals: crate::verify::Verifier,
    /// Freeze watch (running but not answering; see [`crate::freeze`]).
    pub freeze: crate::freeze::FreezeWatch,
    /// Failed-heal phone alerts (loopback ntfy relay), shared with the courier.
    pub phone: Option<Arc<crate::phone::Phone>>,
    outgoing: Vec<Post>,
    seen: HashSet<u64>,
    seen_order: VecDeque<u64>,
    last_said: BTreeMap<(String, &'static str), u64>,
    seq: u64,
    /// Slot 431: process epoch in escalation ids (`esc-<epoch>-<seq>`).
    esc_epoch: u64,
    /// Last non-info signal per subject that has not cleared yet (F3).
    active: BTreeMap<String, Active>,
    /// Nerve cause -> the nerve's short incident id (or the cause itself
    /// when only the stream named it), for every open nerve incident the
    /// cortex holds.
    nerve_incs: BTreeMap<String, String>,
    /// Why recent cortex incidents resolved (id -> cleared / expired /
    /// quiet), newest last.
    resolved_by: VecDeque<(String, &'static str)>,
}

/// Kinds of the heal outcome events the cortex posts (`gaia.heal.<outcome>`).
pub const HEAL_KIND_PREFIX: &str = "gaia.heal.";

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
        // Slot 431 (F-417-2/3): rebuild the open escalation queue from the
        // log, and pick an id epoch no earlier process in that log used.
        let (escalations, reload) = Escalations::load(file("escalations.jsonl"));
        let started = now_secs();
        let esc_epoch = reload.max_epoch.map_or(started, |m| started.max(m + 1));
        if reload.lines > 0 {
            eprintln!("gaia-cortex: escalations reloaded: {reload:?}, id epoch {esc_epoch}");
        }
        let mut heals = crate::verify::Verifier::new(cfg.heal_verify_s, file("heals.jsonl"));
        let rearmed = heals.reload(started);
        if rearmed > 0 {
            eprintln!("gaia-cortex: heal checks re-armed from heals.jsonl: {rearmed}");
        }
        let mut rules = Rules::new(
            guard,
            &cfg.restart_allow,
            Limits {
                restarts_per_unit_hour: cfg.restarts_per_unit_hour,
                restarts_total_hour: cfg.restarts_total_hour,
                flap_failures: cfg.flap_failures.max(1),
                flap_window_s: cfg.flap_window_s,
                circuit_open_s: cfg.circuit_open_s,
            },
        );
        rules.freeze_per_unit_hour = cfg.freeze.per_unit_hour;
        let freeze = crate::freeze::FreezeWatch::new(cfg.freeze.clone());
        Cortex {
            advisor: Advisor::new(cfg.llm.clone(), &dir),
            rules,
            correlator: Correlator::default(),
            nerve,
            decisions: Jsonl::new(file("decisions.jsonl")),
            alerts: Jsonl::new(file("alerts.jsonl")),
            outbox: cfg.outbox.clone(),
            escalations,
            handoff: Handoff::new(
                cfg.hermes.enabled,
                cfg.hermes
                    .inbox
                    .clone()
                    .unwrap_or_else(|| dir.join("hermes-inbox")),
                file("hermes-handoff-state.json"),
                cfg.hermes.per_hour,
                cfg.hermes.subject_window_s,
            )
            .with_exclude(cfg.hermes.exclude_units.clone()),
            repeats: None,
            stats: Stats {
                started: ts(now_secs()),
                escalations_reloaded: reload.open as u64,
                escalations_reload_skipped: reload.skipped as u64,
                ..Stats::default()
            },
            snap: Arc::new(Snapshot::default()),
            contact: Arc::new(crate::contact::NerveContact::default()),
            posts: Arc::new(PostCounters::default()),
            heals,
            freeze,
            phone: cfg
                .phone
                .clone()
                .map(|c| Arc::new(crate::phone::Phone::new(c))),
            outgoing: vec![],
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            last_said: BTreeMap::new(),
            seq: 0,
            esc_epoch,
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
        if let Some(w) = &self.repeats {
            w.clear();
        }
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
            // `unit.frozen` is internal only (crate::freeze): from outside it
            // could ask for a restart of a healthy unit. `gaia.heal.*` are our
            // own heal outcomes; the nerve re-emits what we POST (source
            // `http`, error -> `high`), and that echo must not be decided
            // again (it reached R99 and escalated a failed heal).
            if e.source == CORTEX_SOURCE
                || e.kind() == EventKind::CortexAlert
                || e.kind == crate::freeze::FREEZE_KIND
                || e.kind.starts_with(HEAL_KIND_PREFIX)
            {
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

    /// Nerve engine incidents. The nerve keeps one open incident per cause
    /// and its stream names it by cause: bb287cc publishes
    /// `incident.opened {"id": <cause>, "cause": <cause>, ...}` and
    /// `incident.resolved {"id": <cause>}` (no `cause` field), while
    /// `GET /incidents` shows the short id (`inc-d7320b38`). The cause is
    /// therefore the identity here: `incident.resolved` gets its `cause`
    /// filled in so it lands on the same subject as the opened one, and an
    /// `incident.opened` for a cause the cortex already holds (stream replay
    /// after a backfill, a restarted nerve's warm-up) is dropped.
    fn track_nerve_incident(&mut self, mut e: Event) -> Option<Event> {
        if e.kind() != EventKind::NerveIncident {
            return Some(e);
        }
        let id = data_str(&e, "id").map(str::to_string);
        let cause = data_str(&e, "cause").map(str::to_string).or_else(|| {
            // `id` is the cause on the stream; a short id from a snapshot
            // is mapped back.
            let id = id.as_deref()?;
            Some(
                self.nerve_incs
                    .iter()
                    .find(|(_, i)| i.as_str() == id)
                    .map(|(c, _)| c.clone())
                    .unwrap_or_else(|| id.to_string()),
            )
        });
        let Some(cause) = cause else {
            return Some(e);
        };
        if let Some(o) = e.data.as_object_mut() {
            o.entry("cause")
                .or_insert_with(|| Value::String(cause.clone()));
        }
        if e.kind.ends_with("resolved") {
            self.nerve_incs.remove(&cause);
            return Some(e);
        }
        if self.nerve_incs.contains_key(&cause) {
            if e.kind.ends_with("opened") {
                self.stats.nerve_incidents_duplicate += 1;
                return None;
            }
            return Some(e);
        }
        // Short id when known (backfill), else whatever the stream called it.
        let shown = e
            .data
            .get("nerve_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(id)
            .unwrap_or_else(|| cause.clone());
        self.nerve_incs.insert(cause, shown);
        Some(e)
    }

    /// Events that bring the cortex up to date with the nerve engine's open
    /// incidents (`snapshot` = the nerve's `GET /incidents`), read on every
    /// stream (re)connect:
    ///
    /// - an open nerve incident whose cause the cortex does not hold -> a
    ///   synthetic `incident.opened` (`data.backfill = true`), decided by R11
    ///   like a streamed one (warn/error alert, critical escalate, never an
    ///   action);
    /// - a cause the cortex holds open that the nerve no longer has open
    ///   (resolved while the stream was down, or a restarted nerve that no
    ///   longer sees it) -> a synthetic `incident.resolved`.
    ///
    /// New incidents are not recorded here: the events go through
    /// [`Cortex::admit`] like any other, so a streamed copy of the same
    /// incident is dropped by cause. A held cause that the nerve now shows
    /// under another short id (restarted nerve) is followed silently.
    pub fn backfill(&mut self, snapshot: &Value, now: u64) -> Vec<Event> {
        let Some(open) = snapshot.get("open").and_then(Value::as_array) else {
            return vec![];
        };
        let clip = |s: &str, n: usize| {
            s.chars()
                .filter(|c| !c.is_control())
                .take(n)
                .collect::<String>()
        };
        let mut out = vec![];
        let mut causes: HashSet<String> = HashSet::new();
        for i in open.iter().take(512) {
            let Some(cause) = i
                .get("cause")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let cause = clip(cause, 120);
            let id = i
                .get("id")
                .and_then(Value::as_str)
                .map(|s| clip(s, 64))
                .unwrap_or_default();
            if !causes.insert(cause.clone()) {
                continue;
            }
            if let Some(held) = self.nerve_incs.get_mut(&cause) {
                if !id.is_empty() && *held != id {
                    if held.starts_with("inc-") {
                        self.stats.nerve_incidents_remapped += 1;
                    }
                    *held = id;
                }
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
                    "id": cause, "cause": cause, "nerve_id": id, "backfill": true,
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
            .filter(|(c, _)| !causes.contains(*c))
            .map(|(c, i)| (c.clone(), i.clone()))
            .collect();
        for (cause, id) in gone {
            let mut ev = Event::new(
                "nerve",
                "incident.resolved",
                "info",
                json!({"id": cause, "cause": cause, "nerve_id": id, "backfill": true}),
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
    /// Decide on one coalesced burst: plan, call the nerve, commit, deliver
    /// the alert/escalation posts. One thread, no lock: the replay CLI and
    /// the unit tests use it. The service's decider runs the same phases
    /// with the engine lock released around the nerve calls
    /// ([`run_decider`], slot 247).
    pub fn process(&mut self, evs: Vec<Event>, now: u64) -> Vec<Value> {
        let plan = self.plan(evs, now);
        let outcomes = call_actions(&mut *self.nerve, &plan);
        let out = self.commit(plan, outcomes, now);
        for p in self.take_posts() {
            let ok = self.nerve.post_event(&p.event);
            self.posts.note(p.alert, ok);
        }
        out
    }

    /// Freeze watch bookkeeping for one burst: a systemd lifecycle event
    /// (start, stop, crash, exit, restart, recovery) starts the unit's
    /// freeze cooldown (a deploy restart is not a freeze); fired freezes
    /// join the burst unless their unit just had such an event.
    fn freeze_merge(&mut self, evs: &mut Vec<Event>, now: u64) {
        if !self.freeze.enabled() {
            self.freeze.inbox.clear();
            return;
        }
        let until = now + self.freeze.cfg.cooldown_s;
        let mut moved: HashSet<String> = HashSet::new();
        for e in evs.iter() {
            let life = matches!(
                e.kind(),
                EventKind::UnitState
                    | EventKind::UnitCrash
                    | EventKind::RestartScheduled
                    | EventKind::StartLimit
                    | EventKind::ProcessExit
                    | EventKind::Oom
            );
            if let (true, Some(u)) = (life, e.unit()) {
                let u = crate::freeze::norm_unit(&u);
                self.freeze.cooldown(&u, until);
                moved.insert(u);
            }
        }
        for f in std::mem::take(&mut self.freeze.inbox) {
            let u = f.unit.as_deref().map(crate::freeze::norm_unit);
            if u.as_ref().is_some_and(|u| moved.contains(u)) {
                self.freeze.counts.cooldowns += 1;
                continue;
            }
            evs.push(f);
        }
    }

    /// Phase 1 (engine lock held, no I/O to the nerve): admit, correlate,
    /// decide every subject, build the action requests. A restart request
    /// is reserved against its unit's hourly cap here, so the cap can never
    /// be passed while the call is in flight; [`Cortex::commit`] releases
    /// the reservation when the nerve says the restart did not run.
    pub fn plan(&mut self, evs: Vec<Event>, now: u64) -> Vec<Pending> {
        self.stats.events_invalid += self.snap.take_invalid();
        self.expire_escalations(now);
        let mut evs = self.admit(evs);
        self.freeze_merge(&mut evs, now);
        if evs.is_empty() {
            return vec![];
        }
        for d in self.heals.observe(&evs, now) {
            self.heal_done(d, now);
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
        for e in &evs {
            if e.repeat_of().is_some() {
                self.stats.events_repeat += 1;
            } else if let Some(w) = &self.repeats {
                w.arm(e, now);
            }
        }
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
                // A planned stop (slot 309) is not a failing signal: no incident.
                let planned = self.rules.planned_stop(s, now).is_some();
                let sev = if s.recovered() || planned {
                    Sev::Info
                } else {
                    s.sev()
                };
                let lead = s.lead().map(|e| e.kind.clone()).unwrap_or_default();
                let mut g =
                    Signal::new(&s.key, &s.key, sev, format!("{} x{}", lead, s.events.len()));
                g.hard = s.failures() > 0 && !planned;
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
            self.note_resolved(id, "quiet", now);
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
                self.note_resolved(id, why, now);
            }
        }
        let mut out = vec![];
        let mut stack_wide_done = false;
        for s in subjects.into_values() {
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
            let v = self.rules.decide(&s, stack_wide, now);
            if let Some(p) = self.plan_one(s, incident, v, now) {
                out.push(p);
            }
        }
        out
    }

    /// Close open escalations older than `escalation_max_age_s` (slot 417).
    /// Runs at the start of every burst and, while a max age is set, on the
    /// decider's idle sweep ([`escalation_sweep_every`]). Returns how many.
    pub fn expire_escalations(&mut self, now: u64) -> usize {
        if self.cfg.escalation_max_age_s == 0 {
            return 0;
        }
        let n = self
            .escalations
            .expire(now, self.cfg.escalation_max_age_s, &ts(now));
        self.stats.escalations_closed_expired += n as u64;
        n
    }

    fn note_resolved(&mut self, id: String, why: &'static str, now: u64) {
        if self.cfg.escalation_autoclose {
            let n = self
                .escalations
                .close_incident(&id, &format!("incident-{why}"), &ts(now));
            self.stats.escalations_closed_incident += n as u64;
        }
        self.resolved_by.push_back((id, why));
        while self.resolved_by.len() > KEEP_RESOLVED * 2 {
            self.resolved_by.pop_front();
        }
    }

    fn plan_one(
        &mut self,
        s: Subject,
        incident: String,
        mut v: Verdict,
        now: u64,
    ) -> Option<Pending> {
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
            let summary = summary(&s, &incident, &v);
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
        let mut req = None;
        if let Outcome::Restart(unit) = &v.outcome {
            req = Some(ActionRequest {
                v: CONTRACT_VERSION,
                request_id: format!("{id}-{incident}"),
                source: CORTEX_SOURCE.to_string(),
                ts: ts(now),
                incident: incident.clone(),
                action: ActionKind::Restart,
                unit: unit.clone(),
                dry_run: self.cfg.dry_run(),
                rule: v.rule.to_string(),
                decided_by,
                reason: v.reason.clone(),
            });
            self.stats.actions_requested += 1;
            // Reserve the cap slot before the call (released in commit).
            self.rules.note_restart(unit, now);
        }
        Some(Pending {
            id,
            subject: s,
            incident,
            verdict: v,
            llm,
            decided_by,
            req,
        })
    }

    /// Phase 3 (engine lock held again): account the nerve's answers,
    /// record the decisions, queue the alert/escalation posts (sent later
    /// by [`Cortex::take_posts`]'s caller, outside the lock).
    pub fn commit(
        &mut self,
        plan: Vec<Pending>,
        outcomes: Vec<Option<NerveOutcome>>,
        now: u64,
    ) -> Vec<Value> {
        let mut out = vec![];
        let mut outcomes = outcomes.into_iter();
        for p in plan {
            let o = outcomes.next().flatten();
            out.push(self.commit_one(p, o, now));
        }
        out
    }

    fn commit_one(&mut self, p: Pending, o: Option<NerveOutcome>, now: u64) -> Value {
        let Pending {
            id,
            subject: s,
            incident,
            verdict: mut v,
            llm,
            decided_by,
            req,
        } = p;
        let mut action = Value::Null;
        let mut action_heal = Value::Null;
        if let Some(req) = req {
            // No outcome = the call phase never ran (cannot happen; be safe).
            let o = o.unwrap_or_else(|| crate::nerve::not_sent(&req, "no call made"));
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
            if !(unknown || matches!(o.status, ActionStatus::Simulated | ActionStatus::Executed)) {
                self.rules.unnote_restart(&req.unit, now);
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
            if !req.dry_run && (unknown || o.status == ActionStatus::Executed) {
                // our own restart: never mistaken for a freeze right after
                let cool = now + self.freeze.cfg.cooldown_s;
                self.freeze.cooldown(&req.unit, cool);
                let probe = (v.rule == "R13-frozen")
                    .then(|| self.freeze.target(&req.unit))
                    .flatten()
                    .map(|t| (t.port, t.path.clone()));
                if let Some(c) = self.heals.arm_with(&id, &incident, &req.unit, probe, now) {
                    action_heal = json!({"check": c.id, "due": ts(c.due)});
                }
            }
            action = json!({"request": req, "status": o.status.as_str(), "by": o.by, "reason": o.reason});
            if !action_heal.is_null() {
                action["verify"] = action_heal;
            }
        }
        self.stats.decisions += 1;
        let kinds: Vec<&str> = s.events.iter().map(|e| e.kind.as_str()).collect();
        let d = json!({
            "id": id, "ts": ts(now), "incident": incident, "subject": s.key, "unit": s.unit,
            "rule": v.rule, "severity": v.sev.as_str(), "reason": v.reason,
            "outcome": match &v.outcome { Outcome::Restart(u) => format!("restart:{u}"), Outcome::None => "none".into() },
            "decided_by": match decided_by { DecidedBy::Rules => "rules", DecidedBy::LlmAdvisor => "llm-advisor" },
            "dry_run": self.cfg.dry_run(), "action": action, "alert": v.alert, "escalate": v.escalate,
            "llm": llm, "events": s.events.len(), "failures": s.failures(), "kinds": kinds,
            "repeat_of": s.events.iter().filter_map(Event::repeat_of).collect::<Vec<_>>(),
        });
        self.decisions.push(d.clone());
        if v.alert {
            self.alert(&id, &s, &incident, &v, now);
        }
        if v.escalate {
            self.escalate(&id, &s, &incident, &v, now);
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
        d
    }

    /// Due heal checks (removed from the pending book): the caller reads the
    /// nerve `/state` for each OUTSIDE the engine lock, then calls
    /// [`Cortex::heal_judged`].
    pub fn heal_due(&mut self, now: u64) -> Vec<crate::verify::Check> {
        self.heals.take_due(now)
    }

    /// Finish a due check from its `/state` read.
    pub fn heal_judged(
        &mut self,
        c: crate::verify::Check,
        read: &crate::verify::StateRead,
        now: u64,
    ) {
        let (verdict, by, detail) = crate::verify::judge(read);
        let d = self.heals.finish(c, verdict, &by, &detail, now);
        self.heal_done(d, now);
    }

    /// Side effects of a finished heal check: a `gaia.heal.<outcome>` event
    /// to the nerve; `failed` also opens the unit's circuit (no more blind
    /// restarts) and alerts; `unverified` alerts. Neither escalates to a
    /// human / Hermes (Mate, 2026-10-30 11:27 CET: failed heal = alert only).
    fn heal_done(&mut self, d: crate::verify::Done, now: u64) {
        use crate::verify::Verdict as H;
        let u = d.check.unit.clone();
        let sev = match d.verdict {
            H::Verified => "info",
            H::FailedAgain | H::Unverified => "warn",
            H::Failed => "error",
        };
        eprintln!(
            "gaia-cortex {} heal {} {} -> {} ({}: {})",
            ts(now),
            d.check.id,
            u,
            d.verdict.as_str(),
            d.by,
            d.detail
        );
        let mut data = d.to_json();
        data["unit"] = json!(u);
        let mut ev = Event::new(
            CORTEX_SOURCE,
            &format!("{HEAL_KIND_PREFIX}{}", d.verdict.as_str()),
            sev,
            data,
        );
        ev.unit = Some(u.clone());
        self.outgoing.push(Post {
            event: ev,
            alert: false,
        });
        let (rule, vsev, why) = match d.verdict {
            H::Failed => {
                let fresh = self.rules.hold_circuit(&u, now);
                (
                    "R12-heal-failed",
                    Sev::Critical,
                    format!(
                        "{u}: restart {} did not heal it within {}s ({}); circuit {}, no further restart",
                        d.check.decision,
                        d.at.saturating_sub(d.check.asked),
                        d.detail,
                        if fresh { "opened" } else { "kept open" }
                    ),
                )
            }
            H::Unverified => (
                "R12-heal-unverified",
                Sev::Warn,
                format!(
                    "{u}: restart {} could not be verified ({}: {})",
                    d.check.decision, d.by, d.detail
                ),
            ),
            H::Verified | H::FailedAgain => return,
        };
        let s = Subject {
            key: format!("unit:{u}"),
            unit: Some(u.clone()),
            host: None,
            events: vec![],
        };
        let mut v = Verdict::new(rule, vsev, why);
        v.alert = true;
        v.escalate = false;
        let id = self.next_id("dec");
        self.stats.decisions += 1;
        let rec = json!({
            "id": id, "ts": ts(now), "incident": d.check.incident, "subject": s.key, "unit": u,
            "rule": rule, "severity": vsev.as_str(), "reason": v.reason, "outcome": "none",
            "decided_by": "heal-verify", "dry_run": self.cfg.dry_run(), "action": Value::Null,
            "alert": v.alert, "escalate": v.escalate, "llm": Value::Null, "events": 0, "failures": 0,
            "kinds": Vec::<String>::new(), "repeat_of": Vec::<String>::new(), "heal": d.to_json(),
        });
        self.decisions.push(rec);
        self.alert(&id, &s, &d.check.incident, &v, now);
    }

    pub fn heals_json(&self) -> Value {
        let mut v = self.heals.to_json();
        v["freeze"] = self.freeze.to_json();
        v["phone"] = match &self.phone {
            Some(p) => p.to_json(),
            None => json!({"configured": false}),
        };
        v
    }

    /// Alert/escalation events waiting to be posted to the nerve.
    pub fn take_posts(&mut self) -> Vec<Post> {
        std::mem::take(&mut self.outgoing)
    }

    fn alert(&mut self, dec: &str, s: &Subject, incident: &str, v: &Verdict, now: u64) {
        let id = self.next_id("al");
        let a = json!({"id": id, "ts": ts(now), "decision": dec, "incident": incident, "subject": s.key,
                       "rule": v.rule, "severity": v.sev.as_str(), "msg": v.reason});
        self.stats.alerts += 1;
        if let Some(p) = &self.outbox {
            let _ = crate::sink::append(p, &a);
        }
        let event = Event::new(CORTEX_SOURCE, "cortex.alert", v.sev.as_str(), a.clone());
        self.outgoing.push(Post { event, alert: true });
        self.alerts.push(a);
    }

    fn escalate(&mut self, dec: &str, s: &Subject, incident: &str, v: &Verdict, now: u64) {
        // Slot 431 (D-417-3): id esc-<process epoch>-<seq>, unique across restarts (next_id bumps seq).
        self.next_id("esc");
        let id = format!("esc-{}-{}", self.esc_epoch, self.seq);
        let lead = s.lead().map(Event::to_json).unwrap_or(Value::Null);
        let mut e = json!({"id": id, "ts": ts(now), "decision": dec, "incident": incident, "subject": s.key,
                       "rule": v.rule, "severity": v.sev.as_str(), "reason": v.reason, "lead_event": lead,
                       "events": s.events.len()});
        self.stats.escalations += 1;
        let offer = self.hand_to_hermes(&e, s, now);
        e["handoff"] = json!(offer.code());
        if let Offer::Written(h) = &offer {
            e["handoff_id"] = json!(h);
        }
        if let Offer::Excluded(p) = &offer {
            e["handoff_excluded_by"] = json!(p);
        }
        let event = Event::new(
            CORTEX_SOURCE,
            "cortex.escalation",
            v.sev.as_str(),
            json!({"id": id, "incident": incident, "subject": s.key, "rule": v.rule}),
        );
        self.outgoing.push(Post {
            event,
            alert: false,
        });
        self.escalations.push_at(e, now);
    }

    /// Hand an escalation to the Hermes boss inbox (once per incident, capped).
    fn hand_to_hermes(&mut self, esc: &Value, s: &Subject, now: u64) -> Offer {
        if !self.handoff.enabled {
            return Offer::Disabled;
        }
        let recent: Vec<Value> = self
            .decisions
            .ring
            .iter()
            .rev()
            .filter(|d| d["subject"] == s.key.as_str())
            .take(6)
            .cloned()
            .collect();
        let evs: Vec<Value> = s.events.iter().take(12).map(Event::to_json).collect();
        let ticket = json!({
            "v": 1, "kind": "cortex.handoff", "ts": ts(now), "to": "hermes-boss",
            "incident": esc["incident"], "subject": esc["subject"], "unit": s.unit, "host": s.host,
            "rule": esc["rule"], "severity": esc["severity"], "reason": esc["reason"],
            "escalation": esc, "events": evs, "recent_decisions": recent,
            "cortex": {"mode": if self.cfg.dry_run() { "dry-run" } else { "live" },
                       "allowed_action": "restart via the nerve allowlist (already tried or not applicable)",
                       "restarts_per_unit_hour": self.cfg.restarts_per_unit_hour,
                       "circuit_open_s": self.cfg.circuit_open_s,
                       "unit_books": self.rules.books_json(now)},
            "ask": "diagnose, propose a fix (code or config), notify Mate, change NOTHING until Mate approves",
        });
        let o = self.handoff.offer(&ticket, now);
        match &o {
            Offer::Written(_) => self.stats.handoffs += 1,
            Offer::Duplicate => self.stats.handoffs_duplicate += 1,
            Offer::RateLimited => self.stats.handoffs_rate_limited += 1,
            Offer::BudgetStopped => self.stats.handoffs_budget_stopped += 1,
            Offer::Excluded(_) => self.stats.handoffs_excluded += 1,
            Offer::Failed(e) => {
                self.stats.handoffs_failed += 1;
                eprintln!("gaia-cortex: hermes handoff failed: {e}");
            }
            Offer::Disabled => {}
        }
        o
    }

    pub fn stats_json(&self) -> Value {
        let s = &self.stats;
        let llm = self.advisor.stats();
        // Two json! calls: one macro with every field hits serde_json's recursion limit.
        let mut v = json!({
            "started": s.started, "mode": if self.cfg.dry_run() { "dry-run" } else { "live" },
            "events_in": s.events_in, "events_duplicate": s.events_duplicate, "events_ignored": s.events_ignored, "events_housekeeping": s.events_housekeeping,
            "events_invalid": s.events_invalid + self.snap.pending_invalid(), "batches": s.batches,
            "incidents_open": self.correlator.open.len(), "incidents_opened_total": self.correlator.opened_total,
            "decisions": s.decisions, "decisions_suppressed": s.decisions_suppressed,
            "actions_requested": s.actions_requested, "actions_simulated": s.actions_simulated,
            "actions_executed": s.actions_executed, "actions_refused": s.actions_refused,
            "actions_rate_limited": s.actions_rate_limited, "actions_circuit_open": s.actions_circuit_open, "actions_failed": s.actions_failed,
            "actions_unknown": s.actions_unknown, "nerve_restarts": s.nerve_restarts, "events_repeat": s.events_repeat,
            "restarts_last_hour": self.rules.restarts_last_hour(now_secs()),
            "caps": {"per_unit_hour": self.rules.limits.restarts_per_unit_hour, "total_hour": self.rules.limits.restarts_total_hour,
                     "flap_failures": self.rules.limits.flap_failures, "flap_window_s": self.rules.limits.flap_window_s},
            "repeat_watch": self.repeats.as_ref().map(|w| w.to_json(self.cfg.repeat_poll_s)),
            "alerts": s.alerts, "alerts_to_nerve": self.posts.alerts_ok(), "nerve_posts": self.posts.json(), "escalations": s.escalations,
            "escalations_open": self.escalations.open.len(),
            "escalations_closed_incident": s.escalations_closed_incident, "escalations_closed_expired": s.escalations_closed_expired,
            "escalations_reloaded": s.escalations_reloaded, "escalations_reload_skipped": s.escalations_reload_skipped,
            "escalation_epoch": self.esc_epoch,
        });
        let more = json!({
            "nerve_incidents_known": self.nerve_incs.len(), "nerve_incidents_backfilled": s.nerve_incidents_backfilled,
            "nerve_incidents_reconciled": s.nerve_incidents_reconciled, "nerve_incidents_duplicate": s.nerve_incidents_duplicate,
            "nerve_incidents_remapped": s.nerve_incidents_remapped, "subjects_active": self.active.len(),
            "incidents_resolved_cleared": s.incidents_resolved_cleared, "incidents_resolved_expired": s.incidents_resolved_expired, "incidents_resolved_stale": s.incidents_resolved_stale,
            "handoffs": s.handoffs, "handoffs_duplicate": s.handoffs_duplicate, "handoffs_rate_limited": s.handoffs_rate_limited,
            "handoffs_budget_stopped": s.handoffs_budget_stopped, "handoffs_failed": s.handoffs_failed, "handoffs_excluded": s.handoffs_excluded,
            "hermes_handoff": self.handoff.to_json(now_secs()),
            "llm_calls": s.llm_calls, "llm_refused": s.llm_refused, "llm_skipped_disabled": s.llm_skipped_disabled,
            "llm_spend_usd": llm["ledger"]["spent_usd"].clone(),
            "llm": llm,
            "last_event": s.last_event, "last_batch": s.last_batch,
            "heals": self.heals.summary_json(),
        });
        if let (Some(o), Value::Object(m)) = (v.as_object_mut(), more) {
            o.extend(m);
        }
        v
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

    pub fn health_json(&self) -> Value {
        // `nerve` = real reachability (slot 347, F-337-1), not just "configured".
        // `commit` / `build` = deterministic build stamp (slot 377, D-377-2).
        let mut v = json!({"ok": true, "service": "gaia-cortex", "version": env!("CARGO_PKG_VERSION"), "commit": crate::build_info::COMMIT, "build": crate::build_info::json(), "ts": ts(now_secs()),
               "mode": if self.cfg.dry_run() { "dry-run" } else { "live" }, "llm_enabled": self.cfg.llm.enabled,
               "events_in": self.stats.events_in,
               "incidents_open": self.correlator.open.len(), "escalations_open": self.escalations.open.len(),
               "hermes_handoff": self.handoff.enabled, "handoffs": self.stats.handoffs,
               "heals_pending": self.heals.pending.len(), "heal_verify": self.heals.window_s > 0,
               "freeze_watch": self.freeze.enabled(), "freeze_targets": self.freeze.targets.len(),
               "phone_alerts": self.phone.is_some(),
               "busy": self.snap.busy()});
        self.contact.overlay(
            &mut v,
            self.cfg.nerve_url.is_some(),
            self.cfg.nerve_stale_eff(),
        );
        v
    }

    pub fn escalations_json(&self) -> Value {
        let closed: Vec<&Value> = self.escalations.closed.iter().rev().take(20).collect();
        json!({"open": self.escalations.open, "total": self.escalations.log.total,
               "closed_total": self.escalations.closed_total, "closed_recent": closed})
    }

    /// Publish the API read snapshot (after every batch and every API
    /// mutation), so reads never have to wait for the next batch.
    pub fn publish(&self) {
        self.snap.store(
            self.health_json(),
            self.stats_json(),
            self.incidents_json(),
            self.decisions.recent(usize::MAX),
            self.alerts.recent(usize::MAX),
            self.escalations_json(),
        );
        self.snap.store_heals(self.heals_json());
    }

    /// Refresh only the `/heals` snapshot (freeze watch rounds).
    pub fn publish_heals(&self) {
        self.snap.store_heals(self.heals_json());
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

/// One subject decided in [`Cortex::plan`], waiting for its nerve call.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: String,
    pub subject: Subject,
    pub incident: String,
    pub verdict: Verdict,
    pub llm: Value,
    pub decided_by: DecidedBy,
    /// The restart request to send (`None` = no action, nothing to call).
    pub req: Option<ActionRequest>,
}

/// An alert or escalation event for the nerve (`POST /event`).
#[derive(Debug, Clone)]
pub struct Post {
    pub event: Event,
    /// true = `cortex.alert` (counted in `alerts_to_nerve`), false = escalation.
    pub alert: bool,
}

/// Outcome counters of the alert/escalation posts, shared with the courier
/// thread (lock-free).
#[derive(Debug, Default)]
pub struct PostCounters {
    alerts_ok: AtomicU64,
    escalations_ok: AtomicU64,
    failed: AtomicU64,
    dropped: AtomicU64,
    queued: AtomicU64,
}

impl PostCounters {
    pub fn note(&self, alert: bool, ok: bool) {
        let c = match (ok, alert) {
            (true, true) => &self.alerts_ok,
            (true, false) => &self.escalations_ok,
            (false, _) => &self.failed,
        };
        c.fetch_add(1, Ordering::Relaxed);
    }
    pub fn alerts_ok(&self) -> u64 {
        self.alerts_ok.load(Ordering::Relaxed)
    }
    pub fn json(&self) -> Value {
        let g = |c: &AtomicU64| c.load(Ordering::Relaxed);
        json!({"alerts_ok": g(&self.alerts_ok), "escalations_ok": g(&self.escalations_ok),
               "failed": g(&self.failed), "dropped": g(&self.dropped), "queued": g(&self.queued)})
    }
}

/// Longest courier queue; beyond it new posts are dropped and counted
/// (`nerve_posts.dropped`). Alerts also live in alerts.jsonl / the outbox,
/// so a drop loses only the nerve-side copy.
pub const COURIER_CAP: usize = 1024;

/// Sends alert/escalation posts to the nerve on its own thread, so a slow
/// or hung nerve (3 s timeout per post) never delays a decision.
pub struct Courier {
    tx: SyncSender<Post>,
    counters: Arc<PostCounters>,
}

impl Courier {
    pub fn spawn(link: Box<dyn NerveLink>, counters: Arc<PostCounters>, cap: usize) -> Self {
        Self::spawn_with(link, counters, cap, None)
    }

    /// [`Courier::spawn`] that also offers every post to the phone relay
    /// (only failed-heal alerts go out, see [`crate::phone`]).
    pub fn spawn_with(
        mut link: Box<dyn NerveLink>,
        counters: Arc<PostCounters>,
        cap: usize,
        phone: Option<Arc<crate::phone::Phone>>,
    ) -> Self {
        let (tx, rx) = sync_channel::<Post>(cap);
        let c = counters.clone();
        std::thread::spawn(move || {
            while let Ok(p) = rx.recv() {
                if let Some(ph) = &phone {
                    let r = ph.offer(&p.event, now_secs());
                    if !matches!(r, crate::phone::Sent::NotForPhone) {
                        eprintln!(
                            "gaia-cortex: phone alert ({}): {r:?}",
                            crate::phone::PHONE_RULE
                        );
                    }
                }
                let ok = link.post_event(&p.event);
                c.note(p.alert, ok);
                c.queued.fetch_sub(1, Ordering::Relaxed);
            }
        });
        Courier { tx, counters }
    }

    /// Never blocks: a full queue drops the post (counted).
    pub fn send(&self, p: Post) {
        self.counters.queued.fetch_add(1, Ordering::Relaxed);
        match self.tx.try_send(p) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.counters.queued.fetch_sub(1, Ordering::Relaxed);
                self.counters.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Phase 2 (no engine lock): send the batch's action requests. Per-batch
/// breaker: once one request is sent and its reply is lost (hung or slow
/// nerve), the rest of the batch is not sent but answered locally (dry-run:
/// simulated; live: refused and escalated), so one hung nerve costs a batch
/// at most one request timeout (3 s), not 3 s per subject. The next batch
/// tries the nerve again.
pub fn call_actions(link: &mut dyn NerveLink, plan: &[Pending]) -> Vec<Option<NerveOutcome>> {
    let mut hung: Option<String> = None;
    plan.iter()
        .map(|p| {
            p.req.as_ref().map(|r| {
                if let Some(why) = &hung {
                    return crate::nerve::not_sent(
                        r,
                        &format!("not sent: the nerve did not answer an earlier request in this batch ({why})"),
                    );
                }
                let o = link.request_action(r);
                if o.by == "unknown" {
                    hung = Some(r.request_id.clone());
                }
                o
            })
        })
        .collect()
}

fn lock(cx: &Mutex<Cortex>) -> std::sync::MutexGuard<'_, Cortex> {
    cx.lock().unwrap_or_else(|e| e.into_inner())
}

/// Decider loop: block until an event arrives, then gather the burst for
/// `coalesce` and decide. No events = no wakeups.
///
/// The engine lock is held only for the in-memory phases (plan, commit);
/// the nerve calls run without it (slot 247, D-236-2), so API reads and
/// `POST /escalations/ack` never wait for a hung nerve. Alert/escalation
/// posts go to a [`Courier`] thread; action requests stay on this thread
/// (their answer decides the escalation and the cap) behind the per-batch
/// breaker of [`call_actions`].
/// Idle sweep period for `escalation_max_age_s` (slot 417): a quarter of
/// the age, 5 s .. 15 min. `None` (no max age) = no idle wake-ups at all.
pub fn escalation_sweep_every(max_age_s: u64) -> Option<Duration> {
    (max_age_s > 0).then(|| Duration::from_secs((max_age_s / 4).clamp(5, 900)))
}

/// Finish every due heal check: take them under the lock, read the nerve
/// `/state` for each WITHOUT the lock (`read`), judge under the lock again.
/// Returns the posts to send. No due check = no I/O.
pub fn run_heal_checks(
    cx: &Arc<Mutex<Cortex>>,
    read: &dyn Fn(&str) -> crate::verify::StateRead,
    now: u64,
) -> Vec<Post> {
    let due = lock(cx).heal_due(now);
    if due.is_empty() {
        return vec![];
    }
    let timeout = lock(cx).freeze.cfg.timeout_s;
    let reads: Vec<_> = due
        .into_iter()
        .map(|c| {
            let r = match &c.probe {
                Some((port, path)) => {
                    let t = crate::freeze::Target {
                        unit: c.unit.clone(),
                        port: *port,
                        path: path.clone(),
                    };
                    match crate::freeze::probe(&t, timeout) {
                        crate::freeze::Probe::Up(code) => crate::verify::StateRead::Probe {
                            up: true,
                            detail: format!("HTTP {code}"),
                        },
                        crate::freeze::Probe::Down(d) => crate::verify::StateRead::Probe {
                            up: false,
                            detail: d,
                        },
                    }
                }
                None => read(&c.unit),
            };
            (c, r)
        })
        .collect();
    let mut g = lock(cx);
    for (c, r) in reads {
        g.heal_judged(c, &r, now);
    }
    g.publish();
    g.take_posts()
}

/// The decider's `/state` reader for heal checks (loopback nerve, 5 s).
pub fn state_reader(nerve_url: Option<&str>) -> impl Fn(&str) -> crate::verify::StateRead {
    let addr = nerve_url.and_then(crate::config::nerve_cortex_addr);
    move |unit: &str| match addr {
        Some(a) => crate::nerve::read_unit_state(a, Duration::from_secs(5), unit),
        None => crate::verify::StateRead::Unreachable("no nerve configured".into()),
    }
}

/// The freeze watcher thread (only when `freeze.enabled`): probe every
/// watched target each `probe_s` (outside the engine lock), feed the
/// streaks, and hand fired freezes to the decider (`wake` carries an empty
/// burst; the events themselves go through [`Cortex::freeze`]'s inbox, so
/// nothing from outside can impersonate them). Targets come from the nerve
/// `/state` (unit + health port) filtered by the restart allowlist.
pub fn run_freeze_watch(
    cx: Arc<Mutex<Cortex>>,
    wake: std::sync::mpsc::Sender<Vec<Event>>,
    nerve: std::net::SocketAddr,
) {
    let (probe_s, timeout) = {
        let g = lock(&cx);
        (g.freeze.cfg.probe_s, g.freeze.cfg.timeout_s)
    };
    let io = Duration::from_secs(5);
    let mut targets_at: Option<u64> = None;
    loop {
        let now = now_secs();
        if targets_at.is_none_or(|t| now.saturating_sub(t) >= crate::freeze::TARGETS_EVERY_S) {
            if let Ok(st) = crate::nerve::nerve_state(nerve, io) {
                let mut g = lock(&cx);
                let t = {
                    let (r, skip) = (&g.rules, &g.freeze.cfg.skip);
                    crate::freeze::targets_from_state(&st, &|u| {
                        r.restart_allowed(u) && !skip.iter().any(|s| s == u)
                    })
                };
                g.freeze.set_targets(t);
                targets_at = Some(now);
            }
        }
        let targets = lock(&cx).freeze.targets.clone();
        let results: Vec<_> = targets
            .into_iter()
            .map(|t| {
                let p = crate::freeze::probe(&t, timeout);
                (t, p)
            })
            .collect();
        let now = now_secs();
        let fired = lock(&cx).freeze.round(&results, now);
        let mut evs = vec![];
        for f in fired {
            // systemd must still say running (a crash belongs to R3)
            let read = crate::nerve::read_unit_state(nerve, io, &f.target.unit);
            if !f.wide && !crate::freeze::says_active(&read) {
                lock(&cx).freeze.counts.held_not_active += 1;
                eprintln!(
                    "gaia-cortex {} freeze {} held: nerve does not say active ({read:?})",
                    ts(now),
                    f.target.unit
                );
                continue;
            }
            eprintln!(
                "gaia-cortex {} freeze {} fired: {} bad probes over {} s ({}){}",
                ts(now),
                f.target.unit,
                f.bad,
                f.span_s,
                f.detail,
                if f.wide { ", wide" } else { "" }
            );
            evs.push(f.event(now));
        }
        if !evs.is_empty() {
            lock(&cx).freeze.inbox.extend(evs);
            if wake.send(vec![]).is_err() {
                return;
            }
        } else if !results.is_empty() {
            lock(&cx).publish_heals();
        }
        std::thread::sleep(Duration::from_secs(probe_s));
    }
}

/// How long the decider may sleep: the escalation sweep and the next heal
/// deadline, whichever is first. `None` = until the next event.
pub fn decider_wait(sweep: Option<Duration>, next_heal: Option<u64>, now: u64) -> Option<Duration> {
    let heal = next_heal.map(|t| Duration::from_secs(t.saturating_sub(now).max(1)));
    match (sweep, heal) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

pub fn run_decider(rx: Receiver<Vec<Event>>, cx: Arc<Mutex<Cortex>>, coalesce: Duration) {
    let (mut link, snap, counters) = {
        let mut g = lock(&cx);
        let link = std::mem::replace(&mut g.nerve, Box::new(crate::nerve::Detached));
        (link, g.snap.clone(), g.posts.clone())
    };
    let phone = lock(&cx).phone.clone();
    let courier = link
        .fork()
        .map(|l| Courier::spawn_with(l, counters.clone(), COURIER_CAP, phone.clone()));
    let sweep = escalation_sweep_every(lock(&cx).cfg.escalation_max_age_s);
    let reader = state_reader(lock(&cx).cfg.nerve_url.as_deref());
    let send = |link: &mut Box<dyn NerveLink>, posts: Vec<Post>| {
        for p in posts {
            match &courier {
                Some(c) => c.send(p),
                None => {
                    if let Some(ph) = &phone {
                        let _ = ph.offer(&p.event, now_secs());
                    }
                    let ok = link.post_event(&p.event);
                    counters.note(p.alert, ok);
                }
            }
        }
    };
    loop {
        let wait = decider_wait(sweep, lock(&cx).heals.next_due(), now_secs());
        let first = match wait {
            None => match rx.recv() {
                Ok(v) => v,
                Err(_) => break,
            },
            Some(every) => match rx.recv_timeout(every) {
                Ok(v) => v,
                Err(RecvTimeoutError::Timeout) => {
                    // idle: no burst, still age out escalations and finish due heal checks
                    let now = now_secs();
                    {
                        let mut g = lock(&cx);
                        if g.expire_escalations(now) > 0 {
                            g.publish();
                        }
                    }
                    let posts = run_heal_checks(&cx, &reader, now);
                    send(&mut link, posts);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
        };
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
        let now = now_secs();
        snap.set_busy(true);
        let plan = lock(&cx).plan(burst, now);
        let outcomes = call_actions(&mut *link, &plan);
        let posts = {
            let mut g = lock(&cx);
            g.commit(plan, outcomes, now);
            snap.set_busy(false);
            g.publish();
            g.take_posts()
        };
        send(&mut link, posts);
        let posts = run_heal_checks(&cx, &reader, now_secs());
        send(&mut link, posts);
    }
}
