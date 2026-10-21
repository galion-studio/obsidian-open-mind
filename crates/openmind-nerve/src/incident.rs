// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Correlation: several failing signals become one incident.
//!
//! Every cycle the engine and the cortex emit [`Signal`]s, each tagged with a
//! `cause` key. Signals that share a cause are one incident:
//!
//! - component verdicts carry their component's `group` (galion-one and its
//!   mouth on 8002 are one group; cloudflared, the quick tunnel and the dead
//!   tunnel routes are the `edge` group);
//! - a request flood on a stream that fronts a component pulls that
//!   component's own signals (latency, 5xx, probe down) into the `ddos:`
//!   incident, so "tuner is slow" and "tuner is flooded" are one story;
//! - when 3 or more groups fail hard in the same cycle, they fold into one
//!   `stack-wide` incident (a host or network problem, not N separate ones).
//!
//! An incident opens on its first signal, keeps a stable id, and resolves
//! after [`RESOLVE_AFTER`] consecutive cycles without signals. Resolved
//! incidents are kept (newest 50) for the API.

use crate::util::ts;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const RESOLVE_AFTER: u32 = 2;
const KEEP_RESOLVED: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sev {
    Info,
    Warn,
    Error,
    Critical,
}

impl Sev {
    pub fn as_str(self) -> &'static str {
        match self {
            Sev::Info => "info",
            Sev::Warn => "warn",
            Sev::Error => "error",
            Sev::Critical => "critical",
        }
    }

    /// B41 severity (docs/cortex.md section 2) for a B33 `NerveEvent`:
    /// warn -> `warn`, error -> `high`, critical -> `crit`. B41 has no info
    /// level; info stays `info` (below every B41 alert line).
    pub fn b41(self) -> &'static str {
        match self {
            Sev::Info => "info",
            Sev::Warn => "warn",
            Sev::Error => "high",
            Sev::Critical => "crit",
        }
    }

    /// Parse a severity from a pushed event: B41 (`warn`, `high`, `crit`)
    /// or nerve (`info`, `warn`, `error`, `critical`) spelling.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "info" => Sev::Info,
            "warn" | "warning" => Sev::Warn,
            "high" | "error" => Sev::Error,
            "crit" | "critical" => Sev::Critical,
            _ => return None,
        })
    }
}

/// One failing observation in one cycle.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub cause: String,
    pub component: String,
    pub sev: Sev,
    pub msg: String,
    /// Hard failure (unit down, probe down): counts toward `stack-wide`.
    pub hard: bool,
}

impl Signal {
    pub fn new(cause: &str, component: &str, sev: Sev, msg: impl Into<String>) -> Self {
        Signal {
            cause: cause.into(),
            component: component.into(),
            sev,
            msg: msg.into(),
            hard: false,
        }
    }
}

/// What a cause means: title, why it matters, the next step.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Explain {
    pub title: String,
    pub why: String,
    pub next: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Incident {
    pub id: String,
    pub cause: String,
    pub title: String,
    pub sev: Sev,
    pub opened: u64,
    pub last_seen: u64,
    pub cycles: u32,
    pub quiet_cycles: u32,
    pub resolved: Option<u64>,
    pub components: BTreeSet<String>,
    pub signals: Vec<Signal>,
    pub why: String,
    pub next: String,
    /// component -> owning Gaia function ("unknown" when not in the table);
    /// set by [`crate::cortex::Cortex::attach_owners`].
    pub owners: BTreeMap<String, String>,
}

impl Incident {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "cause": self.cause, "title": self.title, "severity": self.sev.as_str(),
            "state": if self.resolved.is_some() { "resolved" } else { "open" },
            "opened": ts(self.opened), "last_seen": ts(self.last_seen),
            "resolved": self.resolved.map(ts), "cycles": self.cycles,
            "duration_s": self.resolved.unwrap_or(self.last_seen).saturating_sub(self.opened),
            "components": self.components,
            "signals": self.signals.iter().map(|s| json!({"component": s.component, "severity": s.sev.as_str(), "msg": s.msg})).collect::<Vec<_>>(),
            "why": self.why, "next": self.next,
            "gaia": self.owners.values().collect::<BTreeSet<_>>(), "owners": self.owners,
        })
    }
}

/// Fold signals into causes (the correlation rules above).
pub fn correlate(mut sigs: Vec<Signal>) -> BTreeMap<String, Vec<Signal>> {
    // DDoS pulls in its target component's signals.
    let targets: BTreeMap<String, String> = sigs
        .iter()
        .filter(|s| s.cause.starts_with("ddos:") && s.sev >= Sev::Error)
        .map(|s| (s.component.clone(), s.cause.clone()))
        .collect();
    for s in sigs.iter_mut() {
        if !s.cause.starts_with("ddos:") {
            if let Some(c) = targets.get(&s.component) {
                s.cause = c.clone();
            }
        }
    }
    let hard_groups: BTreeSet<String> = sigs
        .iter()
        .filter(|s| s.hard && !s.cause.starts_with("ddos:"))
        .map(|s| s.cause.clone())
        .collect();
    if hard_groups.len() >= 3 {
        for s in sigs.iter_mut() {
            if hard_groups.contains(&s.cause) {
                s.cause = "stack-wide".into();
            }
        }
    }
    let mut m: BTreeMap<String, Vec<Signal>> = BTreeMap::new();
    for s in sigs {
        m.entry(s.cause.clone()).or_default().push(s);
    }
    m
}

