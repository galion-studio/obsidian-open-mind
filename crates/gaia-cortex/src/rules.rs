// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Deterministic rule engine. First matching rule wins; no model, no I/O.
//!
//! | rule | when | outcome |
//! |---|---|---|
//! | `R0-stack-wide` | the correlator folded 3+ hard-failing subjects into `stack-wide` | alert + escalate, never act (host/network problem) |
//! | `R1-protected` | unit/host/path is protected ([`crate::guard`]) | alert only, never act |
//! | `R2-circuit-open` | unit failed `flap_failures` times in `flap_window_s` (an event counts its nerve `count`; repeats the nerve folds are re-read by [`crate::repeat`]), or the nerve reported its breaker open / cap hit | open circuit, alert + escalate once; no action while open |
//! | `R2-start-limit` | systemd hit the start limit (`unit.start_limit`) | open circuit, alert + escalate |
//! | `R3-known-crash` | crash / OOM / `failed` state of a restart-allowed unit | restart via the nerve (dry-run by default) |
//! | `R3-cap` | ... but the cortex hourly cap for the unit is used up (default 2 = the nerve executor cap, applied in dry-run too) | open circuit, alert + escalate |
//! | `R3-cap-total` | ... but restarts asked for all units in the last hour reached `restarts_total_hour` (default 6 = the nerve cap) | alert + escalate, no circuit |
//! | `R3b-systemd-restarting` | failure with `unit.restart_scheduled` (systemd restarts it itself) | alert, no extra action (failure still counts toward flapping) |
//! | `R4-unknown-unit` | failure of a unit no rule may restart | alert + escalate once per circuit |
//! | `R4-trigger-failed` | only a `.path`/`.timer`/`.socket` folded into its service failed | alert + escalate once per circuit, never restart |
//! | `R4b-sandbox-oom` | `oom.kill` on a `sbxtest*` unit / `sbxtest.slice` (sandbox memory cap) | info only, never escalate to Hermes |
//! | `R5b-planned-stop` | failure or exit of a unit with a stop/restart job marker (`job_type` stop/restart, nerve `planned`), or a stop-signal exit (143/130, TERM/INT) that is active again / within 10 s of a job marker; no crash evidence (OOM, core dump, other exit, systemd restart) | nothing, never counts toward flapping (slot 309, D-271-2 A) |
//! | `R5-recovered` | unit back to `active` / info state change | nothing (incident quiets) |
//! | `R6-task-failed` | task/slot finished with `ok=false` | alert |
//! | `R7-threshold` | threshold crossed: warn | alert; error+ | alert + escalate (LLM-eligible) |
//! | `R8-unclassified` | unclassified process: warn | alert; error+ | escalate (LLM-eligible) |
//! | `R9-process-exit` | process exit: non-zero, warn+ | alert |
//! | `R10-remediation` | nerve `action.result` / `circuit.open` | rate-limited/circuit-open opens the cortex circuit + escalates once; echoes of our own requests are info |
//! | `R11-nerve-incident` | nerve engine `incident.opened` (warn/error alert, critical escalate) / `incident.resolved` (info) | no action |
//! | `R12-heal-failed` | heal verify ([`crate::verify`], not a rule here): a restart the nerve executed did not heal (nerve `/state` shows it not active at the deadline; for a freeze heal the health probe still fails) | open circuit, ALERT only (no escalation, Mate 2026-10-30); no further restart while open |
//! | `R12-heal-unverified` | heal verify: the deadline read of nerve `/state` failed or the nerve cannot see the unit | alert only |
//! | `R13-frozen` | internal `unit.frozen` ([`crate::freeze`]): an allowlisted unit systemd shows `active` failed its health probe N times in a row over >= 90 s, outside any lifecycle cooldown | restart via the nerve (same caps, verify by probe) + alert |
//! | `R13-freeze-wide` | ... but 2+ watched services are failing at once (dependency / host problem) | alert only |
//! | `R13-freeze-cap` | ... but the unit already had its freeze restart(s) this hour (`freeze.per_unit_hour`, default 1) | open circuit, alert only |
//! | `R2-circuit-open` (freeze) | ... but the unit's circuit is open (e.g. its last heal failed) | alert only, no restart |
//! | `R99-default` | anything else: error+ | escalate (LLM-eligible); warn | alert; info | nothing |

use crate::guard::Guard;
use nerve_cortex_contract::{Event, EventKind};
use openmind_nerve::incident::Sev;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

/// What the cortex wants done for one subject.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Ask the nerve to restart this unit.
    Restart(String),
    /// No remediation.
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub rule: &'static str,
    pub outcome: Outcome,
    pub reason: String,
    pub sev: Sev,
    pub alert: bool,
    pub escalate: bool,
    /// Ambiguous high-severity: the (optional) LLM advisor may be asked.
    pub llm_candidate: bool,
}

impl Verdict {
    pub fn new(rule: &'static str, sev: Sev, reason: impl Into<String>) -> Self {
        Verdict {
            rule,
            outcome: Outcome::None,
            reason: reason.into(),
            sev,
            alert: false,
            escalate: false,
            llm_candidate: false,
        }
    }
    fn alert(mut self) -> Self {
        self.alert = true;
        self
    }
    fn escalate(mut self) -> Self {
        self.alert = true;
        self.escalate = true;
        self
    }
    fn llm(mut self) -> Self {
        self.llm_candidate = true;
        self
    }
}

/// One subject (unit, metric, process, task) and its coalesced events.
#[derive(Debug, Clone)]
pub struct Subject {
    pub key: String,
    pub unit: Option<String>,
    pub host: Option<String>,
    pub events: Vec<Event>,
}

impl Subject {
    pub fn sev(&self) -> Sev {
        self.events.iter().map(sev_of).max().unwrap_or(Sev::Info)
    }

    /// Distinct failures in this burst: crash + `failed` state + OOM of the
    /// same failure count once, two crash events count twice. Each event
    /// counts its [`Event::occurrences`]: the duplicates the nerve folded
    /// into it (`count`), or the new ones a repeat event reports.
    pub fn failures(&self) -> u32 {
        self.failures_of(false)
    }

    /// Failures of trigger units (`.path`/`.timer`/`.socket`) folded into
    /// this service subject ([`trigger_target`]). They count toward the
    /// incident and its severity, never as a crash of the service: a
    /// trigger's failure can never cause a restart.
    pub fn trigger_failures(&self) -> u32 {
        self.failures_of(true)
    }

    fn failures_of(&self, via_trigger: bool) -> u32 {
        let c = |f: &dyn Fn(&Event) -> bool| {
            self.events
                .iter()
                .filter(|e| is_via_trigger(e) == via_trigger)
                .filter(|e| f(e))
                .map(Event::occurrences)
                .fold(0u32, u32::saturating_add)
        };
        let crash = c(&|e| e.kind() == EventKind::UnitCrash);
        let failed = c(&|e| e.kind() == EventKind::UnitState && e.to_state() == Some("failed"));
        let oom = c(&|e| e.kind() == EventKind::Oom && e.unit().is_some());
        let gave_up = c(&|e| e.kind() == EventKind::StartLimit);
        let sched = c(&|e| e.kind() == EventKind::RestartScheduled);
        crash.max(failed).max(oom).max(gave_up).max(sched)
    }

