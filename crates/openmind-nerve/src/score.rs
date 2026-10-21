// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Self-eval scoring (score v2, cortex plan T3.3): one number, every point
//! explained.
//!
//! Start at 100 and deduct. Incidents are scored once per incident (that is
//! what correlation is for: five signals of one cause cost one deduction,
//! not five). Cortex findings are scored only when no open incident already
//! stands for them, and then once per `cause`.
//!
//! | what | points |
//! |---|---|
//! | open incident, critical | 25 (+3 per extra component, max +9) |
//! | open incident, error | 15 (+2 per extra component, max +6) |
//! | open incident, warn | 5 |
//! | ddos critical / error / warn (not in an incident) | 15 / 10 / 4 |
//! | vulnerability error / warn (not in an incident) | 10 / 4 |
//! | bleeding error / warn (not in an incident) | 8 / 3 |
//! | probe error (nerve blind spot) | 5 each, max 15 |
//! | nerve self-check warn (RSS, cycle time) | 3 each |
//!
//! # No double counting (v2)
//!
//! A finding is **covered** (costs nothing here) when an open incident
//! already scores it:
//!
//! 1. the incident's cause equals the finding's `cause` (the sensor sent a
//!    [`crate::incident::Signal`] with the same cause, e.g. `ssh-flood`), or
//! 2. the incident is an absorbing one, `ddos:<stream>` or `stack-wide`,
//!    and lists the finding's component: correlation pulled that
//!    component's signals into it, so its findings belong there too.
//!
//! **Member floor (T3.2).** The correlator lists the findings an incident
//! covers as its members ([`crate::incident::Member`], `correlation` in the
//! incident JSON). An incident costs at least what its worst member would
//! cost uncovered, and a member that would block `pass` alone still blocks
//! it: a scanner finding at error under a warn scanner signal costs 10, not
//! 5. Without members (an incident built by [`crate::incident::Correlator::update`])
//! nothing changes.
//!
//! Uncovered findings that share a `cause` (an ssh credential flood plus
//! the auth-log disk bleed it causes, both `ssh-flood`) are one deduction:
//! the worst finding's points, the others named in `why`. Findings without
//! a cause are scored one by one, as in v1.
//!
//! DDoS-kind findings ([`crate::cortex::FindingKind::Ddos`]: credential
//! floods, per-hostname edge rate, pushed Gaia ddos events) score on their
//! own when no signal turned them into an incident; with a signal they are
//! scored once, as the incident. The per-stream request-rate verdicts
//! (`ddos.streams`) are never scored directly: the `ddos` sensor turns
//! each `flood` / `elevated` into a `ddos:<stream>` signal and so an
//! incident.
//!
//! Grade: A >= 90, B >= 80, C >= 70, D >= 55, F below.
//! `pass` = no critical/error incident, no error-level vulnerability or ddos
//! finding (uncovered, or a member of an incident), no probe error, and
//! score >= 70.

use crate::cortex::{Cortex, Finding, FindingKind};
use crate::incident::{Incident, Sev};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Deduction {
    pub points: u32,
    pub what: String,
    pub why: String,
}

/// A finding that cost nothing because an open incident already scores it.
#[derive(Debug, Clone, PartialEq)]
pub struct Covered {
    /// `<kind>:<finding id>`.
    pub finding: String,
    /// The cause of the incident that scores it.
    pub by: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub score: u32,
    pub grade: &'static str,
    pub pass: bool,
    pub verdict: String,
    pub deductions: Vec<Deduction>,
    /// Findings scored through their incident (v2 no-double-count trail).
    pub covered: Vec<Covered>,
}

impl Score {
    pub fn to_json(&self) -> Value {
        json!({"score": self.score, "grade": self.grade, "pass": self.pass, "verdict": self.verdict,
               "deductions": self.deductions.iter().map(|d| json!({"points": d.points, "what": d.what, "why": d.why})).collect::<Vec<_>>(),
               "covered": self.covered.iter().map(|c| json!({"finding": c.finding, "by": c.by})).collect::<Vec<_>>(),
               "method": "score v2: 100 minus deductions; incidents scored once per correlated cause; findings only when no open incident scores them, once per cause; see score.rs"})
    }
}