fn short_id(cause: &str, opened: u64) -> String {
    // FNV-1a: stable, dependency-free, not security relevant.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in cause.bytes().chain(opened.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("inc-{:08x}", h as u32)
}

#[derive(Debug, Clone, Default)]
pub struct Correlator {
    pub open: BTreeMap<String, Incident>,
    pub resolved: VecDeque<Incident>,
    pub opened_total: u64,
}

impl Correlator {
    /// One cycle. `explain` turns a cause and its signals into words.
    pub fn update(
        &mut self,
        now: u64,
        sigs: Vec<Signal>,
        explain: &dyn Fn(&str, &[Signal]) -> Explain,
    ) {
        let grouped = correlate(sigs);
        for (cause, ss) in &grouped {
            let sev = ss.iter().map(|s| s.sev).max().unwrap_or(Sev::Info);
            if sev == Sev::Info {
                continue;
            }
            let ex = explain(cause, ss);
            let inc = self.open.entry(cause.clone()).or_insert_with(|| {
                self.opened_total += 1;
                Incident {
                    id: short_id(cause, now),
                    cause: cause.clone(),
                    title: String::new(),
                    sev,
                    opened: now,
                    last_seen: now,
                    cycles: 0,
                    quiet_cycles: 0,
                    resolved: None,
                    components: BTreeSet::new(),
                    signals: vec![],
                    why: String::new(),
                    next: String::new(),
                    owners: BTreeMap::new(),
                }
            });
            inc.sev = sev;
            inc.last_seen = now;
            inc.cycles += 1;
            inc.quiet_cycles = 0;
            inc.components
                .extend(ss.iter().map(|s| s.component.clone()));
            let mut v = ss.clone();
            v.sort_by(|a, b| b.sev.cmp(&a.sev).then(a.component.cmp(&b.component)));
            v.truncate(24);
            inc.signals = v;
            inc.title = ex.title;
            inc.why = ex.why;
            inc.next = ex.next;
        }
        let quiet: Vec<String> = self
            .open
            .keys()
            .filter(|k| {
                grouped
                    .get(*k)
                    .map(|v| v.iter().all(|s| s.sev == Sev::Info))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        for k in quiet {
            let done = {
                let i = self.open.get_mut(&k).expect("key from map");
                i.quiet_cycles += 1;
                i.quiet_cycles >= RESOLVE_AFTER
            };
            if done {
                if let Some(mut i) = self.open.remove(&k) {
                    i.resolved = Some(now);
                    self.resolved.push_back(i);
                    while self.resolved.len() > KEEP_RESOLVED {
                        self.resolved.pop_front();
                    }
                }
            }
        }
    }

    /// Open incidents, worst first.
    pub fn open_sorted(&self) -> Vec<&Incident> {
        let mut v: Vec<&Incident> = self.open.values().collect();
        v.sort_by(|a, b| b.sev.cmp(&a.sev).then(a.opened.cmp(&b.opened)));
        v
    }

    pub fn to_json(&self) -> Value {
        json!({
            "open": self.open_sorted().iter().map(|i| i.to_json()).collect::<Vec<_>>(),
            "resolved": self.resolved.iter().rev().take(20).map(Incident::to_json).collect::<Vec<_>>(),
            "opened_total": self.opened_total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(c: &str, s: &[Signal]) -> Explain {
        Explain {
            title: format!("{c} x{}", s.len()),
            ..Default::default()
        }
    }

    #[test]
    fn same_group_is_one_incident_and_resolves() {
        let mut c = Correlator::default();
        let sigs = vec![
            Signal::new("galion-one", "galion-one-mouth", Sev::Warn, "503"),
            Signal::new("galion-one", "galion-one", Sev::Error, "probe down"),
            Signal::new("edge", "cloudflared", Sev::Error, "dead route 4001"),
            Signal::new("edge", "cloudflared", Sev::Error, "dead route 3011"),
            Signal::new("noise", "x", Sev::Info, "fyi"),
        ];
        c.update(100, sigs.clone(), &ex);
        assert_eq!(c.open.len(), 2);
        let one = &c.open["galion-one"];
        assert_eq!(one.sev, Sev::Error);
        assert_eq!(one.components.len(), 2);
        let id = one.id.clone();
        c.update(130, sigs, &ex);
        assert_eq!(c.open["galion-one"].id, id, "stable id");
        assert_eq!(c.open["galion-one"].cycles, 2);
        c.update(160, vec![], &ex);
        assert_eq!(c.open.len(), 2, "one quiet cycle is not enough");
        c.update(190, vec![], &ex);
        assert!(c.open.is_empty());
        assert_eq!(c.resolved.len(), 2);
        assert_eq!(c.resolved[0].resolved, Some(190));
    }

    #[test]
    fn ddos_absorbs_target_and_stack_wide_folds() {
        let mut flood = Signal::new("ddos:tuner", "tuner", Sev::Critical, "flood");
        flood.hard = false;
        let mut lat = Signal::new("tuner", "tuner", Sev::Warn, "latency");
        lat.hard = true;
        let g = correlate(vec![flood, lat]);
        assert_eq!(g.len(), 1);
        assert_eq!(g["ddos:tuner"].len(), 2);
        let mk = |c: &str| {
            let mut s = Signal::new(c, c, Sev::Critical, "down");
            s.hard = true;
            s
        };
        let g = correlate(vec![
            mk("a"),
            mk("b"),
            mk("c"),
            Signal::new("d", "d", Sev::Warn, "w"),
        ]);
        assert_eq!(g["stack-wide"].len(), 3);
        assert!(g.contains_key("d"));
    }
}
