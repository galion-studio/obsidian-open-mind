// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Unit fault incidents with a published recovery (slot 246, D-237-2).
//!
//! The journal source reports failures for *every* unit but lifecycle
//! (`Started`) only for tracked stack units, so a fault of `postfix`,
//! `ssh` or any other untracked unit was published and its recovery never
//! was: cortex incidents for those units stayed open forever.
//!
//! This book closes the loop:
//!
//! - the first failure event of a non-throwaway unit opens one unit
//!   incident (`incident.opened`, cause `unit:<unit>`); further failures of
//!   the same unit only count (no duplicate opens);
//! - a faulted unit is added to the journal's lifecycle list, so its
//!   `Started` / `Deactivated successfully` lines arrive and are published
//!   (`unit.started` / `unit.finished`);
//! - a start (journal) or an `active` / `inactive` row in the inventory
//!   (heartbeat, after event cycles) arms a recovery that is confirmed after
//!   `hold_s` seconds without a new failure (a crash loop never flaps
//!   open/resolved); then `unit.state` (`to` = the state seen) and
//!   `incident.resolved` (`id`, `cause`, `unit`, `resolved_by`) are published.
//!
//! Event keys carry the incident's open time, so a unit that faults again
//! after a recovery opens a new incident even inside the hub dedupe window.

use super::inventory::UnitRow;
use super::{classify_unit, NerveEvent, Severity};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

/// Event kinds that mean "this unit is failing".
pub const FAULT_KINDS: &[&str] = &[
    "unit.exit",
    "unit.failed",
    "unit.start_failed",
    "unit.spawn_failed",
    "unit.restart_scheduled",
    "unit.start_limit",
    "oom.kill",
];

/// Source name of the events this book publishes.
pub const SOURCE: &str = "units";

const KEEP_RESOLVED: usize = 50;

/// Event severity on the engine incident scale used by `GET /incidents`.
pub fn engine_sev(s: Severity) -> &'static str {
    match s {
        Severity::Critical => "critical",
        Severity::High => "error",
        Severity::Medium => "warn",
        _ => "info",
    }
}

/// Prefix of a unit incident's id / cause.
pub fn cause_of(unit: &str) -> String {
    format!("unit:{unit}")
}

#[derive(Debug, Clone)]
pub struct Fault {
    pub unit: String,
    pub group: String,
    pub opened: u64,
    pub last_fault: u64,
    pub faults: u32,
    pub first_kind: String,
    pub last_kind: String,
    pub severity: Severity,
    /// Recovery armed: (confirm at, evidence, state seen).
    pub pending: Option<(u64, String, String)>,
}

impl Fault {
    pub fn to_json(&self) -> Value {
        let base = self.unit.trim_end_matches(".service");
        json!({
            "id": cause_of(&self.unit), "cause": cause_of(&self.unit), "source": SOURCE, "state": "open",
            "unit": self.unit, "group": self.group, "severity": engine_sev(self.severity),
            "event_severity": self.severity.as_str(), "opened_t": self.opened,
            "title": format!("{} failing ({})", self.unit, self.last_kind),
            "components": [base], "opened": crate::util::ts(self.opened), "last_fault": crate::util::ts(self.last_fault),
            "faults": self.faults, "first_kind": self.first_kind, "last_kind": self.last_kind,
            "recovering": self.pending.as_ref().map(|p| json!({"confirm_at": p.0, "by": p.1, "state": p.2})),
        })
    }
}

#[derive(Debug)]
pub struct UnitFaults {
    pub open: BTreeMap<String, Fault>,
    pub resolved: VecDeque<Value>,
    pub hold_s: u64,
    pub opened_total: u64,
    pub resolved_total: u64,
    /// Faults that hit an already open incident (would have been duplicate opens).
    pub repeat_faults: u64,
    /// Recoveries cancelled by a new failure inside the hold (crash loop).
    pub recoveries_cancelled: u64,
    /// Faulted units, shared with the journal parser (their lifecycle is published).
    pub tracked: Arc<Mutex<BTreeSet<String>>>,
}