pub fn grade(s: u32) -> &'static str {
    match s {
        90.. => "A",
        80..=89 => "B",
        70..=79 => "C",
        55..=69 => "D",
        _ => "F",
    }
}

/// Points for one finding that no incident covers (table in the module doc).
pub fn finding_points(kind: FindingKind, sev: Sev) -> u32 {
    match (kind, sev) {
        (_, Sev::Info) => 0,
        (FindingKind::Ddos, Sev::Critical) => 15,
        (FindingKind::Ddos, Sev::Error) => 10,
        (FindingKind::Ddos, Sev::Warn) => 4,
        (FindingKind::Vulnerability, Sev::Critical | Sev::Error) => 10,
        (FindingKind::Vulnerability, Sev::Warn) => 4,
        (FindingKind::Bleeding, Sev::Critical | Sev::Error) => 8,
        (FindingKind::Bleeding, Sev::Warn) => 3,
    }
}

/// Does an uncovered finding of this kind and severity block `pass`?
/// Error-level vulnerabilities (as in v1) and error-level DDoS findings
/// (an active flood no incident tracks); bleeding only costs points.
pub fn finding_blocks(kind: FindingKind, sev: Sev) -> bool {
    sev >= Sev::Error && matches!(kind, FindingKind::Vulnerability | FindingKind::Ddos)
}

/// The worst member of an incident (T3.2) as an uncovered finding would be
/// scored: its points and whether it would block `pass`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floor<'a> {
    pub key: &'a str,
    pub sev: &'static str,
    pub points: u32,
    pub blocks: bool,
}

/// [`Floor`] of `i`'s member findings (None: no member, or none costs points).
pub fn member_floor(i: &Incident) -> Option<Floor<'_>> {
    i.members
        .iter()
        .filter_map(|m| {
            let kind = FindingKind::parse(m.key.split(':').next().unwrap_or(""))?;
            Some(Floor {
                key: &m.key,
                sev: m.sev.as_str(),
                points: finding_points(kind, m.sev),
                blocks: finding_blocks(kind, m.sev),
            })
        })
        .filter(|f| f.points > 0)
        .reduce(|a, b| {
            if (b.points, b.blocks) > (a.points, a.blocks) {
                b
            } else {
                a
            }
        })
}

/// The open incident that already scores `f`, if any (rules in the module doc).
/// An incident that lists `f` as a T3.2 correlation member
/// ([`crate::incident::Member`]) wins; the cause / absorbing rules below are
/// the same ones the correlator used to place it.
pub fn covering<'a>(f: &Finding, incidents: &[&'a Incident]) -> Option<&'a Incident> {
    let key = format!("{}:{}", f.kind.as_str(), f.id);
    if let Some(i) = incidents
        .iter()
        .find(|i| i.members.iter().any(|m| m.key == key))
    {
        return Some(i);
    }
    if let Some(c) = &f.cause {
        if let Some(i) = incidents.iter().find(|i| &i.cause == c) {
            return Some(i);
        }
    }
    incidents
        .iter()
        .find(|i| crate::incident::absorbing(&i.cause) && i.components.contains(&f.component))
        .copied()
}