    pub fn recovered(&self) -> bool {
        self.failures() == 0
            && self.events.iter().filter(|e| !is_via_trigger(e)).any(|e| {
                e.kind() == EventKind::UnitState
                    && matches!(e.to_state(), Some("active" | "running"))
            })
    }

    /// The most telling event (worst severity, failures first).
    pub fn lead(&self) -> Option<&Event> {
        self.events.iter().max_by_key(|e| {
            let fail = matches!(e.kind(), EventKind::UnitCrash | EventKind::Oom) as u8;
            (sev_of(e), fail)
        })
    }

    pub fn texts_protected(&self, g: &Guard) -> Option<String> {
        self.events
            .iter()
            .find_map(|e| g.text(&e.data.to_string()))
            .map(|p| p.0)
    }
}

/// Per-unit memory: failures, restarts asked, circuit.
#[derive(Debug, Clone, Default)]
pub struct UnitBook {
    pub failures: VecDeque<u64>,
    pub restarts: VecDeque<u64>,
    pub circuit_until: Option<u64>,
    pub circuit_escalated: bool,
    /// Last time a stop/restart job marker was seen for the unit (slot 309).
    pub planned_at: Option<u64>,
    /// Freeze restarts decided ([`crate::freeze`]), last hour.
    pub freezes: VecDeque<u64>,
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub restarts_per_unit_hour: u32,
    /// All units together, per hour (the nerve executor's total cap).
    pub restarts_total_hour: u32,
    pub flap_failures: u32,
    pub flap_window_s: u64,
    pub circuit_open_s: u64,
}

pub struct Rules {
    pub guard: Guard,
    pub allow: Vec<String>,
    pub limits: Limits,
    pub books: BTreeMap<String, UnitBook>,
    /// Freeze restarts per unit per hour (`freeze.per_unit_hour`).
    pub freeze_per_unit_hour: u32,
}

impl Rules {
    pub fn new(guard: Guard, allow_cfg: &[String], limits: Limits) -> Self {
        let base: Vec<String> = if allow_cfg.is_empty() {
            openmind_nerve::reflex::ALLOW
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            allow_cfg.to_vec()
        };
        // Protected wins over the allowlist (cloudflared is on the nerve's
        // reflex ALLOW but Mate's rule says never stop it).
        let allow = base
            .into_iter()
            .filter(|u| guard.unit(u).is_none())
            .collect();
        Rules {
            guard,
            allow,
            limits,
            books: BTreeMap::new(),
            freeze_per_unit_hour: 1,
        }
    }

    /// `R13-*`: a running unit stopped answering its health probe. Every
    /// way out except the restart is alert only.
    fn decide_frozen(&mut self, u: &str, fz: &Event, now: u64) -> Verdict {
        let d = |k: &str| fz.data.get(k).cloned().unwrap_or(Value::Null);
        let what = format!(
            "{u} running but not answering: {} failed health probes over {} s ({})",
            d("bad_probes"),
            d("span_s"),
            d("detail").as_str().unwrap_or("?")
        );
        if d("wide").as_bool() == Some(true) {
            return Verdict::new(
                "R13-freeze-wide",
                Sev::Error,
                format!("{what}; 2+ services unresponsive at once (dependency or host problem): alert only, no restart"),
            )
            .alert();
        }
        if !self.restart_allowed(u) {
            return Verdict::new(
                "R13-frozen",
                Sev::Error,
                format!("{what}; not on the restart allowlist: alert only"),
            )
            .alert();
        }
        if self.circuit_open(u, now) {
            return Verdict::new(
                "R2-circuit-open",
                Sev::Error,
                format!(
                    "{what}; its circuit is open (an earlier restart did not heal it): no restart"
                ),
            )
            .alert();
        }
        let per = self.freeze_per_unit_hour;
        let b = self.books.entry(u.to_string()).or_default();
        while b
            .freezes
            .front()
            .is_some_and(|t| now.saturating_sub(*t) > 3600)
        {
            b.freezes.pop_front();
        }
        while b
            .restarts
            .front()
            .is_some_and(|t| now.saturating_sub(*t) > 3600)
        {
            b.restarts.pop_front();
        }
        let (nfz, nr) = (b.freezes.len() as u32, b.restarts.len() as u32);
        if nfz >= per {
            let _ = self.open_circuit(u, now);
            return Verdict::new(
                "R13-freeze-cap",
                Sev::Error,
                format!("{what}; {nfz} freeze restart(s) in the last hour (cap {per}): circuit open, alert only"),
            )
            .alert();
        }
        if nr >= self.limits.restarts_per_unit_hour {
            let _ = self.open_circuit(u, now);
            return Verdict::new(
                "R3-cap",
                Sev::Error,
                format!(
                    "{what}; {nr} restarts asked in the last hour (cap {}): circuit open, alert only",
                    self.limits.restarts_per_unit_hour
                ),
            )
            .alert();
        }
        let total = self.restarts_last_hour(now);
        if total >= self.limits.restarts_total_hour {
            return Verdict::new(
                "R3-cap-total",
                Sev::Error,
                format!(
                    "{what}; {total} restarts asked in the last hour across all units (cap {}): alert only",
                    self.limits.restarts_total_hour
                ),
            )
            .alert();
        }
        self.books
            .entry(u.to_string())
            .or_default()
            .freezes
            .push_back(now);
        Verdict {
            outcome: Outcome::Restart(u.to_string()),
            ..Verdict::new(
                "R13-frozen",
                Sev::Error,
                format!("{what}; restart via the nerve allowlist, verified by probe"),
            )
        }
        .alert()
    }

    pub fn restart_allowed(&self, unit: &str) -> bool {
        self.guard.unit(unit).is_none() && self.allow.iter().any(|a| a == unit)
    }

    /// Both restart caps (per unit, all units) still have room this hour.
    pub fn restart_cap_free(&self, unit: &str, now: u64) -> bool {
        let unit_n = self.books.get(unit).map_or(0, |b| {
            b.restarts
                .iter()
                .filter(|t| now.saturating_sub(**t) <= 3600)
                .count() as u32
        });
        unit_n < self.limits.restarts_per_unit_hour
            && self.restarts_last_hour(now) < self.limits.restarts_total_hour
    }

    pub fn circuit_open(&self, unit: &str, now: u64) -> bool {
        self.books
            .get(unit)
            .and_then(|b| b.circuit_until)
            .is_some_and(|t| t > now)
    }

    /// Open `unit`'s circuit from outside the rules (a restart that did not
    /// heal it, [`crate::verify`]). Returns true when it was not open yet.
    pub fn hold_circuit(&mut self, unit: &str, now: u64) -> bool {
        self.open_circuit(unit, now)
    }

