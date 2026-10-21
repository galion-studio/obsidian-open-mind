// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The cortex: what the nerve's signals mean for the stack as a whole.
//!
//! main.md section 3: the system must know in real time about
//! vulnerabilities, where it is "bleeding", and about DDoS attacks.
//!
//! - **vulnerabilities**: public listeners that should not be public,
//!   unmanaged listeners no monitored unit owns (stray manual processes),
//!   scanner/exploit probes seen in logs, dev servers serving production.
//! - **bleeding**: where requests, memory or uptime leak away: tunnel routes
//!   to dead origin ports, error rates, restart loops, rising memory, host
//!   pressure, a dependency stuck "loading".
//! - **ddos**: the request-rate anomaly verdict per stream ([`crate::access`]).
//!
//! The detectors live in [`crate::sensors`], one file per [`Sensor`]; this
//! module holds the contract they share: the read-only [`Input`] of one
//! cycle, the [`Finding`] they report (B33 `NerveEvent` / B41 kinds and
//! severities, see [`Finding::to_nerve_event`]) and the assembled [`Cortex`].
//!
//! Each finding says what was seen (evidence), why it matters and the next
//! step. The nerve never acts on cortex findings by itself: every fix here
//! touches config, DNS or units and needs the owner.
//!
//! [`Sensor`]: crate::sensors::Sensor

use crate::access::DdosVerdict;
use crate::incident::Correlator;
use crate::incident::{Sev, Signal};
use crate::logscan::LogInsights;
use crate::owners::{OwnerMap, ProcInfo};
use crate::probe::{JLine, Listener};
use crate::state::{CompState, Status};
use crate::util::ts;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// B41 finding kind (docs/cortex.md section 2): which of the three
/// main.md section 3 questions a finding answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingKind {
    /// Something an attacker could use (exposure, probes, weak posture).
    Vulnerability,
    /// Requests, memory, uptime, disk or money leaking away.
    Bleeding,
    /// Request or credential flood.
    Ddos,
}

impl FindingKind {
    /// The B33 `NerveEvent.kind` string: `vulnerability`, `bleeding`, `ddos`.
    pub fn as_str(self) -> &'static str {
        match self {
            FindingKind::Vulnerability => "vulnerability",
            FindingKind::Bleeding => "bleeding",
            FindingKind::Ddos => "ddos",
        }
    }

    /// Inverse of [`Self::as_str`] (for pushed `NerveEvent`s).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "vulnerability" => FindingKind::Vulnerability,
            "bleeding" => FindingKind::Bleeding,
            "ddos" => FindingKind::Ddos,
            _ => return None,
        })
    }
}

/// One cortex finding: what was seen, why it matters, the next step.
///
/// Build one with [`Finding::new`] and the `with_*` methods so new fields
/// can be added without breaking sensors.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Stable id, unique per cycle within a kind (`public-listener-8652`).
    pub id: String,
    pub kind: FindingKind,
    pub sev: Sev,
    /// The component (spec name) or `host`.
    pub component: String,
    /// One line for a human; counts, not raw log lines.
    pub title: String,
    /// Short supporting lines; stay on the loopback API, never in events.
    pub evidence: Vec<String>,
    /// The next step (always owner-approved when it changes anything).
    pub fix: String,
    /// Incident cause this finding is part of (scored there, not twice).
    pub cause: Option<String>,
    /// The B33 `source`: the sensor that saw it ([`crate::sensors::Sensor::name`]
    /// unless the sensor names its own source, e.g. `auth.log`).
    pub source: String,
}

impl Finding {
    /// A finding with no evidence, fix, cause or source yet. The source is
    /// stamped by [`crate::sensors::Sink::finding`] when left empty.
    pub fn new(
        kind: FindingKind,
        id: impl Into<String>,
        sev: Sev,
        component: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Finding {
            id: id.into(),
            kind,
            sev,
            component: component.into(),
            title: title.into(),
            evidence: vec![],
            fix: String::new(),
            cause: None,
            source: String::new(),
        }
    }