impl UnitFaults {
    pub fn new(hold_s: u64) -> Self {
        UnitFaults {
            open: BTreeMap::new(),
            resolved: VecDeque::new(),
            hold_s,
            opened_total: 0,
            resolved_total: 0,
            repeat_faults: 0,
            recoveries_cancelled: 0,
            tracked: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    fn sync_tracked(&self) {
        *self.tracked.lock().unwrap_or_else(|e| e.into_inner()) =
            self.open.keys().cloned().collect();
    }

    /// Unit of an event that counts as a fault, if any.
    pub fn fault_unit(ev: &NerveEvent) -> Option<&str> {
        if ev.source == SOURCE || !FAULT_KINDS.contains(&ev.kind.as_str()) {
            return None;
        }
        let u = ev.unit.as_deref()?;
        if u.is_empty() || u == "-.slice" || u.ends_with(".slice") {
            return None;
        }
        if matches!(classify_unit(u), "transient" | "sandbox") {
            return None;
        }
        Some(u)
    }

    /// Feed one published event. Returns the events to publish.
    pub fn observe(&mut self, ev: &NerveEvent, now: u64) -> Vec<NerveEvent> {
        if let Some(u) = Self::fault_unit(ev) {
            return self.fault(u, &ev.kind, ev.severity, &ev.group, now);
        }
        if ev.source == SOURCE {
            return vec![];
        }
        let Some(u) = ev.unit.as_deref() else {
            return vec![];
        };
        match ev.kind.as_str() {
            "unit.started" => self.arm(u, "journal:started", "active", now),
            "unit.finished" => self.arm(u, "journal:finished", "inactive", now),
            _ => vec![],
        }
    }

    fn fault(
        &mut self,
        unit: &str,
        kind: &str,
        sev: Severity,
        group: &str,
        now: u64,
    ) -> Vec<NerveEvent> {
        if let Some(f) = self.open.get_mut(unit) {
            f.faults = f.faults.saturating_add(1);
            f.last_fault = now;
            f.last_kind = kind.to_string();
            f.severity = f.severity.max(sev);
            self.repeat_faults += 1;
            if f.pending.take().is_some() {
                self.recoveries_cancelled += 1;
            }
            return vec![];
        }
        let f = Fault {
            unit: unit.to_string(),
            group: group.to_string(),
            opened: now,
            last_fault: now,
            faults: 1,
            first_kind: kind.to_string(),
            last_kind: kind.to_string(),
            severity: sev,
            pending: None,
        };
        self.opened_total += 1;
        let cause = cause_of(unit);
        let ev = NerveEvent::new(
            SOURCE,
            "incident.opened",
            sev,
            format!("{unit} failing ({kind}); resolves when it runs again"),
        )
        .group(group)
        .key(format!("incident.opened:{cause}:{now}"))
        .data(json!({
            "id": cause, "cause": cause, "unit": unit, "kind": kind,
            "components": [unit.trim_end_matches(".service")],
            "opened": now, "recovers_on": ["unit.started", "unit.finished", "inventory active/inactive"],
            "next": format!("watch {unit}; the nerve publishes incident.resolved once it stays up {}s", self.hold_s),
        }))
        .at(now);
        self.open.insert(unit.to_string(), f);
        self.sync_tracked();
        vec![ev]
    }

    /// Arm a recovery (confirmed after `hold_s` without a new failure).
    fn arm(&mut self, unit: &str, by: &str, state: &str, now: u64) -> Vec<NerveEvent> {
        let Some(f) = self.open.get_mut(unit) else {
            return vec![];
        };
        if f.pending.is_none() {
            f.pending = Some((now + self.hold_s, by.to_string(), state.to_string()));
        }
        if self.hold_s == 0 {
            return self.due(now);
        }
        vec![]
    }

    /// Recoveries whose hold has passed.
    pub fn due(&mut self, now: u64) -> Vec<NerveEvent> {
        let ready: Vec<String> = self
            .open
            .iter()
            .filter(|(_, f)| f.pending.as_ref().is_some_and(|p| p.0 <= now))
            .map(|(u, _)| u.clone())
            .collect();
        let mut out = vec![];
        for u in ready {
            let Some(f) = self.open.remove(&u) else {
                continue;
            };
            let (_, by, state) = f.pending.clone().unwrap_or_default();
            let cause = cause_of(&u);
            let open_s = now.saturating_sub(f.opened);
            out.push(
                NerveEvent::new(
                    SOURCE,
                    "unit.state",
                    Severity::Info,
                    format!("{u} {state} again after {open_s}s ({by})"),
                )
                .unit(&u)
                .key(format!("unit.state:{u}:recovered:{}", f.opened))
                .data(json!({"to": state, "active_state": state, "recovered": true, "by": by}))
                .at(now),
            );
            out.push(
                NerveEvent::new(
                    SOURCE,
                    "incident.resolved",
                    Severity::Info,
                    format!("incident {cause} resolved: {u} {state} ({by})"),
                )
                .group(&f.group)
                .key(format!("incident.resolved:{cause}:{}", f.opened))
                .data(json!({
                    "id": cause, "cause": cause, "unit": u, "resolved_by": by, "state": state,
                    "opened": f.opened, "open_s": open_s, "faults": f.faults,
                }))
                .at(now),
            );
            let mut rec = f.to_json();
            rec["state"] = json!("resolved");
            rec["resolved"] = json!(crate::util::ts(now));
            rec["duration_s"] = json!(open_s);
            rec["resolved_by"] = json!(by);
            self.resolved.push_back(rec);
            while self.resolved.len() > KEEP_RESOLVED {
                self.resolved.pop_front();
            }
            self.resolved_total += 1;
        }
        if !out.is_empty() {
            self.sync_tracked();
        }
        out
    }

    /// Inventory rows (`systemctl list-units --type=service --all`): an open
    /// unit seen `active` or `inactive` arms its recovery, `failed` cancels
    /// one. Only used when the listing itself worked (`healthy`).
    pub fn inventory(&mut self, rows: &[UnitRow], healthy: bool, now: u64) -> Vec<NerveEvent> {
        if !healthy || rows.is_empty() {
            return vec![];
        }
        let units: Vec<String> = self.open.keys().cloned().collect();
        let mut out = vec![];
        for u in units {
            match rows.iter().find(|r| r.unit == u).map(|r| r.active.as_str()) {
                Some("active") => out.extend(self.arm(&u, "inventory:active", "active", now)),
                Some("inactive") => out.extend(self.arm(&u, "inventory:inactive", "inactive", now)),
                Some("failed") => {
                    if let Some(f) = self.open.get_mut(&u) {
                        if f.pending.take().is_some() {
                            self.recoveries_cancelled += 1;
                        }
                    }
                }
                // activating / deactivating / reloading: undecided.
                Some(_) => {}
                // A service missing from the `--type=service --all` listing is
                // gone (unit file removed, never loaded): nothing to wait for.
                // `.path` / `.timer` / `.socket` units are never listed there:
                // only their journal `Started` recovers them.
                None if u.ends_with(".service") => {
                    out.extend(self.arm(&u, "inventory:gone", "inactive", now))
                }
                None => {}
            }
        }
        out
    }

    /// Earliest pending recovery confirmation.
    pub fn deadline(&self) -> Option<u64> {
        self.open
            .values()
            .filter_map(|f| f.pending.as_ref().map(|p| p.0))
            .min()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "open": self.open.values().map(Fault::to_json).collect::<Vec<_>>(),
            "resolved": self.resolved.iter().rev().take(20).cloned().collect::<Vec<_>>(),
            "opened_total": self.opened_total, "resolved_total": self.resolved_total,
            "repeat_faults": self.repeat_faults, "recoveries_cancelled": self.recoveries_cancelled,
            "hold_s": self.hold_s,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail(u: &str, kind: &str) -> NerveEvent {
        NerveEvent::new("journal", kind, Severity::High, "x").unit(u)
    }
    fn started(u: &str) -> NerveEvent {
        NerveEvent::new("journal", "unit.started", Severity::Medium, "x").unit(u)
    }
    fn row(u: &str, active: &str) -> UnitRow {
        UnitRow {
            unit: u.into(),
            load: "loaded".into(),
            active: active.into(),
            sub: "x".into(),
            desc: String::new(),
        }
    }

    #[test]
    fn open_once_resolve_after_hold() {
        let mut b = UnitFaults::new(30);
        let o = b.observe(&fail("postfix.service", "unit.failed"), 100);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].kind, "incident.opened");
        assert_eq!(o[0].data["cause"], "unit:postfix.service");
        assert_eq!(o[0].key, "incident.opened:unit:postfix.service:100");
        assert!(
            o[0].unit.is_none(),
            "cortex subject = nerve-incident:<cause>"
        );
        assert!(b.tracked.lock().unwrap().contains("postfix.service"));
        // repeats: no duplicate open
        for k in ["unit.exit", "unit.restart_scheduled", "unit.failed"] {
            assert!(b.observe(&fail("postfix.service", k), 101).is_empty());
        }
        assert_eq!((b.opened_total, b.repeat_faults), (1, 3));
        assert!(b.observe(&started("postfix.service"), 110).is_empty());
        assert_eq!(b.deadline(), Some(140));
        assert!(b.due(139).is_empty());
        let r = b.due(140);
        assert_eq!(
            r.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            ["unit.state", "incident.resolved"]
        );
        assert_eq!(r[0].unit.as_deref(), Some("postfix.service"));
        assert_eq!(r[0].data["to"], "active");
        assert_eq!(r[1].data["id"], "unit:postfix.service");
        assert_eq!(r[1].data["cause"], "unit:postfix.service");
        assert_eq!(r[1].data["resolved_by"], "journal:started");
        assert_eq!(r[1].data["faults"], 4);
        assert_eq!(r[1].key, "incident.resolved:unit:postfix.service:100");
        assert!(b.open.is_empty() && b.tracked.lock().unwrap().is_empty());
        // a new fault later opens a new incident with a new key
        let o2 = b.observe(&fail("postfix.service", "unit.failed"), 200);
        assert_eq!(o2[0].key, "incident.opened:unit:postfix.service:200");
        assert_eq!(b.opened_total, 2);
    }

    #[test]
    fn crash_loop_cancels_recovery() {
        let mut b = UnitFaults::new(30);
        b.observe(&fail("ssh.service", "unit.exit"), 0);
        for t in [5, 15, 25] {
            b.observe(&started("ssh.service"), t);
            assert!(b
                .observe(&fail("ssh.service", "unit.exit"), t + 2)
                .is_empty());
        }
        assert!(b.due(100).is_empty(), "still crash looping");
        assert_eq!(b.recoveries_cancelled, 3);
        b.observe(&started("ssh.service"), 100);
        assert_eq!(b.due(130).len(), 2);
    }

    #[test]
    fn throwaway_units_and_own_events_ignored() {
        let mut b = UnitFaults::new(0);
        for u in [
            "run-u12.service",
            "session-3.scope",
            "slot-sbx-1.service",
            "-.slice",
        ] {
            assert!(b.observe(&fail(u, "unit.failed"), 1).is_empty(), "{u}");
        }
        assert!(b
            .observe(
                &NerveEvent::new("journal", "unit.started", Severity::Medium, "x")
                    .unit("x.service"),
                1
            )
            .is_empty());
        let mut own = fail("postfix.service", "unit.failed");
        own.source = SOURCE.into();
        assert!(b.observe(&own, 1).is_empty());
        assert_eq!(b.opened_total, 0);
    }

    #[test]
    fn inventory_confirms_and_cancels() {
        let mut b = UnitFaults::new(10);
        b.observe(&fail("postfix.service", "unit.failed"), 0);
        b.observe(&fail("cron.service", "unit.failed"), 0);
        b.observe(&fail("gone.service", "unit.failed"), 0);
        b.observe(&fail("gaia-hermes-handoff.path", "unit.failed"), 0);
        // broken listing: no decision
        assert!(b.inventory(&[], true, 1).is_empty());
        assert!(b
            .inventory(&[row("postfix.service", "active")], false, 1)
            .is_empty());
        assert_eq!(b.deadline(), None);
        let rows = [
            row("postfix.service", "active"),
            row("cron.service", "failed"),
            row("x.service", "active"),
        ];
        b.inventory(&rows, true, 5);
        assert_eq!(b.deadline(), Some(15));
        let r = b.due(15);
        let res: Vec<_> = r.iter().filter(|e| e.kind == "incident.resolved").collect();
        assert_eq!(res.len(), 2, "postfix active + gone.service unlisted");
        assert!(b.open.contains_key("cron.service"));
        assert!(
            b.open.contains_key("gaia-hermes-handoff.path"),
            "a .path unit is never in the service listing: not gone"
        );
        assert!(res
            .iter()
            .any(|e| e.data["resolved_by"] == "inventory:gone"));
        // hold 0: immediate
        let mut z = UnitFaults::new(0);
        z.observe(&fail("postfix.service", "unit.failed"), 0);
        assert_eq!(z.observe(&started("postfix.service"), 3).len(), 2);
    }
}