    fn open_circuit(&mut self, unit: &str, now: u64) -> bool {
        let b = self.books.entry(unit.to_string()).or_default();
        let first = !b.circuit_until.is_some_and(|t| t > now);
        if first {
            b.circuit_escalated = false;
        }
        b.circuit_until = Some(now + self.limits.circuit_open_s);
        let esc = !b.circuit_escalated;
        b.circuit_escalated = true;
        esc
    }

    /// Restarts asked (simulated, executed or unknown) in the last hour,
    /// all units together.
    pub fn restarts_last_hour(&self, now: u64) -> u32 {
        self.books
            .values()
            .map(|b| {
                b.restarts
                    .iter()
                    .filter(|t| now.saturating_sub(**t) <= 3600)
                    .count() as u32
            })
            .sum()
    }

    /// Record that a restart was asked (counts toward the hourly cap).
    pub fn note_restart(&mut self, unit: &str, now: u64) {
        self.books
            .entry(unit.to_string())
            .or_default()
            .restarts
            .push_back(now);
    }

    /// Release a cap slot reserved by [`Rules::note_restart`] at `now` (the
    /// nerve said the restart did not run).
    pub fn unnote_restart(&mut self, unit: &str, now: u64) {
        if let Some(b) = self.books.get_mut(unit) {
            if let Some(i) = b.restarts.iter().rposition(|t| *t == now) {
                b.restarts.remove(i);
            }
        }
    }