    pub fn with_evidence(mut self, evidence: Vec<String>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = fix.into();
        self
    }

    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Loopback API shape (`/api/v1/cortex`, self-eval report).
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "kind": self.kind.as_str(), "source": self.source,
               "severity": self.sev.as_str(), "component": self.component, "title": self.title,
               "evidence": self.evidence, "fix": self.fix, "incident_cause": self.cause})
    }

    /// The finding as one B33 `NerveEvent` (`openmind_schema::NerveEvent`
    /// field for field): `ts` Europe/Zagreb RFC 3339 of `now`, `source`,
    /// `kind` (B41), `severity` (B41 via [`Sev::b41`]), `data`.
    ///
    /// `data` holds the id, component, title, fix, cause and the evidence
    /// *count*; evidence lines are left out (B41: no raw lines or addresses).
    pub fn to_nerve_event(&self, now: u64) -> Value {
        json!({"ts": ts(now), "source": self.source_or_cortex(), "kind": self.kind.as_str(),
               "severity": self.sev.b41(),
               "data": {"id": self.id, "component": self.component, "title": self.title, "fix": self.fix,
                        "cause": self.cause, "evidence_n": self.evidence.len()}})
    }

    fn source_or_cortex(&self) -> &str {
        if self.source.is_empty() {
            "cortex"
        } else {
            &self.source
        }
    }
}

/// Everything a sensor may read in one cycle. Read-only, borrowed from the
/// engine (live) or from a capture (replay).
///
/// Construct with [`Input::new`] and set what you have; fields may be added
/// (always with a neutral default in `new`).
pub struct Input<'a> {
    /// Cycle clock (unix seconds): live now, or the capture time in replay.
    pub now: u64,
    pub comps: &'a [CompState],
    /// Sliding journal window insights (dead origins, scanners, dev servers).
    pub logs: &'a LogInsights,
    /// Listening sockets (None: not read this cycle).
    pub listeners: Option<&'a [Listener]>,
    pub ddos: &'a [DdosVerdict],
    pub known_ports: &'a [u16],
    pub public_ok_ports: &'a [u16],
    /// Only listeners in this inclusive range are judged (None: all).
    pub listen_scope: Option<(u16, u16)>,
    pub own_port: u16,
    pub loading_grace_s: u64,
    /// Journal lines new in this cycle (all collected units), for sensors
    /// that count their own patterns (sshd, cloudflared `dest=`, hades-gate).
    pub journal: &'a [JLine],
    /// True in a live cycle. False in replay and tests: a sensor that reads
    /// the host itself (files, commands, /proc) must do nothing when false,
    /// so replays stay deterministic and never read the box.
    pub live: bool,
    /// Port ownership map ([`crate::owners`]).
    pub owners: &'a OwnerMap,
    /// Holders of unowned listeners (from /proc or `ss -ltnp`).
    pub procs: &'a BTreeMap<u16, ProcInfo>,
}

