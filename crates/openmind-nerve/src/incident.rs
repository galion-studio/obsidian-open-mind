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
//! # Cross-sensor causes (cortex plan T3.2)
//!
//! Cortex findings carry a `cause` too ([`crate::cortex::Finding::cause`]).
//! [`Correlator::update_with`] takes them as [`Member`]s, so one incident
//! holds every sensor's view of one problem:
//!
//! | rule | what becomes one incident |
//! |---|---|
//! | [`RULE_SAME_CAUSE`] | signals and findings with one cause key: `edge` = dead origins (sensor `edge`) + ingress lint closed routes (`ingress-lint`) + DNS/TLS/HTTP surface (`surface`) + the tunnel components; `ssh-flood` = the sshd credential flood + the auth-log disk bleed it writes (`auth.log`) |
//! | [`RULE_DDOS_TARGET`] | a flood (`ddos:<stream>`, error or worse) pulls its target component's signals |
//! | [`RULE_SSH_DISK`] | an `ssh-flood` signal (warn or worse) **and** an auth-log bleed finding with cause `ssh-flood` in the same cycle pull the disk-only host signals (`disk 91% used`, `disk full in ~h`) into `ssh-flood`: the flood writes the log that fills the disk |
//! | [`RULE_STACK_WIDE`] | 3+ hard-failing groups fold into `stack-wide` |
//!
//! A member joins the incident that score v2 charges for it
//! ([`crate::score::covering`]): same cause first, else an absorbing
//! incident (`ddos:<stream>`, `stack-wide`) that lists its component. The
//! incident's severity stays the worst of its **signals** (alerts, reflexes
//! and the system status are unchanged); score v2 charges the incident at
//! least what its worst member would cost alone ([`crate::score`], "member
//! floor"), so a covered finding never costs less inside its incident than
//! alone. Members never add components (no extra-component points) and
//! never open an incident on their own: a finding without a signal of its
//! cause is scored by score v2 as a finding, once per cause. Every
//! incident's JSON gets a `correlation` section (rules, sensors, member
//! findings, folded signals, worst finding).
//!
//! An incident opens on its first signal, keeps a stable id, and resolves
//! after [`RESOLVE_AFTER`] consecutive cycles without signals. Resolved
//! incidents are kept (newest 50) for the API.

use crate::util::ts;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const RESOLVE_AFTER: u32 = 2;
const KEEP_RESOLVED: usize = 50;

/// Signals (and findings) that share one cause key.
pub const RULE_SAME_CAUSE: &str = "same-cause";
/// A `ddos:<stream>` flood absorbed its target component's signals.
pub const RULE_DDOS_TARGET: &str = "ddos-target";
/// The ssh flood absorbed the host's disk signals (auth-log bleed seen).
pub const RULE_SSH_DISK: &str = "ssh-disk-bleed";
/// 3+ hard-failing groups folded into `stack-wide`.
pub const RULE_STACK_WIDE: &str = "stack-wide";
/// Cause key of the sshd credential flood and its auth-log bleed.
pub const SSH_FLOOD: &str = "ssh-flood";

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

/// A cortex finding as the correlator sees it (T3.2): no title, no
/// evidence, only what joins it to an incident.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// `<kind>:<id>`, the key score v2 lists in `covered`.
    pub key: String,
    /// The sensor (B33 source) that saw it.
    pub source: String,
    pub cause: Option<String>,
    pub component: String,
    pub sev: Sev,
}

impl Member {
    pub fn new(key: &str, source: &str, cause: Option<&str>, component: &str, sev: Sev) -> Self {
        Member {
            key: key.into(),
            source: source.into(),
            cause: cause.map(str::to_string),
            component: component.into(),
            sev,
        }
    }

    fn is_auth_log_bleed(&self) -> bool {
        self.cause.as_deref() == Some(SSH_FLOOD)
            && self.key.starts_with("bleeding:")
            && self.sev >= Sev::Warn
    }
}

/// One signal a rule moved from its own cause into another incident.
#[derive(Debug, Clone, PartialEq)]
pub struct Fold {
    pub from: String,
    pub component: String,
    pub rule: &'static str,
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
    /// Findings that joined this incident in its last active cycle (T3.2).
    pub members: Vec<Member>,
    /// Signals folded in from other causes in its last active cycle.
    pub folds: Vec<Fold>,
}

