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
//! | `R4-unknown-unit` | failure of a unit no rule may restart | alert + escalate |
//! | `R4b-sandbox-oom` | `oom.kill` on a `sbxtest*` unit / `sbxtest.slice` (sandbox memory cap) | info only, never escalate to Hermes |
//! | `R5-recovered` | unit back to `active` / info state change | nothing (incident quiets) |
//! | `R6-task-failed` | task/slot finished with `ok=false` | alert |
//! | `R7-threshold` | threshold crossed: warn | alert; error+ | alert + escalate (LLM-eligible) |
//! | `R8-unclassified` | unclassified process: warn | alert; error+ | escalate (LLM-eligible) |
//! | `R9-process-exit` | process exit: non-zero, warn+ | alert |
//! | `R10-remediation` | nerve `action.result` / `circuit.open` | rate-limited/circuit-open opens the cortex circuit + escalates once; echoes of our own requests are info |
//! | `R11-nerve-incident` | nerve engine `incident.opened` (warn/error alert, critical escalate) / `incident.resolved` (info) | no action |
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
    fn new(rule: &'static str, sev: Sev, reason: impl Into<String>) -> Self {
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
        let c = |f: &dyn Fn(&Event) -> bool| {
            self.events
                .iter()
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
            && self.events.iter().any(|e| {
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
        }
    }

    pub fn restart_allowed(&self, unit: &str) -> bool {
        self.guard.unit(unit).is_none() && self.allow.iter().any(|a| a == unit)
    }

    pub fn circuit_open(&self, unit: &str, now: u64) -> bool {
        self.books
            .get(unit)
            .and_then(|b| b.circuit_until)
            .is_some_and(|t| t > now)
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
            let fails = s.failures();
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
                    return Verdict::new(
                        "R4-unknown-unit",
                        sev.max(Sev::Warn),
                        format!("{u} failed; not on the restart allowlist, no rule can act"),
                    )
                    .escalate();
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

/// Subject key and unit for an event.
pub fn subject_key(e: &Event) -> (String, Option<String>) {
    if let Some(u) = e.unit() {
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
}