impl<'a> Input<'a> {
    /// Neutral input: no listeners, no streams, no ports, no journal,
    /// `loading_grace_s` 900, not live.
    pub fn new(now: u64, comps: &'a [CompState], logs: &'a LogInsights) -> Self {
        Input {
            now,
            comps,
            logs,
            listeners: None,
            ddos: &[],
            known_ports: &[],
            public_ok_ports: &[],
            listen_scope: None,
            own_port: 0,
            loading_grace_s: 900,
            journal: &[],
            live: false,
            owners: &crate::owners::EMPTY,
            procs: &crate::owners::NO_PROCS,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Cortex {
    pub vulnerabilities: Vec<Finding>,
    pub bleeding: Vec<Finding>,
    /// Per-stream rate verdicts ([`DdosVerdict::to_json`]).
    pub ddos: Vec<Value>,
    pub ddos_firing: bool,
    /// Findings of kind [`FindingKind::Ddos`] (credential floods and the
    /// like). Scored on their own unless an open incident (a [`Signal`] with
    /// the same cause) already scores them; see [`crate::score`].
    pub ddos_findings: Vec<Finding>,
    pub signals: Vec<Signal>,
    pub exposure: Value,
    /// Open incidents with their owning Gaia function(s) ([`Cortex::attach_owners`]).
    pub incident_owners: Vec<Value>,
    /// component -> Gaia function, for findings.
    pub gaia_of: BTreeMap<String, String>,
}

impl Cortex {
    pub fn to_json(&self) -> Value {
        let mut ddos = json!({"firing": self.ddos_firing, "streams": self.ddos});
        if !self.ddos_findings.is_empty() {
            ddos["findings"] = self
                .ddos_findings
                .iter()
                .map(|f| self.finding_json(f))
                .collect::<Vec<_>>()
                .into();
        }
        json!({
            "vulnerabilities": self.vulnerabilities.iter().map(|f| self.finding_json(f)).collect::<Vec<_>>(),
            "bleeding": self.bleeding.iter().map(|f| self.finding_json(f)).collect::<Vec<_>>(),
            "incidents": self.incident_owners,
            "ddos": ddos,
            "exposure": self.exposure,
        })
    }

    /// All findings, vulnerabilities then bleeding then ddos.
    pub fn findings(&self) -> impl Iterator<Item = &Finding> {
        self.vulnerabilities
            .iter()
            .chain(&self.bleeding)
            .chain(&self.ddos_findings)
    }

    /// B33 `NerveEvent`s for this cycle with the B41 rule "the highest
    /// severity wins inside one `(source, kind)` pair": one event per pair,
    /// `data` = `{findings, ids, top}` (`top` is the worst finding's data).
    /// Info-only pairs are skipped (B41 has no info level).
    pub fn nerve_events(&self, now: u64) -> Vec<Value> {
        let mut by: BTreeMap<(&str, FindingKind), Vec<&Finding>> = BTreeMap::new();
        for f in self.findings() {
            by.entry((f.source_or_cortex(), f.kind))
                .or_default()
                .push(f);
        }
        let mut out = vec![];
        for ((source, kind), fs) in by {
            let Some(top) = fs
                .iter()
                .copied()
                .max_by(|a, b| a.sev.cmp(&b.sev).then(b.id.cmp(&a.id)))
            else {
                continue;
            };
            if top.sev == Sev::Info {
                continue;
            }
            let ev = top.to_nerve_event(now);
            out.push(json!({"ts": ev["ts"], "source": source, "kind": kind.as_str(), "severity": ev["severity"],
                            "data": {"findings": fs.len(), "ids": fs.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
                                     "top": ev["data"]}}));
        }
        out
    }

    fn finding_json(&self, f: &Finding) -> Value {
        let mut v = f.to_json();
        v["gaia"] = json!(self.gaia_of.get(&f.component));
        v
    }

    /// After correlation: name the owning Gaia function(s) of every open
    /// incident. A component maps to its spec's function; signals from
    /// components the table does not know (the host, an unknown pushed
    /// source) are listed as "unknown". Also tags each incident itself.
    pub fn attach_owners(&mut self, inc: &mut Correlator, comps: &[CompState]) {
        self.gaia_of = comps
            .iter()
            .map(|c| (c.spec.name.to_string(), c.spec.gaia.to_string()))
            .collect();
        let sys: BTreeMap<&str, &str> = comps
            .iter()
            .map(|c| (c.spec.name, c.spec.gaia_system))
            .collect();
        for i in inc.open.values_mut() {
            i.owners = i
                .components
                .iter()
                .map(|c| {
                    (
                        c.clone(),
                        self.gaia_of
                            .get(c)
                            .cloned()
                            .unwrap_or_else(|| "unknown".into()),
                    )
                })
                .collect();
        }
        self.incident_owners = inc
            .open_sorted()
            .iter()
            .map(|i| {
                let fns: BTreeSet<&String> = i.owners.values().collect();
                json!({
                    "id": i.id, "cause": i.cause, "title": i.title, "severity": i.sev.as_str(),
                    "gaia": fns,
                    "owners": i.owners.iter().map(|(c, g)| json!({
                        "component": c, "gaia": g, "system": sys.get(c.as_str()),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
    }
}

/// Human duration: `45 s`, `12 min`, `3.5 h`, `1.8 d`.
pub fn dur(s: u64) -> String {
    if s >= 86_400 {
        format!("{:.1} d", s as f64 / 86_400.0)
    } else if s >= 3600 {
        format!("{:.1} h", s as f64 / 3600.0)
    } else if s >= 60 {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

/// Seconds a non-healthy component has been in its state, if it looks stuck
/// "loading" (503 with a loading body) for longer than `grace`.
pub fn stuck_loading(c: &CompState, now: u64, grace: u64) -> Option<u64> {
    let s = c.latest()?;
    let loading = s.http_code == Some(503)
        && (s.extra.get("mouth").is_some()
            || s.extra
                .get("health_body")
                .and_then(|b| b.get("status"))
                .and_then(Value::as_str)
                == Some("loading"));
    let age = now.saturating_sub(c.status_since);
    (loading && c.status != Status::Healthy && c.status_since > 0 && age > grace).then_some(age)
}

/// Severity of a component's verdict as an incident signal.
pub fn comp_sev(c: &CompState, now: u64, grace: u64) -> Option<Sev> {
    let base = match c.status {
        Status::Healthy | Status::Absent => return None,
        Status::Failing if c.spec.critical => Sev::Critical,
        Status::Failing | Status::FailingSoon => Sev::Error,
        Status::Degrading => Sev::Warn,
    };
    Some(if stuck_loading(c, now, grace).is_some() {
        base.max(Sev::Error)
    } else {
        base
    })
}

/// Component verdicts as incident signals (cause = the component's group).
pub fn component_signals(comps: &[CompState], now: u64, grace: u64) -> Vec<Signal> {
    let mut v = vec![];
    for c in comps {
        let Some(sev) = comp_sev(c, now, grace) else {
            continue;
        };
        let mut msg = if c.reasons.is_empty() {
            c.status.as_str().to_string()
        } else {
            c.reasons.join("; ")
        };
        if let Some(age) = stuck_loading(c, now, grace) {
            msg.push_str(&format!(" -- stuck loading for {}", dur(age)));
        }
        if let Some(b) = c.latest().and_then(|s| s.extra.get("health_body")) {
            msg.push_str(&format!(" [health body {b}]"));
        }
        let mut s = Signal::new(c.spec.group, c.spec.name, sev, msg);
        s.hard = c.hard_down;
        v.push(s);
    }
    v
}

/// Run the built-in sensors once over `i` (stateless convenience; the
/// engine keeps a [`crate::sensors::Sensors`] registry across cycles).
pub fn analyse(i: &Input) -> Cortex {
    crate::sensors::Sensors::builtin().run(i)
}

/// Component name of a systemd unit (the unit name without `.service`
/// when no spec owns it).
pub fn comp_of_unit(comps: &[CompState], unit: &str) -> String {
    comps
        .iter()
        .find(|c| c.spec.unit == Some(unit))
        .map(|c| c.spec.name.to_string())
        .unwrap_or_else(|| unit.trim_end_matches(".service").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_specs;
    use crate::probe::JLine;
    use crate::state::{Sample, Thresholds};

    fn comps() -> Vec<CompState> {
        default_specs(None, None)
            .into_iter()
            .map(CompState::new)
            .collect()
    }

    #[test]
    fn vps_like_picture() {
        let th = Thresholds::default();
        let mut cs = comps();
        let now = 1_792_520_000;
        for c in cs.iter_mut() {
            let mut s = Sample {
                t: now,
                present: true,
                extra: json!({}),
                ..Default::default()
            };
            s.unit_active = Some("active".into());
            s.http_code = c.spec.port.map(|_| 200);
            if c.spec.name == "galion-one-mouth" {
                s.http_code = Some(503);
                s.extra = json!({"mouth": "model loading", "health_body": {"status": "loading", "model": "chatterbox", "loaded": false}});
                c.status = Status::Degrading;
                c.status_since = now - 44 * 3600;
            }
            if c.spec.name == "cloudflared" {
                s.tunnel_conns = Some(4);
                s.tunnel_requests = Some(81948);
                s.tunnel_req_errors = Some(48591);
            }
            c.push(s, &th);
        }
        let lines: Vec<JLine> = (0..60u64).map(|k| JLine { t: now - 1800 + k * 30, unit: "cloudflared.service".into(), prio: 6,
            msg: format!("Z ERR Request failed error=\"dial tcp [::1]:{}: connect: connection refused\" dest=https://{}/ ip=1.1.1.1", if k % 2 == 0 { 4001 } else { 3011 }, if k % 2 == 0 { "galion.studio" } else { "merlinelement.com" }) }).chain([ // repo-check: allow (test fixture address, never dialled)
            JLine { t: now - 60, unit: "console.service".into(), prio: 6, msg: "The request id \"/x/.git/config\" is outside of Vite serving allow list.".into() }]).collect();
        let li = crate::logscan::insights(&lines);
        let ss_text = "LISTEN 0 1 127.0.0.1:8652 0.0.0.0:* users:((\"hermes-gateway\",pid=1395619,fd=4))\nLISTEN 0 1 127.0.0.1:8431 0.0.0.0:* users:((\"openmind-memory\",pid=1375169,fd=6))\nLISTEN 0 1 127.0.0.1:8420 0.0.0.0:*\nLISTEN 0 1 *:22 *:*\nLISTEN 0 1 127.0.0.1:20241 0.0.0.0:*\n";
        let ls = crate::probe::parse_ss(ss_text);
        let procs = crate::owners::parse_ss_procs(ss_text);
        let om = OwnerMap::build(
            &default_specs(None, None),
            &[20241],
            &[],
            &crate::owners::VPS_UNOWNED_PORTS,
        );
        let cx = analyse(&Input {
            now,
            comps: &cs,
            logs: &li,
            listeners: Some(&ls),
            ddos: &[],
            known_ports: &[20241],
            public_ok_ports: &[22],
            listen_scope: None,
            own_port: 8670,
            loading_grace_s: 900,
            journal: &[],
            live: false,
            owners: &om,
            procs: &procs,
        });
        let ids: Vec<&str> = cx.vulnerabilities.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"unmanaged-listeners"), "{ids:?}");
        assert!(cx.vulnerabilities.iter().any(|f| f.title.contains(":8652")));
        let um = cx
            .vulnerabilities
            .iter()
            .find(|f| f.id == "unmanaged-listeners")
            .unwrap();
        assert!(um.title.contains(":8431 :8652"), "{}", um.title);
        assert!(
            um.evidence
                .iter()
                .any(|e| e.starts_with(":8652 unknown owner")
                    && e.contains("hermes-gateway (pid 1395619)")),
            "{:?}",
            um.evidence
        );
        let own = cx.exposure["ownership"].as_array().unwrap();
        let o8652 = own.iter().find(|o| o["port"] == 8652).unwrap();
        assert_eq!(
            (o8652["owner"].as_str(), o8652["process"].as_str()),
            (Some("unknown owner"), Some("hermes-gateway"))
        );
        let o8420 = own.iter().find(|o| o["port"] == 8420).unwrap();
        assert_eq!(
            (o8420["owner"].as_str(), o8420["gaia"].as_str()),
            (Some("openmind-memory"), Some("APOLLO"))
        );
        assert!(ids.contains(&"scanner-probes-console"), "{ids:?}");
        assert_eq!(
            cx.vulnerabilities
                .iter()
                .find(|f| f.id == "scanner-probes-console")
                .unwrap()
                .sev,
            Sev::Error
        );
        let edge = cx
            .bleeding
            .iter()
            .find(|f| f.id == "edge-dead-origins")
            .unwrap();
        assert_eq!(edge.sev, Sev::Error, "{}", edge.title);
        assert!(edge.title.contains(":3011 :4001"), "{}", edge.title);
        assert!(edge.evidence.iter().any(|e| e.contains("48591 of 81948")));
        assert!(edge
            .evidence
            .iter()
            .any(|e| e.contains("none of these ports has a listener")));
        let stuck = cx
            .bleeding
            .iter()
            .find(|f| f.id == "stuck-loading-galion-one-mouth")
            .unwrap();
        assert!(stuck.title.contains("1.8 d"), "{}", stuck.title);
        let sig = component_signals(&cs, now, 900);
        let m = sig
            .iter()
            .find(|s| s.component == "galion-one-mouth")
            .unwrap();
        assert_eq!((m.sev, m.cause.as_str()), (Sev::Error, "galion-one"));
        assert!(cx.signals.iter().any(|s| s.cause == "edge"));
        assert!(!cx.ddos_firing);
    }
}