impl Incident {
    /// Correlation rules that built this incident in its last active cycle.
    pub fn rules(&self) -> Vec<&'static str> {
        let mut r = vec![RULE_SAME_CAUSE];
        for f in &self.folds {
            if !r.contains(&f.rule) {
                r.push(f.rule);
            }
        }
        r
    }

    /// The worst member finding (first of the worst severity).
    pub fn worst_member(&self) -> Option<&Member> {
        self.members
            .iter()
            .reduce(|a, b| if b.sev > a.sev { b } else { a })
    }

    /// Sensors behind the member findings (sorted, unique).
    pub fn sensors(&self) -> Vec<&str> {
        let s: BTreeSet<&str> = self.members.iter().map(|m| m.source.as_str()).collect();
        s.into_iter().collect()
    }

    /// The `correlation` section (T3.2, additive to the pre-T3.2 shape).
    pub fn correlation_json(&self) -> Value {
        json!({
            "rules": self.rules(),
            "sensors": self.sensors(),
            "findings": self.members.iter().map(|m| json!({"finding": m.key, "source": m.source, "severity": m.sev.as_str()})).collect::<Vec<_>>(),
            "folded": self.folds.iter().map(|f| json!({"from": f.from, "component": f.component, "rule": f.rule})).collect::<Vec<_>>(),
            "worst_finding": self.worst_member().map(|m| json!({"finding": m.key, "severity": m.sev.as_str()})),
        })
    }

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
            "correlation": self.correlation_json(),
        })
    }
}

/// `ddos:<stream>` and `stack-wide` absorb other components' signals, so
/// they also stand for those components' findings.
pub fn absorbing(cause: &str) -> bool {
    cause.starts_with("ddos:") || cause == "stack-wide"
}

/// A host signal made only of disk reasons (`degrading: disk 91% used`,
/// `failing-soon: disk full in ~3.0 h at this rate`; the status prefix of
/// [`crate::state::CompState`] reasons is optional), the kind an auth-log
/// bleed causes.
pub fn disk_only(s: &Signal) -> bool {
    const STATUS: [&str; 5] = ["healthy", "degrading", "failing-soon", "failing", "absent"];
    !s.msg.is_empty()
        && s.msg.split("; ").all(|r| {
            let r = match r.split_once(": ") {
                Some((st, rest)) if STATUS.contains(&st) => rest,
                _ => r,
            };
            r.starts_with("disk ")
        })
}

/// Fold signals into causes (the correlation rules above, no findings).
pub fn correlate(sigs: Vec<Signal>) -> BTreeMap<String, Vec<Signal>> {
    correlate_with(sigs, &[]).0
}

/// Fold signals into causes with the cycle's findings as evidence for the
/// cross-sensor rules. Returns the groups and, per target cause, the
/// signals each rule moved there.
pub fn correlate_with(
    mut sigs: Vec<Signal>,
    members: &[Member],
) -> (BTreeMap<String, Vec<Signal>>, BTreeMap<String, Vec<Fold>>) {
    let mut folds: BTreeMap<String, Vec<Fold>> = BTreeMap::new();
    let mut fold = |s: &mut Signal, into: &str, rule: &'static str| {
        folds.entry(into.to_string()).or_default().push(Fold {
            from: std::mem::replace(&mut s.cause, into.to_string()),
            component: s.component.clone(),
            rule,
        });
    };
    // DDoS pulls in its target component's signals.
    let targets: BTreeMap<String, String> = sigs
        .iter()
        .filter(|s| s.cause.starts_with("ddos:") && s.sev >= Sev::Error)
        .map(|s| (s.component.clone(), s.cause.clone()))
        .collect();
    for s in sigs.iter_mut() {
        if !s.cause.starts_with("ddos:") {
            if let Some(c) = targets.get(&s.component) {
                fold(s, c, RULE_DDOS_TARGET);
            }
        }
    }
    // The ssh flood pulls in the disk signals its auth-log bleed explains.
    let flood = sigs
        .iter()
        .any(|s| s.cause == SSH_FLOOD && s.sev >= Sev::Warn);
    if flood && members.iter().any(Member::is_auth_log_bleed) {
        for s in sigs.iter_mut() {
            if s.cause != SSH_FLOOD && !absorbing(&s.cause) && disk_only(s) {
                fold(s, SSH_FLOOD, RULE_SSH_DISK);
            }
        }
    }
    let hard_groups: BTreeSet<String> = sigs
        .iter()
        .filter(|s| s.hard && !s.cause.starts_with("ddos:") && s.cause != SSH_FLOOD)
        .map(|s| s.cause.clone())
        .collect();
    if hard_groups.len() >= 3 {
        for s in sigs.iter_mut() {
            if hard_groups.contains(&s.cause) {
                fold(s, "stack-wide", RULE_STACK_WIDE);
            }
        }
    }
    let mut m: BTreeMap<String, Vec<Signal>> = BTreeMap::new();
    for s in sigs {
        m.entry(s.cause.clone()).or_default().push(s);
    }
    // A fold chain (ddos -> stack-wide is impossible, but disk -> ssh-flood
    // -> nothing) keeps only folds whose target still exists.
    folds.retain(|c, _| m.contains_key(c));
    (m, folds)
}

