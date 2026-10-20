// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Self-eval scoring: one number, every point explained.
//!
//! Start at 100 and deduct. Incidents are scored once per incident (that is
//! what correlation is for: five signals of one cause cost one deduction,
//! not five). Cortex findings that belong to an incident are not scored
//! again.
//!
//! | what | points |
//! |---|---|
//! | open incident, critical | 25 (+3 per extra component, max +9) |
//! | open incident, error | 15 (+2 per extra component, max +6) |
//! | open incident, warn | 5 |
//! | vulnerability error / warn (not in an incident) | 10 / 4 |
//! | bleeding error / warn (not in an incident) | 8 / 3 |
//! | probe error (nerve blind spot) | 5 each, max 15 |
//! | nerve self-check warn (RSS, cycle time) | 3 each |
//!
//! Grade: A >= 90, B >= 80, C >= 70, D >= 55, F below.
//! `pass` = no critical/error incident, no error-level vulnerability, no
//! probe error, and score >= 70.

use crate::cortex::Cortex;
use crate::incident::{Incident, Sev};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Deduction {
    pub points: u32,
    pub what: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub score: u32,
    pub grade: &'static str,
    pub pass: bool,
    pub verdict: String,
    pub deductions: Vec<Deduction>,
}

impl Score {
    pub fn to_json(&self) -> Value {
        json!({"score": self.score, "grade": self.grade, "pass": self.pass, "verdict": self.verdict,
               "deductions": self.deductions.iter().map(|d| json!({"points": d.points, "what": d.what, "why": d.why})).collect::<Vec<_>>(),
               "method": "100 minus deductions; incidents scored once per correlated cause; see score.rs"})
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

pub fn score(
    incidents: &[&Incident],
    cx: &Cortex,
    probe_errors: &[String],
    self_warns: &[String],
) -> Score {
    let mut d: Vec<Deduction> = vec![];
    for i in incidents {
        let extra = i.components.len().saturating_sub(1) as u32;
        let (base, per, cap) = match i.sev {
            Sev::Critical => (25, 3, 9),
            Sev::Error => (15, 2, 6),
            Sev::Warn => (5, 0, 0),
            Sev::Info => continue,
        };
        d.push(Deduction {
            points: base + (extra * per).min(cap),
            what: format!("{} incident: {}", i.sev.as_str(), i.title),
            why: format!(
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
            ),
        });
    }
    let in_incident = |c: &Option<String>| {
        c.as_ref()
            .is_some_and(|c| incidents.iter().any(|i| &i.cause == c))
    };
    for v in &cx.vulnerabilities {
        if in_incident(&v.cause) {
            continue;
        }
        let p = match v.sev {
            Sev::Critical | Sev::Error => 10,
            Sev::Warn => 4,
            Sev::Info => 0,
        };
        if p > 0 {
            d.push(Deduction {
                points: p,
                what: format!("vulnerability ({}): {}", v.sev.as_str(), v.title),
                why: v.evidence.join("; "),
            });
        }
    }
    for b in &cx.bleeding {
        if in_incident(&b.cause) {
            continue;
        }
        let p = match b.sev {
            Sev::Critical | Sev::Error => 8,
            Sev::Warn => 3,
            Sev::Info => 0,
        };
        if p > 0 {
            d.push(Deduction {
                points: p,
                what: format!("bleeding ({}): {}", b.sev.as_str(), b.title),
                why: b.evidence.join("; "),
            });
        }
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
        || cx
            .vulnerabilities
            .iter()
            .any(|v| v.sev >= Sev::Error && !in_incident(&v.cause))
        || !probe_errors.is_empty();
    let pass = !blocking && s >= 70;
    let crit = incidents.iter().filter(|i| i.sev == Sev::Critical).count();
    let err = incidents.iter().filter(|i| i.sev == Sev::Error).count();
    let verdict = if incidents.is_empty() && d.is_empty() {
        "healthy: no open incidents, no cortex findings".to_string()
    } else {
        let state = if crit > 0 {
            "failing"
        } else if err > 0 {
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
        format!(
            "{state}: {} open incident(s) ({crit} critical, {err} error), {} vulnerability finding(s), {} bleeding point(s). {}",
            incidents.len(),
            cx.vulnerabilities.iter().filter(|v| v.sev > Sev::Info).count(),
            cx.bleeding.iter().filter(|v| v.sev > Sev::Info).count(),
            parts.join(" | ")
        )
    };
    Score {
        score: s,
        grade: grade(s),
        pass,
        verdict,
        deductions: d,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortex::Finding;
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
        cx.bleeding.push(Finding {
            id: "edge-dead-origins".into(),
            sev: Sev::Error,
            component: "cloudflared".into(),
            title: "t".into(),
            evidence: vec![],
            fix: String::new(),
            cause: Some("edge".into()),
        });
        cx.vulnerabilities.push(Finding {
            id: "unmanaged-listeners".into(),
            sev: Sev::Warn,
            component: "host".into(),
            title: "5 stray".into(),
            evidence: vec![],
            fix: String::new(),
            cause: None,
        });
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
}