    /// Decide for one subject. `stack_wide` = the correlator folded it.
    pub fn decide(&mut self, s: &Subject, stack_wide: bool, now: u64) -> Verdict {
        let sev = s.sev();
        let lead_kind = s.lead().map(Event::kind).unwrap_or(EventKind::Other);
        if stack_wide {
            return Verdict::new(
                "R0-stack-wide",
                sev.max(Sev::Error),
                "3+ subjects failing at once: host or network problem, not N restarts",
            )
            .escalate();
        }
        // Protected: never act, alert only.
        let prot = s
            .unit
            .as_deref()
            .and_then(|u| self.guard.unit(u))
            .map(|p| p.0)
            .or_else(|| {
                s.host
                    .as_deref()
                    .and_then(|h| self.guard.host(h))
                    .map(|p| p.0)
            })
            .or_else(|| s.texts_protected(&self.guard));
        if let Some(p) = prot {
            let v = Verdict::new(
                "R1-protected",
                sev.max(Sev::Warn),
                format!("{p}; alert only, never act"),
            );
            return if sev > Sev::Info || s.failures() > 0 {
                v.alert()
            } else {
                v
            };
        }
        // Sandbox test OOM (sbxtest*): cgroup MemoryMax kills are expected
        // during swarm sandbox runs. Log as info; never alert or escalate
        // to Hermes (Mate approved hoff-1792623978-2, 2026-10-27).
        if let Some(u) = s.unit.as_deref() {
            if is_sbxtest_unit(u) && s.events.iter().any(|e| e.kind() == EventKind::Oom) {
                return Verdict::new(
                    "R4b-sandbox-oom",
                    Sev::Info,
                    format!(
                        "{u}: sandbox OOM kill (sbxtest* memory cap); info only, no Hermes escalate"
                    ),
                );
            }
        }
        // Freeze heal (internal `unit.frozen`, crate::freeze).
        if let (Some(u), Some(fz)) = (
            s.unit.clone(),
            s.events.iter().find(|e| crate::freeze::is_frozen(e)),
        ) {
            return self.decide_frozen(&u, fz, now);
        }
        // Nerve-side remediation outcomes.
        if lead_kind == EventKind::Remediation || lead_kind == EventKind::CircuitOpen {
            if let Some(u) = s.unit.clone() {
                let lead = s.lead();
                let status = lead
                    .and_then(|e| {
                        e.data
                            .get("result")
                            .or_else(|| e.data.get("status"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                // Echo of our own request: already handled synchronously.
                if lead
                    .and_then(|e| e.data.get("source"))
                    .and_then(Value::as_str)
                    == Some(nerve_cortex_contract::CORTEX_SOURCE)
                {
                    return Verdict::new(
                        "R10-remediation",
                        Sev::Info,
                        format!("nerve {status} for our request on {u}"),
                    );
                }
                if lead_kind == EventKind::CircuitOpen
                    || status == "circuit-open"
                    || status == "rate-limited"
                {
                    let esc = self.open_circuit(&u, now);
                    let v = Verdict::new(
                        "R10-remediation",
                        sev.max(Sev::Warn),
                        format!(
                            "nerve reports {} for {u}; cortex circuit open",
                            if status.is_empty() {
                                "circuit open"
                            } else {
                                &status
                            }
                        ),
                    );
                    return if esc { v.escalate() } else { v.alert() };
                }
                return Verdict::new(
                    "R10-remediation",
                    Sev::Info,
                    format!("nerve remediation {status} for {u}"),
                );
            }
        }
        if let Some(u) = s.unit.clone() {
            // Planned stop (slot 309, slot 271 F3, D-271-2 option A = ignore):
            // an operator, cutover or job restart is not a crash. Checked
            // before the flap book so it never counts toward R2 either.
            if s.stop_evidence().marker.is_some() {
                self.books.entry(u.clone()).or_default().planned_at = Some(now);
            }
            if let Some(why) = self.planned_stop(s, now) {
                return Verdict::new("R5b-planned-stop", Sev::Info, why);
            }
            let fails = s.failures();
            if fails == 0 && s.trigger_failures() > 0 {
                // Only the trigger (.path/.timer/.socket) failed: one subject
                // with its service (Friday 2026-10-30: the self-modify gate
                // .path and .service were two incidents and two paid
                // hand-offs). Never a restart; escalate once per circuit.
                let trig = s
                    .events
                    .iter()
                    .find_map(|e| e.data.get(VIA_TRIGGER).and_then(Value::as_str))
                    .unwrap_or("trigger")
                    .to_string();
                let esc = self.open_circuit(&u, now);
                let v = Verdict::new(
                    "R4-trigger-failed",
                    sev.max(Sev::Warn),
                    format!("{trig} (trigger of {u}) failed; folded into {u}, never restarted"),
                );
                return if esc { v.escalate() } else { v.alert() };
            }
            if fails > 0 {
                let w = self.limits.flap_window_s;
                let b = self.books.entry(u.clone()).or_default();
                // A long crash loop folded by the nerve can report thousands
                // of repeats; the window only needs to reach the flap limit.
                for _ in 0..fails.min(1024) {
                    b.failures.push_back(now);
                }
                while b
                    .failures
                    .front()
                    .is_some_and(|t| now.saturating_sub(*t) > w)
                {
                    b.failures.pop_front();
                }
                while b
                    .restarts
                    .front()
                    .is_some_and(|t| now.saturating_sub(*t) > 3600)
                {
                    b.restarts.pop_front();
                }
                let (nf, nr) = (b.failures.len() as u32, b.restarts.len() as u32);
                if self.circuit_open(&u, now) {
                    let esc = self.open_circuit(&u, now);
                    let v = Verdict::new("R2-circuit-open", sev.max(Sev::Error), format!("{u} failed again while its circuit is open ({nf} failures in {w}s); no action"));
                    return if esc { v.escalate() } else { v.alert() };
                }
                if nf >= self.limits.flap_failures {
                    let esc = self.open_circuit(&u, now);
                    let v = Verdict::new(
                        "R2-circuit-open",
                        sev.max(Sev::Error),
                        format!("{u} flapping: {nf} failures in {w}s >= {}; circuit open {}s, human needed", self.limits.flap_failures, self.limits.circuit_open_s),
                    );
                    return if esc { v.escalate() } else { v.alert() };
                }
                let has = |k: EventKind| s.events.iter().any(|e| e.kind() == k);
                if has(EventKind::StartLimit) {
                    let esc = self.open_circuit(&u, now);
                    let v = Verdict::new("R2-start-limit", sev.max(Sev::Error), format!("{u}: systemd start limit hit (gave up restarting); circuit open, human needed"));
                    return if esc { v.escalate() } else { v.alert() };
                }
                if has(EventKind::RestartScheduled) {
                    return Verdict::new("R3b-systemd-restarting", sev.max(Sev::Warn), format!("{u} failed; systemd already scheduled its own restart (failure {nf} in {w}s), no extra action")).alert();
                }
                if !self.restart_allowed(&u) {
                    // One escalation per circuit (was: every failure escalated;
                    // 32 galion-ntfy-relay@ hand-offs on 2026-10-30/31).
                    let esc = self.open_circuit(&u, now);
                    let v = Verdict::new(
                        "R4-unknown-unit",
                        sev.max(Sev::Warn),
                        format!("{u} failed; not on the restart allowlist, no rule can act"),
                    );
                    return if esc { v.escalate() } else { v.alert() };
                }
                if nr >= self.limits.restarts_per_unit_hour {
                    let esc = self.open_circuit(&u, now);
                    let v = Verdict::new(
                        "R3-cap",
                        sev.max(Sev::Error),
                        format!(
                            "{u}: {nr} restarts asked in the last hour (cap {}); circuit open",
                            self.limits.restarts_per_unit_hour
                        ),
                    );
                    return if esc { v.escalate() } else { v.alert() };
                }
                let total = self.restarts_last_hour(now);
                if total >= self.limits.restarts_total_hour {
                    // Not a property of this unit: no circuit, the decision
                    // cooldown keeps it to one alert per subject.
                    return Verdict::new(
                        "R3-cap-total",
                        sev.max(Sev::Error),
                        format!(
                            "{u} failed; {total} restarts asked in the last hour across all units (cap {}); no action, human needed",
                            self.limits.restarts_total_hour
                        ),
                    )
                    .escalate();
                }
                let what = match lead_kind {
                    EventKind::Oom => "OOM kill",
                    EventKind::UnitState => "entered failed state",
                    _ => "crashed",
                };
                return Verdict {
                    outcome: Outcome::Restart(u.clone()),
                    ..Verdict::new(
                        "R3-known-crash",
                        sev.max(Sev::Warn),
                        format!("{u} {what} (failure {nf} in {w}s); restart via nerve allowlist"),
                    )
                }
                .alert();
            }
            if s.recovered() {
                return Verdict::new("R5-recovered", Sev::Info, format!("{u} active again"));
            }
        }
        match lead_kind {
            EventKind::TaskFinished | EventKind::SlotFinished => {
                let failed = s.events.iter().any(|e| e.ok() == Some(false));
                if failed {
                    Verdict::new(
                        "R6-task-failed",
                        sev.max(Sev::Warn),
                        format!("{} finished with ok=false", s.key),
                    )
                    .alert()
                } else {
                    Verdict::new(
                        "R6-task-failed",
                        Sev::Info,
                        format!("{} finished ok", s.key),
                    )
                }
            }
            EventKind::NerveIncident => {
                let resolved = s
                    .events
                    .last()
                    .is_some_and(|e| e.kind.ends_with("resolved"));
                let backfill = s
                    .events
                    .iter()
                    .any(|e| e.data.get("backfill") == Some(&Value::Bool(true)));
                let v = Verdict::new(
                    "R11-nerve-incident",
                    sev,
                    format!(
                        "nerve engine incident {} {}{}",
                        s.key,
                        if resolved { "resolved" } else { "open" },
                        match (backfill, resolved) {
                            (false, _) => "",
                            (true, false) => " (backfill: opened before the cortex was listening)",
                            (true, true) => " (reconciled: no longer open on the nerve)",
                        }
                    ),
                );
                if resolved || sev == Sev::Info {
                    Verdict {
                        sev: Sev::Info,
                        ..v
                    }
                } else if sev >= Sev::Critical {
                    v.escalate()
                } else {
                    v.alert()
                }
            }
            EventKind::Threshold => {
                let v = Verdict::new(
                    "R7-threshold",
                    sev,
                    format!("{} crossed its threshold", s.key),
                );
                match sev {
                    Sev::Info => v,
                    Sev::Warn => v.alert(),
                    _ => v.escalate().llm(),
                }
            }
            EventKind::UnclassifiedProcess => {
                let v = Verdict::new(
                    "R8-unclassified",
                    sev,
                    format!("unclassified process {}", s.key),
                );
                match sev {
                    Sev::Info => v,
                    Sev::Warn => v.alert(),
                    _ => v.escalate().llm(),
                }
            }
            EventKind::ProcessExit => {
                let nonzero = s.events.iter().any(abnormal_exit);
                if nonzero && sev >= Sev::Warn {
                    Verdict::new(
                        "R9-process-exit",
                        sev,
                        format!("{} exited abnormally", s.key),
                    )
                    .alert()
                } else {
                    Verdict::new("R9-process-exit", Sev::Info, format!("{} exited", s.key))
                }
            }
            EventKind::UnitState => {
                Verdict::new("R5-recovered", Sev::Info, format!("{} state change", s.key))
            }
            _ => {
                let v = Verdict::new(
                    "R99-default",
                    sev,
                    format!(
                        "{} {}",
                        s.key,
                        s.lead().map(|e| e.kind.as_str()).unwrap_or("event")
                    ),
                );
                match sev {
                    Sev::Info => v,
                    Sev::Warn => v.alert(),
                    _ => v.escalate().llm(),
                }
            }
        }
    }

    pub fn books_json(&self, now: u64) -> Value {
        Value::Object(
            self.books
                .iter()
                .map(|(u, b)| {
                    (
                        u.clone(),
                        json!({"failures_window": b.failures.len(), "restarts_hour": b.restarts.len(),
                               "circuit_open": b.circuit_until.is_some_and(|t| t > now),
                               "circuit_until": b.circuit_until.filter(|t| *t > now).map(|t| zagreb_time::from_unix(t as i64))}),
                    )
                })
                .collect(),
        )
    }
}

/// Event data key naming the trigger unit an event was folded from.
pub const VIA_TRIGGER: &str = "via_trigger";

/// The service a trigger unit starts: `X.path`, `X.timer`, `X.socket` ->
/// `X.service` (systemd's default `Unit=`; templates keep their `@`).
/// `None` for anything else.
pub fn trigger_target(unit: &str) -> Option<String> {
    [".path", ".timer", ".socket"].iter().find_map(|suf| {
        let stem = unit.strip_suffix(suf)?;
        (!stem.is_empty() && !stem.ends_with('.')).then(|| format!("{stem}.service"))
    })
}

/// Was this event folded into its service subject from a trigger unit?
pub fn is_via_trigger(e: &Event) -> bool {
    e.data.get(VIA_TRIGGER).is_some_and(Value::is_string)
}

/// Tag a trigger unit's event with [`VIA_TRIGGER`] (the engine does this
/// before grouping, so [`Subject::failures`] can tell them apart).
pub fn tag_trigger(e: &mut Event) {
    let Some(u) = e.unit() else { return };
    if trigger_target(&u).is_none() {
        return;
    }
    if !e.data.is_object() {
        e.data = Value::Object(Default::default());
    }
    if let Some(o) = e.data.as_object_mut() {
        o.insert(VIA_TRIGGER.into(), Value::String(u));
    }
}

/// Subject key and unit for an event. Trigger units fold into the service
/// they start (one problem counts as one subject).
pub fn subject_key(e: &Event) -> (String, Option<String>) {
    if let Some(u) = e.unit() {
        let u = trigger_target(&u).unwrap_or(u);
        return (format!("unit:{u}"), Some(u));
    }
    let k = e.kind();
    let tail = match k {
        EventKind::Threshold => e.metric().map(|m| format!("threshold:{m}")),
        EventKind::NerveIncident => e
            .data
            .get("cause")
            .and_then(Value::as_str)
            .map(|c| format!("nerve-incident:{c}")),
        EventKind::UnclassifiedProcess | EventKind::ProcessExit | EventKind::Oom => {
            e.comm().map(|c| format!("proc:{c}"))
        }
        EventKind::TaskFinished | EventKind::SlotFinished => e.data.get("id").map(|v| {
            format!(
                "{}:{}",
                k.as_str().split('.').next().unwrap_or("task"),
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            )
        }),
        _ => None,
    };
    let key = tail
        .or_else(|| e.host().map(|h| format!("host:{h}")))
        .unwrap_or_else(|| format!("{}:{}", e.source, k.as_str()));
    let key: String = key.chars().filter(|c| !c.is_control()).take(160).collect();
    (key, None)
}

/// Event severity on the correlator scale (nerve low = info, medium = warn,
/// high = error).
pub fn sev_of(e: &Event) -> Sev {
    match e.rank() {
        0 => Sev::Info,
        1 => Sev::Warn,
        2 => Sev::Error,
        _ => Sev::Critical,
    }
}

/// Non-zero exit: numeric `exit_code`/`exit_status` != 0, a signal, or the
/// journald `EXIT_CODE` words `killed` / `dumped`.
pub fn abnormal_exit(e: &Event) -> bool {
    let num = |k: &str| {
        e.data.get(k).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
    };
    num("exit_code").is_some_and(|c| c != 0)
        || num("exit_status").is_some_and(|c| c != 0)
        || e.data.get("signal").is_some_and(|v| !v.is_null())
        || matches!(
            e.data.get("exit_code").and_then(Value::as_str),
            Some("killed" | "dumped")
        )
}

/// Throwaway swarm sandbox units: `sbxtest.slice` and any `sbxtest-*` service.
pub fn is_sbxtest_unit(unit: &str) -> bool {
    unit == "sbxtest.slice" || unit.starts_with("sbxtest")
}

/// How long a seen stop/restart job of a unit makes a following
/// stop-signal exit (143/130) of that unit count as planned (slot 309,
/// slot 271 F3): covers a burst boundary between the job line and the exit.
pub const PLANNED_HOLD_S: u64 = 10;

/// What a subject's events say about a stop (slot 309, D-271-2 option A).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StopEvidence {
    /// Explicit stop/restart job marker: nerve `data.planned=true`, kind
    /// `unit.planned_stop` (slot 310 F2), or `data.job_type` stop/restart
    /// (journald `Stopping` / `Stopped` lines).
    pub marker: Option<&'static str>,
    /// Occurrences of marker events (nerve `count`, repeat rises).
    pub marker_occ: u32,
    /// Exit by the stop signal: status 143/130 (`code=exited`), or
    /// TERM/INT (`code=killed`).
    pub stop_exit: bool,
    /// Occurrences of stop-signal exits.
    pub stop_exit_occ: u32,
    /// The unit is active again in this burst (`unit.started`, to=active).
    pub active_again: bool,
    /// Hard crash evidence: OOM, start limit, systemd's own restart, a core
    /// dump, a non-stop exit status or signal, or a watchdog/oom/core/signal
    /// unit result. Crash evidence always wins over a planned marker.
    pub crash: bool,
}

fn data_text(e: &Event, k: &str) -> Option<String> {
    e.data.get(k).and_then(|v| match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// Exit status of a stop by SIGTERM/SIGINT. `None` = no status on the event.
fn stop_signal_exit(e: &Event) -> Option<bool> {
    let st = data_text(e, "exit_status")?;
    let code = data_text(e, "exit_code").unwrap_or_default();
    Some(match code.as_str() {
        "killed" => matches!(
            st.as_str(),
            "TERM" | "INT" | "SIGTERM" | "SIGINT" | "15" | "2"
        ),
        "dumped" => false,
        _ => matches!(st.as_str(), "143" | "130"),
    })
}

/// A unit process exit with a non-zero status (crash or stop-signal
/// evidence). Armed by the repeat watch next to the failures (slot 309).
pub fn exit_evidence(e: &Event) -> bool {
    e.kind() == EventKind::ProcessExit
        && e.unit().is_some()
        && data_text(e, "exit_status").is_some_and(|s| s != "0")
}

/// A stop/restart job marker on a unit event: nerve `data.planned=true`,
/// kind `unit.planned_stop` (slot 310 F2), or journald `JOB_TYPE`
/// stop/restart (`Stopping` / `Stopped` lines; the live nerve 9d1a4f3 puts
/// it in `data.job_type`). Also armed by the repeat watch so a planned
/// restart the nerve folds (dedupe 600 s) still carries its marker.
pub fn stop_marker(e: &Event) -> Option<&'static str> {
    e.unit()?;
    if e.data.get("planned") == Some(&Value::Bool(true)) {
        Some("nerve planned flag")
    } else if e.kind == "unit.planned_stop" {
        Some("nerve unit.planned_stop")
    } else if matches!(
        data_text(e, "job_type").as_deref(),
        Some("stop" | "restart")
    ) {
        Some("systemd stop/restart job")
    } else {
        None
    }
}

impl Subject {
    /// Stop/crash evidence of this burst (see [`StopEvidence`]).
    pub fn stop_evidence(&self) -> StopEvidence {
        let mut ev = StopEvidence::default();
        for e in &self.events {
            let k = e.kind();
            if matches!(
                k,
                EventKind::Oom | EventKind::StartLimit | EventKind::RestartScheduled
            ) {
                ev.crash = true;
            }
            if data_text(e, "exit_code").as_deref() == Some("dumped") {
                ev.crash = true;
            }
            if matches!(
                data_text(e, "unit_result").as_deref(),
                Some("core-dump" | "oom-kill" | "watchdog" | "signal" | "start-limit-hit")
            ) {
                ev.crash = true;
            }
            match stop_signal_exit(e) {
                Some(true) => {
                    ev.stop_exit = true;
                    ev.stop_exit_occ = ev.stop_exit_occ.saturating_add(e.occurrences());
                }
                Some(false) if data_text(e, "exit_status").as_deref() != Some("0") => {
                    ev.crash = true
                }
                _ => {}
            }
            if let Some(m) = stop_marker(e) {
                ev.marker.get_or_insert(m);
                ev.marker_occ = ev.marker_occ.saturating_add(e.occurrences());
            }
            if k == EventKind::UnitState && matches!(e.to_state(), Some("active" | "running")) {
                ev.active_again = true;
            }
        }
        ev
    }

    /// Something a planned stop could explain: a failure, a process exit,
    /// or a failure/exit the slot 310 nerve already tagged as
    /// `unit.planned_stop` (it no longer arrives as unit.failed/unit.exit;
    /// without this the pair never reaches R5b, slot 328 F-328-1).
    fn stop_shaped(&self) -> bool {
        self.failures() > 0
            || self
                .events
                .iter()
                .any(|e| e.kind() == EventKind::ProcessExit || e.kind == "unit.planned_stop")
    }
}

impl Rules {
    /// Planned stop (operator / cutover / job restart) of a unit subject:
    /// the reason, or `None`. Read-only (the engine asks it for the signal
    /// severity before [`Rules::decide`] runs). Slot 309 / slot 271 F3.
    pub fn planned_stop(&self, s: &Subject, now: u64) -> Option<String> {
        let u = s.unit.as_deref()?;
        if !s.stop_shaped() {
            return None;
        }
        let ev = s.stop_evidence();
        if ev.crash {
            return None;
        }
        // Every failure of the burst needs its own planned explanation: a
        // repeat burst that folds one planned restart and one real crash
        // (failed +2, stop job +1) is still a crash.
        let fails = s.failures();
        if let Some(m) = ev.marker {
            if ev.marker_occ >= fails {
                return Some(format!("{u}: {m}; planned stop, not a crash"));
            }
            return None;
        }
        if !ev.stop_exit || ev.stop_exit_occ < fails {
            return None;
        }
        let recent = self
            .books
            .get(u)
            .and_then(|b| b.planned_at)
            .is_some_and(|t| now.saturating_sub(t) <= PLANNED_HOLD_S);
        if recent {
            return Some(format!(
                "{u}: stop-signal exit within {PLANNED_HOLD_S}s of a stop/restart job; planned stop, not a crash"
            ));
        }
        if ev.active_again {
            return Some(format!(
                "{u}: stop-signal exit (143/130) and active again; planned restart, not a crash"
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn limits() -> Limits {
        Limits {
            restarts_per_unit_hour: 2,
            restarts_total_hour: 6,
            flap_failures: 3,
            flap_window_s: 600,
            circuit_open_s: 3600,
        }
    }

    fn rules() -> Rules {
        Rules::new(Guard::new(&[]), &[], limits())
    }

    fn oom(unit: &str, seq: u64) -> Event {
        let mut e = Event::new(
            "journal",
            "oom.kill",
            "low",
            json!({
                "seq": seq,
                "unit": unit,
                "message_id": "fe6faa94e7774663a0da52717891d8ef",
            }),
        );
        e.unit = Some(unit.to_string());
        e.seq = Some(seq);
        e.msg = Some(format!(
            "{unit}: A process of this unit has been killed by the OOM killer."
        ));
        e
    }

    fn subject_of(e: Event) -> Subject {
        let (key, unit) = subject_key(&e);
        Subject {
            key,
            unit,
            host: None,
            events: vec![e],
        }
    }

    /// Live VPS 2026-10-22 01:06 +02:00: seq 51 on sbxtest.slice must not
    /// escalate (was R4-unknown-unit -> hoff-1792623984-3).
    #[test]
    fn sbxtest_slice_oom_seq51_is_info_no_escalate() {
        let mut r = rules();
        let s = subject_of(oom("sbxtest.slice", 51));
        let v = r.decide(&s, false, 1_792_623_982);
        assert_eq!(v.rule, "R4b-sandbox-oom");
        assert_eq!(v.sev, Sev::Info);
        assert!(!v.alert, "log only");
        assert!(!v.escalate, "no Hermes handoff");
        assert_eq!(v.outcome, Outcome::None);
        assert!(
            !r.books.contains_key("sbxtest.slice"),
            "sandbox OOM must not open a flap book"
        );
    }

    #[test]
    fn sbxtest_service_oom_is_info_no_escalate() {
        let mut r = rules();
        for unit in [
            "sbxtest-main-20261022-010534.service",
            "sbxtest-main2-20261022-010534.service",
        ] {
            let s = subject_of(oom(unit, 48));
            let v = r.decide(&s, false, 1_792_623_976);
            assert_eq!(v.rule, "R4b-sandbox-oom", "{unit}");
            assert!(!v.escalate, "{unit}");
            assert_eq!(v.sev, Sev::Info, "{unit}");
        }
    }

    #[test]
    fn non_sandbox_oom_still_escalates_via_r4() {
        let mut r = rules();
        let s = subject_of(oom("mystery.service", 99));
        let v = r.decide(&s, false, 1_792_623_976);
        assert_eq!(v.rule, "R4-unknown-unit");
        assert!(v.escalate);
    }

    #[test]
    fn sbxtest_crash_without_oom_still_escalates() {
        let mut r = rules();
        let mut e = Event::new(
            "journal",
            "unit.failed",
            "error",
            json!({"seq": 7, "unit": "sbxtest-main-20261022-010534.service"}),
        );
        e.unit = Some("sbxtest-main-20261022-010534.service".into());
        let s = subject_of(e);
        let v = r.decide(&s, false, 1_792_623_976);
        assert_eq!(v.rule, "R4-unknown-unit");
        assert!(v.escalate);
    }

    #[test]
    fn is_sbxtest_unit_matches_prefix_and_slice() {
        assert!(is_sbxtest_unit("sbxtest.slice"));
        assert!(is_sbxtest_unit("sbxtest-main-20261022-010534.service"));
        assert!(!is_sbxtest_unit("galion-nerve.service"));
        assert!(!is_sbxtest_unit("test.slice"));
    }

    // ---- slot 309: planned stops (slot 271 F3, D-271-2 option A) ----

    const CONSOLE: &str = "console.service";

    /// A nerve event as the live nerve 9d1a4f3 (evt/journal.rs) emits it.
    fn jev(kind: &str, sev: &str, seq: u64, data: Value) -> Event {
        let mut e = Event::new("journal", kind, sev, data);
        e.unit = Some(CONSOLE.to_string());
        e.seq = Some(seq);
        e.key = Some(format!("{kind}:{CONSOLE}"));
        e
    }

    fn exit(seq: u64, code: &str, status: &str) -> Event {
        jev(
            "unit.exit",
            "high",
            seq,
            json!({"message_id": "98e322203f7a4ed290d09fe03c09fe15", "exit_code": code, "exit_status": status}),
        )
    }
    fn failed(seq: u64, result: &str) -> Event {
        jev(
            "unit.failed",
            "high",
            seq,
            json!({"message_id": "d9b373ed55a64feb8242e02dbe79a49c", "unit_result": result}),
        )
    }
    fn stopped(seq: u64, job: &str) -> Event {
        jev(
            "unit.stopped",
            "medium",
            seq,
            json!({"message_id": "9d1aaa27d60140bd96365438aad20286", "job_type": job, "job_result": "done"}),
        )
    }
    fn started(seq: u64) -> Event {
        jev(
            "unit.started",
            "medium",
            seq,
            json!({"message_id": "39f53479d3a045ac8e11786248231fbf", "job_type": "start", "job_result": "done"}),
        )
    }
    fn subj(events: Vec<Event>) -> Subject {
        Subject {
            key: format!("unit:{CONSOLE}"),
            unit: Some(CONSOLE.to_string()),
            host: None,
            events,
        }
    }

    /// Live VPS 2026-10-27 17:25:04 +01:00 (journal seq 21467446-453):
    /// operator `systemctl restart console.service`, srvx exits 143 on
    /// SIGTERM, systemd "Failed with result 'exit-code'", Stopped
    /// JOB_TYPE=restart, Started. Base 73583ab answered R3-known-crash ->
    /// restart (live dec-874). Now: R5b, no action, no flap count.
    #[test]
    fn live_1725_operator_restart_is_planned_not_r3() {
        let mut r = rules();
        let s = subj(vec![
            exit(21467449, "exited", "143"),
            failed(21467450, "exit-code"),
            stopped(21467451, "restart"),
            started(21467453),
        ]);
        assert!(
            r.restart_allowed(CONSOLE),
            "console is on the reflex allowlist"
        );
        let v = r.decide(&s, false, 1_793_118_305);
        assert_eq!(v.rule, "R5b-planned-stop", "{v:?}");
        assert_eq!(v.outcome, Outcome::None);
        assert_eq!(v.sev, Sev::Info);
        assert!(!v.alert && !v.escalate);
        let b = r.books.get(CONSOLE).expect("marker noted");
        assert!(b.failures.is_empty(), "planned stop never counts toward R2");
        assert!(b.restarts.is_empty());
        assert_eq!(b.planned_at, Some(1_793_118_305));
    }

    /// Same burst under the old rules shape (no job marker, no restart):
    /// a crash-exit-1 control still restarts.
    #[test]
    fn crash_exit_1_control_still_restarts() {
        let mut r = rules();
        let s = subj(vec![exit(10, "exited", "1"), failed(11, "exit-code")]);
        let v = r.decide(&s, false, 1_000);
        assert_eq!(v.rule, "R3-known-crash");
        assert_eq!(v.outcome, Outcome::Restart(CONSOLE.into()));
    }

    #[test]
    fn crash_evidence_beats_a_job_marker() {
        // kill -9 during a restart job, a core dump, an OOM, a non-stop exit
        // status or systemd's own restart: never planned.
        let cases: Vec<Vec<Event>> = vec![
            vec![
                exit(1, "killed", "KILL"),
                failed(2, "signal"),
                stopped(3, "restart"),
            ],
            vec![
                exit(1, "dumped", "SEGV"),
                failed(2, "core-dump"),
                stopped(3, "stop"),
            ],
            vec![
                exit(1, "exited", "1"),
                failed(2, "exit-code"),
                stopped(3, "restart"),
            ],
            vec![failed(2, "oom-kill"), stopped(3, "restart")],
            vec![
                exit(1, "exited", "143"),
                failed(2, "exit-code"),
                stopped(3, "restart"),
                jev("unit.restart_scheduled", "high", 4, json!({})),
            ],
        ];
        for (i, evs) in cases.into_iter().enumerate() {
            let mut r = rules();
            let s = subj(evs);
            assert!(r.planned_stop(&s, 1_000).is_none(), "case {i}");
            let v = r.decide(&s, false, 1_000);
            assert_ne!(v.rule, "R5b-planned-stop", "case {i}: {v:?}");
        }
    }

    #[test]
    fn kill_9_control_restarts() {
        let mut r = rules();
        let s = subj(vec![exit(1, "killed", "KILL"), failed(2, "signal")]);
        let v = r.decide(&s, false, 1_000);
        assert_eq!(v.rule, "R3-known-crash");
    }

    #[test]
    fn exit_143_alone_is_not_enough() {
        // No job marker, not active again: could be anything that sent
        // SIGTERM; keep the old behaviour (restart).
        let mut r = rules();
        let s = subj(vec![exit(1, "exited", "143"), failed(2, "exit-code")]);
        assert_eq!(r.decide(&s, false, 1_000).rule, "R3-known-crash");
    }

    #[test]
    fn exit_143_and_active_again_without_marker_is_planned() {
        let mut r = rules();
        for (code, st) in [("exited", "143"), ("exited", "130"), ("killed", "TERM")] {
            let s = subj(vec![exit(1, code, st), failed(2, "exit-code"), started(3)]);
            let v = r.decide(&s, false, 1_000);
            assert_eq!(v.rule, "R5b-planned-stop", "{code}/{st}");
        }
    }

    #[test]
    fn split_burst_within_hold_is_planned_after_hold_is_crash() {
        // F2 nerve (slot 310): the Stopping line arrives first, its own burst.
        let mut stopping = jev(
            "unit.planned_stop",
            "low",
            1,
            json!({"job_type": "stop", "planned": true}),
        );
        stopping.kind = "unit.planned_stop".into();
        let mut r = rules();
        let v0 = r.decide(&subj(vec![stopping]), false, 1_000);
        assert_ne!(v0.rule, "R3-known-crash");
        let late = subj(vec![exit(2, "exited", "143"), failed(3, "exit-code")]);
        let v1 = r.decide(&late, false, 1_000 + PLANNED_HOLD_S);
        assert_eq!(v1.rule, "R5b-planned-stop", "{v1:?}");
        let mut r2 = rules();
        r2.decide(&subj(vec![stopped(1, "stop")]), false, 1_000);
        let v2 = r2.decide(&late, false, 1_000 + PLANNED_HOLD_S + 1);
        assert_eq!(v2.rule, "R3-known-crash", "hold expired");
    }

    #[test]
    fn nerve_planned_flag_on_the_failure_itself() {
        let mut f = failed(2, "exit-code");
        f.data["planned"] = json!(true);
        f.severity = "low".into();
        let mut r = rules();
        let v = r.decide(&subj(vec![f]), false, 1_000);
        assert_eq!(v.rule, "R5b-planned-stop");
    }

    /// The live nerve folds a 2nd restart inside its 600 s dedupe: the
    /// cortex sees it only as repeat events (count rises). A planned one
    /// carries its folded job marker + 143 exit; a crash loop the cortex
    /// restarts carries a non-stop exit and stays a crash; one planned
    /// restart + one crash folded together (failed +2) is not planned.
    #[test]
    fn folded_repeats_planned_vs_crash() {
        let rep = |mut e: Event, n: u32| {
            e.count = Some(n);
            e.data["repeat"] = json!({"of_seq": e.seq, "total": n + 1, "new": n});
            e
        };
        let mut r = rules();
        let planned = subj(vec![
            rep(failed(2, "exit-code"), 1),
            rep(exit(1, "exited", "143"), 1),
            rep(stopped(3, "restart"), 1),
        ]);
        assert_eq!(r.decide(&planned, false, 2_000).rule, "R5b-planned-stop");
        let crash_loop = subj(vec![
            rep(failed(2, "exit-code"), 1),
            rep(exit(5, "exited", "1"), 1),
            rep(stopped(3, "restart"), 1),
        ]);
        assert_eq!(r.decide(&crash_loop, false, 2_100).rule, "R3-known-crash");
        let mixed = subj(vec![
            rep(failed(2, "exit-code"), 2),
            rep(exit(1, "exited", "143"), 1),
            rep(stopped(3, "restart"), 1),
        ]);
        let mut r3 = rules();
        assert!(r3.planned_stop(&mixed, 3_000).is_none());
        assert_eq!(r3.decide(&mixed, false, 3_000).rule, "R3-known-crash");
    }

    #[test]
    fn three_planned_restarts_never_open_the_circuit() {
        // flap_failures 3 in 600 s: three cutover restarts in 3.5 min (the
        // live 17:25-17:28 sequence) must leave the next real crash to R3.
        let mut r = rules();
        for (i, t) in [0u64, 2, 205].into_iter().enumerate() {
            let b = i as u64 * 10;
            let s = subj(vec![
                exit(b + 1, "exited", "143"),
                failed(b + 2, "exit-code"),
                stopped(b + 3, "restart"),
                started(b + 4),
            ]);
            assert_eq!(r.decide(&s, false, 10_000 + t).rule, "R5b-planned-stop");
        }
        let crash = subj(vec![exit(99, "exited", "1"), failed(100, "exit-code")]);
        let v = r.decide(&crash, false, 10_300);
        assert_eq!(v.rule, "R3-known-crash", "{v:?}");
    }

    #[test]
    fn protected_unit_stays_r1_and_plain_stop_stays_r5() {
        let mut r = Rules::new(Guard::new(&["console.service".to_string()]), &[], limits());
        let s = subj(vec![
            exit(1, "exited", "143"),
            failed(2, "exit-code"),
            stopped(3, "restart"),
        ]);
        assert_eq!(r.decide(&s, false, 1_000).rule, "R1-protected");
        let mut r = rules();
        let only_stop = subj(vec![stopped(1, "stop")]);
        assert_eq!(r.decide(&only_stop, false, 1_000).rule, "R5-recovered");
    }

    // ---- slot 328: the slot 310 nerve tags the failure itself ----

    /// A failure/exit as the slot 310 nerve (12956b45) emits it: kind
    /// `unit.planned_stop`, `data.planned`, `planned_kind`, exit fields kept.
    fn tagged(seq: u64, was: &str, extra: Value) -> Event {
        let mut d = json!({"planned": true, "planned_kind": was, "stop_job": "restart"});
        if let (Some(o), Some(e)) = (d.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        jev("unit.planned_stop", "info", seq, d)
    }

    #[test]
    fn nerve_tagged_restart_is_r5b_not_r5_or_r99() {
        // The exact burst the e2e pair saw (slot 328 run 00:16 +01:00):
        // stopping, planned_stop (exit 143), planned_stop (failed), stopped, started.
        let mut stopping = jev(
            "unit.stopping",
            "info",
            1,
            json!({"job_type": "restart", "planned": true}),
        );
        stopping.key = Some(format!("unit.stopping:{CONSOLE}"));
        let burst = subj(vec![
            stopping.clone(),
            tagged(
                2,
                "unit.exit",
                json!({"exit_code": "exited", "exit_status": "143"}),
            ),
            tagged(3, "unit.failed", json!({"unit_result": "exit-code"})),
            stopped(4, "restart"),
            started(5),
        ]);
        let mut r = rules();
        let v = r.decide(&burst, false, 1_000);
        assert_eq!(v.rule, "R5b-planned-stop", "{v:?}");
        assert!(
            v.outcome == Outcome::None && !v.escalate && !v.alert,
            "{v:?}"
        );
        // without the active-again tail (the cortex burst split before Started)
        let short = subj(vec![
            stopping.clone(),
            tagged(
                2,
                "unit.exit",
                json!({"exit_code": "exited", "exit_status": "143"}),
            ),
            tagged(3, "unit.failed", json!({"unit_result": "exit-code"})),
        ]);
        assert_eq!(r.decide(&short, false, 1_010).rule, "R5b-planned-stop");
        // a Stopping line alone is still not stop-shaped (plain R5 as before)
        assert_ne!(
            rules().decide(&subj(vec![stopping]), false, 1_000).rule,
            "R5b-planned-stop"
        );
    }

    #[test]
    fn nerve_tagged_unclean_stop_is_not_r5b_and_never_restarts() {
        // The nerve tags an exit 1 during a stop job as planned (low); the
        // cortex sees crash evidence, so no R5b, but also no failure to restart.
        let s = subj(vec![
            tagged(
                2,
                "unit.exit",
                json!({"exit_code": "exited", "exit_status": "1"}),
            ),
            tagged(3, "unit.failed", json!({"unit_result": "exit-code"})),
        ]);
        let mut r = rules();
        assert!(r.planned_stop(&s, 1_000).is_none());
        let v = r.decide(&s, false, 1_000);
        assert_ne!(v.rule, "R5b-planned-stop", "{v:?}");
        assert!(!v.rule.starts_with("R3"), "{v:?}");
        assert_eq!(v.outcome, Outcome::None, "{v:?}");
    }
}