/// The cause group a member joins (same rule as [`crate::score::covering`]):
/// its own cause when that group is an incident this cycle, else the first
/// absorbing group whose signals name its component.
fn member_home<'a>(
    m: &Member,
    groups: &'a BTreeMap<String, Vec<Signal>>,
    live: &BTreeSet<&str>,
) -> Option<&'a str> {
    if let Some(c) = &m.cause {
        if let Some((k, _)) = groups.get_key_value(c) {
            if live.contains(k.as_str()) {
                return Some(k.as_str());
            }
        }
    }
    groups
        .iter()
        .find(|(k, ss)| {
            live.contains(k.as_str())
                && absorbing(k)
                && ss.iter().any(|s| s.component == m.component)
        })
        .map(|(k, _)| k.as_str())
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
    /// One cycle without findings. `explain` turns a cause and its signals
    /// into words.
    pub fn update(
        &mut self,
        now: u64,
        sigs: Vec<Signal>,
        explain: &dyn Fn(&str, &[Signal]) -> Explain,
    ) {
        self.update_with(now, sigs, &[], explain)
    }

    /// One cycle with the cortex findings as cross-sensor [`Member`]s.
    pub fn update_with(
        &mut self,
        now: u64,
        sigs: Vec<Signal>,
        members: &[Member],
        explain: &dyn Fn(&str, &[Signal]) -> Explain,
    ) {
        let (grouped, mut folds) = correlate_with(sigs, members);
        let live: BTreeSet<&str> = grouped
            .iter()
            .filter(|(_, ss)| ss.iter().any(|s| s.sev > Sev::Info))
            .map(|(k, _)| k.as_str())
            .collect();
        let mut homes: BTreeMap<&str, Vec<Member>> = BTreeMap::new();
        for m in members.iter().filter(|m| m.sev > Sev::Info) {
            if let Some(h) = member_home(m, &grouped, &live) {
                homes.entry(h).or_default().push(m.clone());
            }
        }
        for (cause, ss) in &grouped {
            let sig_sev = ss.iter().map(|s| s.sev).max().unwrap_or(Sev::Info);
            if sig_sev == Sev::Info {
                continue;
            }
            let sev = sig_sev;
            let mine = homes.remove(cause.as_str()).unwrap_or_default();
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
                    members: vec![],
                    folds: vec![],
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
            inc.members = mine;
            inc.folds = folds.remove(cause).unwrap_or_default();
            inc.title = ex.title;
            inc.why = ex.why;
            inc.next = ex.next;
        }
        let quiet: Vec<String> = self
            .open
            .keys()
            .filter(|k| !live.contains(k.as_str()))
            .cloned()
            .collect();
        for k in quiet {
            let done = {
                let i = self.open.get_mut(&k).expect("key from map");
                i.quiet_cycles += 1;
                i.members.clear();
                i.folds.clear();
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

    // ---- T3.2 cross-sensor causes ----

    fn edge_members(dns: Sev) -> Vec<Member> {
        vec![
            Member::new(
                "bleeding:edge-dead-origins",
                "edge",
                Some("edge"),
                "cloudflared",
                Sev::Warn,
            ),
            Member::new(
                "bleeding:ingress-closed-4001",
                "ingress-lint",
                Some("edge"),
                "cloudflared",
                Sev::Warn,
            ),
            Member::new("bleeding:surface-dns", "surface", Some("edge"), "edge", dns),
            Member::new(
                "vulnerability:public-listener-9999",
                "listeners",
                None,
                "host",
                Sev::Error,
            ),
        ]
    }

    #[test]
    fn dead_origin_ingress_lint_and_dns_are_one_edge_incident() {
        let mut c = Correlator::default();
        let sigs = vec![
            Signal::new(
                "edge",
                "cloudflared",
                Sev::Warn,
                "tunnel routes 3 hostnames to closed ports",
            ),
            Signal::new("edge", "tunnel-quick", Sev::Warn, "degrading"),
        ];
        c.update_with(100, sigs.clone(), &edge_members(Sev::Warn), &ex);
        assert_eq!(c.open.len(), 1, "one incident, not one per sensor");
        let e = &c.open["edge"];
        assert_eq!(e.sev, Sev::Warn);
        assert_eq!(e.sensors(), ["edge", "ingress-lint", "surface"]);
        assert_eq!(
            e.members.len(),
            3,
            "the causeless listener finding stays out"
        );
        assert_eq!(
            e.components.iter().map(String::as_str).collect::<Vec<_>>(),
            ["cloudflared", "tunnel-quick"],
            "members add no components (no extra-component points)"
        );
        let j = e.to_json();
        assert_eq!(j["cause"], "edge", "cause key unchanged");
        assert_eq!(j["correlation"]["rules"], json!(["same-cause"]));
        assert_eq!(
            j["correlation"]["worst_finding"],
            json!({"finding": "bleeding:edge-dead-origins", "severity": "warn"})
        );
        assert_eq!(
            j["correlation"]["findings"][2]["finding"],
            "bleeding:surface-dns"
        );
        // An error DNS failure is the worst member; the incident severity
        // stays with its signals (score v2 applies the member floor).
        c.update_with(130, sigs, &edge_members(Sev::Error), &ex);
        let e = &c.open["edge"];
        assert_eq!(e.sev, Sev::Warn);
        assert_eq!(
            e.worst_member().map(|m| m.key.as_str()),
            Some("bleeding:surface-dns")
        );
        assert_eq!(e.cycles, 2);
    }

    #[test]
    fn findings_alone_never_open_an_incident() {
        let mut c = Correlator::default();
        c.update_with(100, vec![], &edge_members(Sev::Error), &ex);
        assert!(c.open.is_empty());
        // An info-only group stays closed too, members or not.
        c.update_with(
            130,
            vec![Signal::new("edge", "cloudflared", Sev::Info, "fyi")],
            &edge_members(Sev::Error),
            &ex,
        );
        assert!(c.open.is_empty());
    }

    fn ssh_members(bleed: Sev) -> Vec<Member> {
        vec![
            Member::new(
                "ddos:sshd-credential-flood",
                "auth.log",
                Some(SSH_FLOOD),
                "host",
                Sev::Critical,
            ),
            Member::new(
                "bleeding:sshd-auth-log-growth",
                "auth.log",
                Some(SSH_FLOOD),
                "host",
                bleed,
            ),
        ]
    }

    fn disk(msg: &str) -> Signal {
        Signal::new("lxc-host", "lxc-host", Sev::Error, msg)
    }

    #[test]
    fn ssh_flood_and_auth_log_disk_bleed_are_one_incident() {
        let mut c = Correlator::default();
        let sigs = vec![
            Signal::new(SSH_FLOOD, "host", Sev::Critical, "ssh credential flood"),
            disk("disk 91% used; disk full in ~3.0 h at this rate"),
        ];
        c.update_with(100, sigs.clone(), &ssh_members(Sev::Error), &ex);
        assert_eq!(c.open.keys().collect::<Vec<_>>(), ["ssh-flood"]);
        let i = &c.open[SSH_FLOOD];
        assert_eq!(i.sev, Sev::Critical);
        assert_eq!(i.signals.len(), 2);
        assert_eq!(i.members.len(), 2);
        assert_eq!(i.rules(), [RULE_SAME_CAUSE, RULE_SSH_DISK]);
        assert_eq!(
            i.folds,
            [Fold {
                from: "lxc-host".into(),
                component: "lxc-host".into(),
                rule: RULE_SSH_DISK
            }]
        );
        assert_eq!(i.sensors(), ["auth.log"]);
        // Without the auth-log bleed the disk is its own story.
        let mut c = Correlator::default();
        c.update_with(100, sigs.clone(), &ssh_members(Sev::Info), &ex);
        assert_eq!(c.open.len(), 2);
        // RAM trouble on the host is never blamed on the auth log.
        let mut c = Correlator::default();
        let mixed = vec![
            sigs[0].clone(),
            disk("disk 91% used; RAM available 3% (crit)"),
        ];
        c.update_with(100, mixed, &ssh_members(Sev::Error), &ex);
        assert_eq!(c.open.len(), 2);
        // A flood below warn does not pull anything.
        let (g, f) = correlate_with(
            vec![
                Signal::new(SSH_FLOOD, "host", Sev::Info, "x"),
                disk("disk 91% used"),
            ],
            &ssh_members(Sev::Error),
        );
        assert_eq!((g.len(), f.len()), (2, 0));
    }

    #[test]
    fn absorbed_members_follow_their_signals() {
        // A per-hostname edge flood absorbs cloudflared's dead-origin signal;
        // the dead-origin finding goes with it (score v2 covers it there).
        let mut c = Correlator::default();
        let sigs = vec![
            Signal::new("ddos:edge-cloudflared", "cloudflared", Sev::Error, "flood"),
            Signal::new("edge", "cloudflared", Sev::Warn, "dead origins"),
        ];
        let m = vec![Member::new(
            "bleeding:edge-dead-origins",
            "edge",
            Some("edge"),
            "cloudflared",
            Sev::Critical,
        )];
        c.update_with(100, sigs, &m, &ex);
        assert_eq!(c.open.len(), 1);
        let i = &c.open["ddos:edge-cloudflared"];
        assert_eq!(i.members.len(), 1);
        assert_eq!(i.sev, Sev::Error, "severity from signals only");
        assert_eq!(i.worst_member().map(|m| m.sev), Some(Sev::Critical));
        assert_eq!(i.rules(), [RULE_SAME_CAUSE, RULE_DDOS_TARGET]);
        assert_eq!(i.folds[0].from, "edge");
    }

    #[test]
    fn quiet_incident_drops_members_and_stack_wide_ignores_ssh() {
        let mut c = Correlator::default();
        c.update_with(
            100,
            vec![Signal::new("edge", "cloudflared", Sev::Warn, "x")],
            &edge_members(Sev::Warn),
            &ex,
        );
        c.update_with(130, vec![], &edge_members(Sev::Warn), &ex);
        assert!(c.open["edge"].members.is_empty());
        assert_eq!(c.open["edge"].correlation_json()["findings"], json!([]));
        let mk = |c: &str| {
            let mut s = Signal::new(c, c, Sev::Critical, "down");
            s.hard = true;
            s
        };
        let mut flood = Signal::new(SSH_FLOOD, "host", Sev::Critical, "f");
        flood.hard = true;
        let g = correlate(vec![mk("a"), mk("b"), flood]);
        assert!(g.contains_key(SSH_FLOOD) && !g.contains_key("stack-wide"));
    }

    #[test]
    fn disk_only_reasons() {
        assert!(disk_only(&disk("disk 91% used")));
        assert!(disk_only(&disk(
            "degrading: disk 91% used; failing-soon: disk full in ~2.0 h at this rate"
        )));
        assert!(!disk_only(&disk("failing-soon: RAM available 3%")));
        assert!(!disk_only(&disk("other: disk 91% used")));
        assert!(disk_only(&disk(
            "disk 80% used; disk full in ~2.0 h at this rate"
        )));
        assert!(!disk_only(&disk("disk 91% used; RAM available 3% (crit)")));
        assert!(!disk_only(&disk("")));
        assert!(!disk_only(&disk("probe down")));
    }
}
