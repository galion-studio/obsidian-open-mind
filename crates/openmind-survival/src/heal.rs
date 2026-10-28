// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Pure repair planning: classify -> plan. Nothing here runs a command,
//! opens a socket or writes a file; `guardian-tick heal` does that.
//!
//! Ported 2026-10-20 from `galion-healer` (317d1d4, dropped by the PR #8
//! merge 13a296c): the protected-unit list, the `[[restart]]` allow-list
//! idea and the "streak before restart" rule. New here: the reflex cap of
//! [`REFLEX_CAP`] restarts per service per [`REFLEX_WINDOW_S`] seconds,
//! which replaces the old "one restart per 600 s" gap.

use crate::decide::{decide, Escalation};
use crate::ProbeResult;

/// At most this many applied restarts per unit in one window.
pub const REFLEX_CAP: u32 = 3;
/// The reflex window: one hour.
pub const REFLEX_WINDOW_S: i64 = 3600;

/// Names the healer never acts on (galion-healer `PROTECTED_UNITS`).
/// Matched case-insensitively as a substring of the unit name.
pub const PROTECTED_UNITS: &[&str] = &[
    "gateway",
    "hermes",
    "whisper",
    "vision",
    "keep.sh",
    "cloudflared",
    "ssh",
    "memory",
];

pub fn is_protected(unit: &str) -> bool {
    let low = unit.to_lowercase();
    PROTECTED_UNITS.iter().any(|p| low.contains(p))
}

/// What a probe result means for repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Healthy,
    /// Host pressure, stale file, unreadable sample: warn, never restart.
    Degraded,
    /// A service, unit or tunnel is down: a restart candidate.
    Down,
}

impl Class {
    pub const fn as_str(self) -> &'static str {
        match self {
            Class::Healthy => "healthy",
            Class::Degraded => "degraded",
            Class::Down => "down",
        }
    }
}

/// One classified probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub name: String,
    pub class: Class,
    pub detail: String,
    /// Operator text from [`decide`]; never a command line.
    pub text: String,
}

/// Classify every result via [`decide`]'s escalation, same order.
pub fn classify(results: &[ProbeResult]) -> Vec<Classified> {
    decide(results)
        .into_iter()
        .zip(results)
        .map(|(s, r)| Classified {
            name: r.name.clone(),
            class: match s.level {
                Escalation::Observe => Class::Healthy,
                Escalation::Warn => Class::Degraded,
                Escalation::SuggestRestart => Class::Down,
            },
            detail: r.detail.clone(),
            text: s.text,
        })
        .collect()
}

/// Allow-list entry: restart `unit` once probe `probe` has failed
/// `after_fails` ticks in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub probe: String,
    pub unit: String,
    pub after_fails: u32,
}

/// The planned step for one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing to do (healthy).
    Observe,
    /// Tell the operator; no repair exists for this class.
    Notify(String),
    /// Down but not yet `after_fails` ticks in a row.
    Wait { fails: u32, after: u32 },
    /// Unit is on the protected list.
    RefuseProtected,
    /// Reflex cap reached in the current window.
    CapReached { used: u32, cap: u32 },
    /// Restart this allow-listed unit (dry-run unless the caller applies it).
    Restart { used: u32, cap: u32 },
}

impl Step {
    pub fn token(&self) -> &'static str {
        match self {
            Step::Observe => "observe",
            Step::Notify(_) => "notify",
            Step::Wait { .. } => "wait",
            Step::RefuseProtected => "refuse-protected",
            Step::CapReached { .. } => "cap-reached",
            Step::Restart { .. } => "restart",
        }
    }

    pub fn reason(&self) -> String {
        match self {
            Step::Observe => "healthy".into(),
            Step::Notify(r) => r.clone(),
            Step::Wait { fails, after } => format!("down {fails}/{after} ticks"),
            Step::RefuseProtected => "unit is protected; never restarted".into(),
            Step::CapReached { used, cap } => {
                format!("reflex cap {used}/{cap} restarts in the last hour")
            }
            Step::Restart { used, cap } => {
                format!("restart {} of {cap} this hour", used + 1)
            }
        }
    }
}