pub fn score(
    incidents: &[&Incident],
    cx: &Cortex,
    probe_errors: &[String],
    self_warns: &[String],
) -> Score {
    let mut d: Vec<Deduction> = vec![];
    let mut blocking_member = false;
    for i in incidents {
        let extra = i.components.len().saturating_sub(1) as u32;
        let (base, per, cap) = match i.sev {
            Sev::Critical => (25, 3, 9),
            Sev::Error => (15, 2, 6),
            Sev::Warn => (5, 0, 0),
            Sev::Info => continue,
        };
        let points = base + (extra * per).min(cap);
        // T3.2 member floor: never less than its worst member costs alone.
        let floor = member_floor(i);
        blocking_member |= floor.is_some_and(|f| f.blocks);
        let mut why = format!(
            "{} signal(s) from {} correlated into one cause `{}`; open since {}{}",
            i.signals.len(),
            i.components.iter().cloned().collect::<Vec<_>>().join(", "),
            i.cause,
            crate::util::ts(i.opened),
            if i.why.is_empty() {
                String::new()
            } else {
                format!(". {}", i.why)
            }
        );
        let points = match floor {
            Some(f) if f.points > points => {
                why.push_str(&format!(
                    "; member floor: {} ({}) costs {} alone, so the incident costs {} not {points}",
                    f.key, f.sev, f.points, f.points
                ));
                f.points
            }
            _ => points,
        };
        d.push(Deduction {
            points,
            what: format!("{} incident: {}", i.sev.as_str(), i.title),
            why,
        });
    }
    // Uncovered findings, grouped by cause in first-seen order
    // (vulnerabilities, bleeding, ddos); no cause = a group of one.
    let mut covered: Vec<Covered> = vec![];
    let mut groups: Vec<(Option<&str>, Vec<&Finding>)> = vec![];
    let mut blocking_finding = false;
    for f in cx.findings() {
        if let Some(i) = covering(f, incidents) {
            if f.sev > Sev::Info {
                covered.push(Covered {
                    finding: format!("{}:{}", f.kind.as_str(), f.id),
                    by: i.cause.clone(),
                });
            }
            continue;
        }
        if finding_points(f.kind, f.sev) == 0 {
            continue;
        }
        blocking_finding |= finding_blocks(f.kind, f.sev);
        match f
            .cause
            .as_deref()
            .and_then(|c| groups.iter_mut().find(|g| g.0 == Some(c)))
        {
            Some(g) => g.1.push(f),
            None => groups.push((f.cause.as_deref(), vec![f])),
        }
    }
    for (cause, fs) in &groups {
        let worst = fs
            .iter()
            .copied()
            .reduce(|a, b| {
                if finding_points(b.kind, b.sev) > finding_points(a.kind, a.sev) {
                    b
                } else {
                    a
                }
            })
            .expect("groups are never empty");
        let mut why = worst.evidence.join("; ");
        if fs.len() > 1 {
            let others: Vec<String> = fs
                .iter()
                .filter(|f| !std::ptr::eq(**f, worst))
                .map(|f| format!("{} {} ({})", f.kind.as_str(), f.id, f.sev.as_str()))
                .collect();
            if !why.is_empty() {
                why.push_str("; ");
            }
            why.push_str(&format!(
                "same cause `{}`, scored once: also {}",
                cause.unwrap_or_default(),
                others.join(", ")
            ));
        }
        d.push(Deduction {
            points: finding_points(worst.kind, worst.sev),
            what: format!(
                "{} ({}): {}",
                worst.kind.as_str(),
                worst.sev.as_str(),
                worst.title
            ),
            why,
        });
    }
    let pe = (probe_errors.len() as u32 * 5).min(15);
    if pe > 0 {
        d.push(Deduction {
            points: pe,
            what: format!(
                "{} probe error(s): the nerve is partly blind",
                probe_errors.len()
            ),
            why: probe_errors.join("; "),
        });
    }
    for w in self_warns {
        d.push(Deduction {
            points: 3,
            what: format!("nerve self-check: {w}"),
            why: "the hub itself must stay CPU-light and fast".into(),
        });
    }
    d.sort_by_key(|x| std::cmp::Reverse(x.points));
    let total: u32 = d.iter().map(|x| x.points).sum();
    let s = 100u32.saturating_sub(total);
    let blocking = incidents.iter().any(|i| i.sev >= Sev::Error)
        || blocking_finding
        || blocking_member
        || !probe_errors.is_empty();
    let pass = !blocking && s >= 70;
    let crit = incidents.iter().filter(|i| i.sev == Sev::Critical).count();
    let err = incidents.iter().filter(|i| i.sev == Sev::Error).count();
    let verdict = if incidents.is_empty() && d.is_empty() {
        "healthy: no open incidents, no cortex findings".to_string()
    } else {
        let state = if crit > 0 {
            "failing"
        } else if err > 0 || blocking_finding || blocking_member {
            "degraded"
        } else {
            "degrading"
        };
        let mut parts: Vec<String> = incidents
            .iter()
            .take(4)
            .map(|i| format!("[{}] {}", i.sev.as_str(), i.title))
            .collect();
        if incidents.len() > 4 {
            parts.push(format!("+{} more", incidents.len() - 4));
        }
        let ddos_n = cx
            .ddos_findings
            .iter()
            .filter(|v| v.sev > Sev::Info)
            .count();
        format!(
            "{state}: {} open incident(s) ({crit} critical, {err} error), {} vulnerability finding(s), {} bleeding point(s){}. {}",
            incidents.len(),
            cx.vulnerabilities.iter().filter(|v| v.sev > Sev::Info).count(),
            cx.bleeding.iter().filter(|v| v.sev > Sev::Info).count(),
            if ddos_n > 0 { format!(", {ddos_n} ddos finding(s)") } else { String::new() },
            parts.join(" | ")
        )
    };
    Score {
        score: s,
        grade: grade(s),
        pass,
        verdict,
        deductions: d,
        covered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortex::Input;
    use crate::incident::{Correlator, Explain, Signal};
    use crate::sensors::{Sensor, Sensors, Sink};
    use std::collections::BTreeSet;

    fn inc(cause: &str, sev: Sev, comps: &[&str]) -> Incident {
        Incident {
            id: "i".into(),
            cause: cause.into(),
            title: format!("{cause} t"),
            sev,
            opened: 1_792_520_000,
            last_seen: 0,
            cycles: 1,
            quiet_cycles: 0,
            resolved: None,
            components: comps.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
            signals: vec![],
            why: String::new(),
            next: String::new(),
            owners: Default::default(),
            members: vec![],
            folds: vec![],
        }
    }

    #[test]
    fn healthy_is_100_a_pass() {
        let s = score(&[], &Cortex::default(), &[], &[]);
        assert_eq!((s.score, s.grade, s.pass), (100, "A", true));
    }

    #[test]
    fn incidents_scored_once_and_fail() {
        let a = inc("edge", Sev::Error, &["cloudflared"]);
        let b = inc("galion-one", Sev::Error, &["galion-one-mouth"]);
        let mut cx = Cortex::default();
        cx.bleeding.push(
            Finding::new(
                FindingKind::Bleeding,
                "edge-dead-origins",
                Sev::Error,
                "cloudflared",
                "t",
            )
            .with_cause("edge"),
        );
        cx.vulnerabilities.push(Finding::new(
            FindingKind::Vulnerability,
            "unmanaged-listeners",
            Sev::Warn,
            "host",
            "5 stray",
        ));
        let s = score(&[&a, &b], &cx, &[], &[]);
        assert_eq!(s.score, 100 - 15 - 15 - 4, "{:?}", s.deductions);
        assert_eq!(s.grade, "D");
        assert!(!s.pass);
        assert!(
            s.verdict.starts_with("degraded: 2 open incident(s)"),
            "{}",
            s.verdict
        );
        let w = inc("x", Sev::Warn, &["x"]);
        let s = score(&[&w], &Cortex::default(), &[], &[]);
        assert_eq!((s.score, s.pass), (95, true));
        let c = inc("stack-wide", Sev::Critical, &["a", "b", "c", "d", "e"]);
        assert_eq!(
            score(&[&c], &Cortex::default(), &[], &[]).score,
            100 - 25 - 9
        );
    }

    fn vuln(id: &str, sev: Sev, comp: &str) -> Finding {
        Finding::new(FindingKind::Vulnerability, id, sev, comp, format!("{id} t"))
    }

    fn ddos(id: &str, sev: Sev, comp: &str) -> Finding {
        Finding::new(FindingKind::Ddos, id, sev, comp, format!("{id} t"))
            .with_evidence(vec![format!("{id} evidence")])
    }

    #[test]
    fn ddos_finding_without_incident_scores_on_its_own_and_blocks() {
        let mut cx = Cortex::default();
        cx.ddos_findings
            .push(ddos("sshd-credential-flood", Sev::Critical, "host").with_cause("ssh-flood"));
        let s = score(&[], &cx, &[], &[]);
        assert_eq!((s.score, s.grade), (85, "B"), "{:?}", s.deductions);
        assert!(!s.pass, "an untracked critical flood blocks pass");
        assert_eq!(
            s.deductions[0].what,
            "ddos (critical): sshd-credential-flood t"
        );
        assert_eq!(s.deductions[0].why, "sshd-credential-flood evidence");
        assert!(
            s.verdict
                .starts_with("degraded: 0 open incident(s) (0 critical, 0 error), 0 vulnerability finding(s), 0 bleeding point(s), 1 ddos finding(s)."),
            "{}",
            s.verdict
        );
        for (sev, pts) in [(Sev::Error, 10), (Sev::Warn, 4), (Sev::Info, 0)] {
            let mut cx = Cortex::default();
            cx.ddos_findings.push(ddos("ev-gaia-x", sev, "gaia"));
            let s = score(&[], &cx, &[], &[]);
            assert_eq!(s.score, 100 - pts, "{sev:?}");
            assert_eq!(s.pass, sev < Sev::Error, "{sev:?}");
        }
    }

    #[test]
    fn ddos_finding_with_its_signal_is_scored_once_as_the_incident() {
        let i = inc("ssh-flood", Sev::Critical, &["host"]);
        let mut cx = Cortex::default();
        cx.ddos_findings
            .push(ddos("sshd-credential-flood", Sev::Critical, "host").with_cause("ssh-flood"));
        cx.bleeding.push(
            Finding::new(
                FindingKind::Bleeding,
                "sshd-auth-log-growth",
                Sev::Warn,
                "host",
                "auth log grew",
            )
            .with_cause("ssh-flood"),
        );
        let s = score(&[&i], &cx, &[], &[]);
        assert_eq!(s.score, 75, "{:?}", s.deductions);
        assert_eq!(s.deductions.len(), 1);
        assert_eq!(
            s.covered,
            [
                Covered {
                    finding: "bleeding:sshd-auth-log-growth".into(),
                    by: "ssh-flood".into()
                },
                Covered {
                    finding: "ddos:sshd-credential-flood".into(),
                    by: "ssh-flood".into()
                }
            ]
        );
        assert_eq!(s.to_json()["covered"][1]["by"], "ssh-flood");
    }

    #[test]
    fn uncovered_findings_sharing_a_cause_cost_one_deduction() {
        let mut cx = Cortex::default();
        cx.bleeding.push(
            Finding::new(
                FindingKind::Bleeding,
                "sshd-auth-log-growth",
                Sev::Error,
                "host",
                "auth log grew",
            )
            .with_cause("ssh-flood"),
        );
        cx.ddos_findings
            .push(ddos("sshd-credential-flood", Sev::Error, "host").with_cause("ssh-flood"));
        // Two causeless warns stay two deductions (v1 behaviour).
        for id in ["a", "b"] {
            cx.vulnerabilities.push(Finding::new(
                FindingKind::Vulnerability,
                id,
                Sev::Warn,
                "host",
                id,
            ));
        }
        let s = score(&[], &cx, &[], &[]);
        assert_eq!(s.score, 100 - 10 - 4 - 4, "{:?}", s.deductions);
        let top = &s.deductions[0];
        assert_eq!(top.what, "ddos (error): sshd-credential-flood t");
        assert_eq!(
            top.why,
            "sshd-credential-flood evidence; same cause `ssh-flood`, scored once: also bleeding sshd-auth-log-growth (error)"
        );
        assert!(!s.pass);
    }

    #[test]
    fn absorbing_incidents_cover_their_components_findings() {
        let flood = inc("ddos:tuner", Sev::Critical, &["tuner"]);
        let wide = inc("stack-wide", Sev::Critical, &["a", "b", "c"]);
        let mut cx = Cortex::default();
        cx.bleeding.push(
            Finding::new(
                FindingKind::Bleeding,
                "errors-tuner",
                Sev::Error,
                "tuner",
                "5xx",
            )
            .with_cause("tuner"),
        );
        cx.bleeding.push(
            Finding::new(FindingKind::Bleeding, "restarts-b", Sev::Error, "b", "loop")
                .with_cause("b"),
        );
        cx.bleeding.push(
            Finding::new(FindingKind::Bleeding, "memory-z", Sev::Warn, "z", "rss").with_cause("z"),
        );
        let s = score(&[&flood, &wide], &cx, &[], &[]);
        assert_eq!(s.score, 100u32.saturating_sub(25 + 25 + 6 + 3));
        let by: Vec<&str> = s.covered.iter().map(|c| c.by.as_str()).collect();
        assert_eq!(by, ["ddos:tuner", "stack-wide"]);
        assert!(s.deductions.iter().any(|d| d.what.contains("rss")));
    }

    #[test]
    fn stream_verdicts_are_never_scored_directly() {
        let mut cx = Cortex::default();
        cx.ddos.push(json!({"stream": "tuner", "level": "flood"}));
        cx.ddos_firing = true;
        let s = score(&[], &cx, &[], &[]);
        assert_eq!((s.score, s.pass), (100, true));
    }

    /// An S2-style sensor: a ddos finding, optionally with its signal.
    struct Flood {
        signal: bool,
    }

    impl Sensor for Flood {
        fn name(&self) -> &'static str {
            "flood"
        }
        fn sense(&mut self, _: &Input<'_>, out: &mut Sink<'_>) {
            out.finding(
                ddos("sshd-credential-flood", Sev::Critical, "host").with_cause("ssh-flood"),
            );
            if self.signal {
                out.signal(Signal::new("ssh-flood", "host", Sev::Critical, "flood"));
            }
        }
    }

    /// Sensors -> Correlator -> score, the engine's order.
    fn pipeline(signal: bool) -> Score {
        let mut s = Sensors::empty();
        s.add(Flood { signal });
        let logs = crate::logscan::LogInsights::default();
        let cx = s.run(&Input::new(1_792_520_000, &[], &logs));
        let mut c = Correlator::default();
        let explain = |cause: &str, _: &[Signal]| Explain {
            title: format!("{cause} title"),
            ..Default::default()
        };
        c.update(1_792_520_000, cx.signals.clone(), &explain);
        let open = c.open_sorted();
        score(&open, &cx, &[], &[])
    }

    #[test]
    fn member_floor_and_member_covering() {
        use crate::incident::Member;
        // A warn scanner incident whose member finding is an error vuln.
        let mut i = inc("scan:console", Sev::Warn, &["console"]);
        i.members = vec![Member::new(
            "vulnerability:scanner-probes-console",
            "scanners",
            Some("scan:console"),
            "console",
            Sev::Error,
        )];
        let mut cx = Cortex::default();
        cx.vulnerabilities
            .push(vuln("scanner-probes-console", Sev::Error, "console").with_cause("scan:console"));
        let s = score(&[&i], &cx, &[], &[]);
        assert_eq!(s.score, 90, "{:?}", s.deductions);
        assert_eq!(s.deductions.len(), 1, "scored once");
        assert!(s.deductions[0].why.contains("member floor"));
        assert!(!s.pass, "an error member blocks pass as it would alone");
        assert!(s.verdict.starts_with("degraded"));
        assert_eq!(s.covered[0].by, "scan:console");
        // Without members: v2 behaviour (5 points, pass).
        i.members.clear();
        let s = score(&[&i], &cx, &[], &[]);
        assert_eq!((s.score, s.pass), (95, true));
        // A member listed by an incident wins over a same-cause one.
        let mut a = inc("edge", Sev::Warn, &["cloudflared"]);
        let mut b = inc("ddos:edge-cloudflared", Sev::Error, &["cloudflared"]);
        b.members = vec![Member::new(
            "vulnerability:scanner-probes-console",
            "scanners",
            Some("edge"),
            "x",
            Sev::Warn,
        )];
        a.members.clear();
        let mut cx = Cortex::default();
        cx.vulnerabilities
            .push(vuln("scanner-probes-console", Sev::Warn, "x").with_cause("edge"));
        let s = score(&[&a, &b], &cx, &[], &[]);
        assert_eq!(s.covered[0].by, "ddos:edge-cloudflared");
        // A floor below the incident's own points changes nothing.
        let mut c = inc("galion-one", Sev::Error, &["galion-one"]);
        c.members = vec![Member::new(
            "bleeding:stuck",
            "components",
            Some("galion-one"),
            "galion-one",
            Sev::Error,
        )];
        let s = score(&[&c], &Cortex::default(), &[], &[]);
        assert_eq!(s.score, 85);
        assert!(!s.deductions[0].why.contains("member floor"));
    }

    #[test]
    fn pipeline_scores_a_flood_exactly_once_with_or_without_signal() {
        let with = pipeline(true);
        assert_eq!(with.score, 75, "{:?}", with.deductions);
        assert_eq!(with.deductions.len(), 1);
        assert!(with.deductions[0].what.starts_with("critical incident: "));
        assert_eq!(with.covered.len(), 1);
        let without = pipeline(false);
        assert_eq!(without.score, 85, "{:?}", without.deductions);
        assert_eq!(without.deductions.len(), 1);
        assert!(without.deductions[0].what.starts_with("ddos (critical)"));
        assert!(!with.pass && !without.pass);
    }
}