/// One plan line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    pub service: String,
    pub class: Class,
    pub unit: Option<String>,
    pub step: Step,
}

/// Plan one tick.
///
/// * `streak(name)`: consecutive failed ticks including this one.
/// * `used(unit)`: applied restarts of `unit` in the last window.
/// * `cap` is clamped to [`REFLEX_CAP`]; a config can lower it, never raise it.
pub fn plan(
    classes: &[Classified],
    rules: &[Rule],
    streak: &dyn Fn(&str) -> u32,
    used: &dyn Fn(&str) -> u32,
    cap: u32,
) -> Vec<PlanItem> {
    let cap = cap.min(REFLEX_CAP);
    classes
        .iter()
        .map(|c| {
            let rule = rules.iter().find(|r| r.probe == c.name);
            let unit = rule.map(|r| r.unit.clone());
            let step = match (c.class, rule) {
                (Class::Healthy, _) => Step::Observe,
                (Class::Degraded, _) => Step::Notify("degraded: warn only".into()),
                (Class::Down, None) => Step::Notify("down: no allow-list rule".into()),
                (Class::Down, Some(r)) if is_protected(&r.unit) => Step::RefuseProtected,
                (Class::Down, Some(r)) => {
                    let fails = streak(&c.name);
                    let after = r.after_fails.max(1);
                    let n = used(&r.unit);
                    if fails < after {
                        Step::Wait { fails, after }
                    } else if n >= cap {
                        Step::CapReached { used: n, cap }
                    } else {
                        Step::Restart { used: n, cap }
                    }
                }
            };
            PlanItem {
                service: c.name.clone(),
                class: c.class,
                unit,
                step,
            }
        })
        .collect()
}

/// Count unix stamps in `(now - window, now]`.
pub fn count_in_window(stamps: &[i64], now: i64, window: i64) -> u32 {
    stamps
        .iter()
        .filter(|&&t| t > now - window && t <= now)
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: &str, ok: bool, d: &str) -> ProbeResult {
        ProbeResult::new(n, ok, d)
    }

    #[test]
    fn classes() {
        let c = classify(&[
            r("one", true, "status 200"),
            r("mem", false, "used 90%"),
            r("tuner", false, "connect refused"),
        ]);
        let k: Vec<Class> = c.iter().map(|x| x.class).collect();
        assert_eq!(k, [Class::Healthy, Class::Degraded, Class::Down]);
        assert!(!c[2].text.contains('/'));
    }

    #[test]
    fn plan_steps_and_cap() {
        let c = classify(&[
            r("a", false, "x"),
            r("b", false, "x"),
            r("hermes", false, "x"),
            r("n", false, "x"),
            r("ok", true, "200"),
        ]);
        let rules = vec![
            Rule {
                probe: "a".into(),
                unit: "a.service".into(),
                after_fails: 2,
            },
            Rule {
                probe: "b".into(),
                unit: "b.service".into(),
                after_fails: 1,
            },
            Rule {
                probe: "hermes".into(),
                unit: "hermes-rs.service".into(),
                after_fails: 1,
            },
        ];
        let p = plan(&c, &rules, &|_| 1, &|_| 0, 3);
        let t: Vec<&str> = p.iter().map(|x| x.step.token()).collect();
        assert_eq!(
            t,
            ["wait", "restart", "refuse-protected", "notify", "observe"]
        );
        let p = plan(
            &c,
            &rules,
            &|_| 5,
            &|u| if u == "b.service" { 3 } else { 2 },
            99,
        );
        assert_eq!(p[0].step, Step::Restart { used: 2, cap: 3 });
        assert_eq!(p[1].step, Step::CapReached { used: 3, cap: 3 });
        let p = plan(&c, &rules, &|_| 5, &|_| 1, 1);
        assert_eq!(p[1].step, Step::CapReached { used: 1, cap: 1 });
    }

    #[test]
    fn window_and_protected() {
        assert_eq!(count_in_window(&[100, 3700, 3701, 9999], 3701, 3600), 2);
        assert!(is_protected("Hermes-RS.service"));
        assert!(is_protected("ssh.service"));
        assert!(!is_protected("tuner.service"));
    }
}
