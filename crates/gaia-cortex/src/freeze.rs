// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Freeze watch (gaia-freeze-heal, Mate 2026-10-30 11:27 CET): a service
//! that systemd still shows as running but that no longer answers its
//! health check (hung, or HTTP 5xx) is restarted through the normal
//! decide / act / verify path.
//!
//! Before this module the heal loop only reacted to systemd events
//! (crash, OOM, `failed`). A hung process never produces one, so it stayed
//! hung until a human noticed.
//!
//! **Frozen** is defined conservatively. All of these must hold:
//!
//! - the unit is on the cortex restart allowlist (the nerve reflex
//!   allowlist minus every protected unit, [`crate::rules::Rules::allow`])
//!   and the nerve `/state` lists it with a loopback health port;
//! - its health probe (the nerve's own probe: same port, same path,
//!   `openmind_nerve::probe::http_probe`) fails, i.e. no answer within
//!   the timeout or HTTP 5xx, on `fails` CONSECUTIVE probes (default 6,
//!   one every `probe_s` = 15 s) spanning at least `min_s` (default 90 s).
//!   One good answer resets the streak: a blip is never restarted;
//! - the unit had no systemd lifecycle event (start, stop, crash, exit,
//!   restart, recovery) for `cooldown_s` (default 300 s): a deploy or
//!   operator restart, or the cortex's own restart, is never mistaken for
//!   a freeze. Probes inside the cooldown do not count;
//! - at most one target is failing at a time: two or more unresponsive
//!   services at once is a dependency or host problem, so the cortex only
//!   alerts (`R13-freeze-wide`);
//! - a fresh nerve `/state` read says the unit is `active` (otherwise the
//!   crash path, `R3`, owns it).
//!
//! A fired freeze becomes a `unit.frozen` event that only this module can
//! produce (the engine drops any such event arriving from outside) and
//! goes through the same rules ([`crate::rules`] `R13-*`: circuit breaker,
//! one freeze restart per unit per hour, the per-unit and total restart
//! caps), the same nerve `POST /actions` restart and the same heal verify
//! ([`crate::verify`]): the deadline check of a freeze heal is a fresh
//! health probe, so "the process restarted" is not enough, it must answer.
//! A heal that does not answer opens the circuit (no second blind restart)
//! and alerts.
//!
//! Off unless `CORTEX_FREEZE=1` (`freeze.enabled`). The watcher only
//! sends HTTP GETs to loopback health ports and reads the nerve `/state`.

use crate::verify::StateRead;
use nerve_cortex_contract::Event;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const FREEZE_KIND: &str = "unit.frozen";
pub const FREEZE_SOURCE: &str = "gaia-cortex-freeze";

/// Allowed ranges (anything else refuses to start).
pub const PROBE_S_RANGE: (u64, u64) = (5, 300);
pub const FAILS_RANGE: (u32, u32) = (3, 100);
pub const MIN_S_RANGE: (u64, u64) = (60, 3600);
pub const COOLDOWN_S_RANGE: (u64, u64) = (60, 3600);
pub const PER_HOUR_RANGE: (u32, u32) = (1, 3);
pub const TIMEOUT_S_RANGE: (u64, u64) = (1, 30);
/// The target list is re-read from the nerve `/state` this often.
pub const TARGETS_EVERY_S: u64 = 60;
/// Units probing bad at once (streak >= 2) that make a freeze "wide".
const WIDE_SUSPECTS: usize = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FreezeCfg {
    pub enabled: bool,
    pub probe_s: u64,
    pub fails: u32,
    pub min_s: u64,
    pub cooldown_s: u64,
    /// Freeze restarts per unit per hour.
    pub per_unit_hour: u32,
    pub timeout_s: u64,
}

impl Default for FreezeCfg {
    fn default() -> Self {
        FreezeCfg {
            enabled: false,
            probe_s: 15,
            fails: 6,
            min_s: 90,
            cooldown_s: 300,
            per_unit_hour: 1,
            timeout_s: 5,
        }
    }
}

impl FreezeCfg {
    pub fn validate(&self) -> Result<(), String> {
        let chk = |name: &str, v: u64, (lo, hi): (u64, u64)| {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(format!("freeze.{name} must be {lo}..={hi}, got {v}"))
            }
        };
        chk("probe_s", self.probe_s, PROBE_S_RANGE)?;
        chk(
            "fails",
            self.fails as u64,
            (FAILS_RANGE.0 as u64, FAILS_RANGE.1 as u64),
        )?;
        chk("min_s", self.min_s, MIN_S_RANGE)?;
        chk("cooldown_s", self.cooldown_s, COOLDOWN_S_RANGE)?;
        chk(
            "per_unit_hour",
            self.per_unit_hour as u64,
            (PER_HOUR_RANGE.0 as u64, PER_HOUR_RANGE.1 as u64),
        )?;
        chk("timeout_s", self.timeout_s, TIMEOUT_S_RANGE)?;
        if self.timeout_s >= self.probe_s {
            return Err(format!(
                "freeze.timeout_s ({}) must be below freeze.probe_s ({})",
                self.timeout_s, self.probe_s
            ));
        }
        Ok(())
    }
}

/// One watched service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub unit: String,
    pub port: u16,
    pub path: String,
}

/// One probe answer, classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Answered below 500 (a 3xx/4xx answer is a live process).
    Up(u16),
    /// HTTP 5xx, timeout, refused, ...
    Down(String),
}

impl Probe {
    pub fn from_http(p: &openmind_nerve::probe::HttpProbe) -> Probe {
        match p.code {
            Some(c) if c < 500 => Probe::Up(c),
            Some(c) => Probe::Down(format!("HTTP {c}")),
            None => Probe::Down(clean(p.error.as_deref().unwrap_or("no answer"))),
        }
    }
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect::<String>()
}

/// GET the target's health path on loopback (the nerve's own probe).
pub fn probe(t: &Target, timeout_s: u64) -> Probe {
    Probe::from_http(&openmind_nerve::probe::http_probe(
        t.port,
        &t.path,
        std::time::Duration::from_secs(timeout_s),
    ))
}

/// Normalise a unit name to `x.service`.
pub fn norm_unit(u: &str) -> String {
    if u.contains('.') {
        u.to_string()
    } else {
        format!("{u}.service")
    }
}

/// Health path for a unit: the nerve's built-in spec, else `/health`.
pub fn health_path(unit: &str) -> String {
    let want = norm_unit(unit);
    openmind_nerve::config::default_specs(None, None)
        .into_iter()
        .find(|s| s.unit.is_some_and(|u| norm_unit(u) == want) && s.port.is_some())
        .map(|s| s.path.to_string())
        .filter(|p| p.starts_with('/'))
        .unwrap_or_else(|| "/health".into())
}

/// Watched targets from a nerve `/state` body: components with a unit and
/// a health port whose unit `allowed` (the restart allowlist) accepts.
pub fn targets_from_state(state: &Value, allowed: &dyn Fn(&str) -> bool) -> Vec<Target> {
    let mut out: Vec<Target> = vec![];
    for c in state
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(u) = c
            .get("unit")
            .and_then(Value::as_str)
            .filter(|u| !u.is_empty())
        else {
            continue;
        };
        let Some(port) = c
            .get("port")
            .and_then(Value::as_u64)
            .filter(|p| (1..=65535).contains(p))
        else {
            continue;
        };
        let unit = norm_unit(u);
        if !allowed(&unit) || out.iter().any(|t| t.unit == unit) {
            continue;
        }
        out.push(Target {
            path: health_path(&unit),
            unit,
            port: port as u16,
        });
    }
    out
}

/// Does a nerve `/state` read say `unit` is running? Only an explicit
/// `active` counts (an unknown state never fires a freeze).
pub fn says_active(read: &StateRead) -> bool {
    matches!(read, StateRead::Unit { state, .. } if state == "active")
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Track {
    bad: u32,
    first_bad: Option<u64>,
    last: String,
    cooldown_until: u64,
    last_probe: Option<u64>,
    last_up: Option<u64>,
}

/// A freeze the watcher wants decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    pub target: Target,
    pub bad: u32,
    pub span_s: u64,
    pub detail: String,
    pub wide: bool,
}

impl Fired {
    /// The internal `unit.frozen` event (decided like any other event).
    pub fn event(&self, now: u64) -> Event {
        let u = &self.target.unit;
        let mut e = Event::new(
            FREEZE_SOURCE,
            FREEZE_KIND,
            "error",
            json!({"unit": u, "bad_probes": self.bad, "span_s": self.span_s, "detail": self.detail,
                   "wide": self.wide, "port": self.target.port, "path": self.target.path,
                   "unit_state": "active"}),
        );
        e.unit = Some(u.clone());
        e.t = Some(now);
        e.ts = crate::engine::ts(now);
        e.key = Some(format!("freeze-{u}-{now}"));
        e.msg = Some(format!(
            "{u} unresponsive while running: {} failed health probes over {} s ({})",
            self.bad, self.span_s, self.detail
        ));
        e
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Counts {
    pub rounds: u64,
    pub probes: u64,
    pub down: u64,
    /// Streaks that ended with a good answer before firing.
    pub blips: u64,
    pub fired: u64,
    pub held_wide: u64,
    pub held_not_active: u64,
    pub cooldowns: u64,
}

/// The freeze book (lives in the engine, under its lock).
#[derive(Debug)]
pub struct FreezeWatch {
    pub cfg: FreezeCfg,
    pub targets: Vec<Target>,
    tracks: BTreeMap<String, Track>,
    /// Fired freezes waiting for the decider (never from outside).
    pub inbox: Vec<Event>,
    pub counts: Counts,
}

impl FreezeWatch {
    pub fn new(cfg: FreezeCfg) -> Self {
        FreezeWatch {
            cfg,
            targets: vec![],
            tracks: BTreeMap::new(),
            inbox: vec![],
            counts: Counts::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    pub fn set_targets(&mut self, t: Vec<Target>) {
        self.targets = t;
    }

    pub fn target(&self, unit: &str) -> Option<&Target> {
        let u = norm_unit(unit);
        self.targets.iter().find(|t| t.unit == u)
    }

    /// A lifecycle event (or our own restart) for `unit`: no freeze before
    /// `until`, and the current streak is dropped.
    pub fn cooldown(&mut self, unit: &str, until: u64) {
        let t = self.tracks.entry(norm_unit(unit)).or_default();
        if until > t.cooldown_until {
            t.cooldown_until = until;
            self.counts.cooldowns += 1;
        }
        t.bad = 0;
        t.first_bad = None;
    }

    fn observe(&mut self, target: &Target, p: &Probe, now: u64) -> Option<Fired> {
        let (fails, min_s, cooldown_s) = (self.cfg.fails, self.cfg.min_s, self.cfg.cooldown_s);
        self.counts.probes += 1;
        let t = self.tracks.entry(target.unit.clone()).or_default();
        t.last_probe = Some(now);
        match p {
            Probe::Up(_) => {
                if t.bad > 0 {
                    self.counts.blips += 1;
                }
                t.bad = 0;
                t.first_bad = None;
                t.last_up = Some(now);
                t.last.clear();
                None
            }
            Probe::Down(d) => {
                self.counts.down += 1;
                t.last = d.clone();
                if now < t.cooldown_until {
                    t.bad = 0;
                    t.first_bad = None;
                    return None;
                }
                t.bad += 1;
                let first = *t.first_bad.get_or_insert(now);
                let span = now.saturating_sub(first);
                if t.bad >= fails && span >= min_s {
                    let f = Fired {
                        target: target.clone(),
                        bad: t.bad,
                        span_s: span,
                        detail: d.clone(),
                        wide: false,
                    };
                    t.bad = 0;
                    t.first_bad = None;
                    t.cooldown_until = now + cooldown_s;
                    return Some(f);
                }
                None
            }
        }
    }

    /// One probe round: update every streak, return the freezes that fired.
    /// When 2+ targets are failing at once every fired freeze is `wide`.
    pub fn round(&mut self, results: &[(Target, Probe)], now: u64) -> Vec<Fired> {
        self.counts.rounds += 1;
        let mut fired: Vec<Fired> = results
            .iter()
            .filter_map(|(t, p)| self.observe(t, p, now))
            .collect();
        if fired.is_empty() {
            return fired;
        }
        let suspects = self.tracks.values().filter(|t| t.bad >= 2).count() + fired.len();
        if suspects >= WIDE_SUSPECTS {
            for f in &mut fired {
                f.wide = true;
            }
            self.counts.held_wide += fired.len() as u64;
        }
        self.counts.fired += fired.len() as u64;
        fired
    }

    pub fn to_json(&self) -> Value {
        let c = &self.counts;
        let tracks: Vec<Value> = self
            .targets
            .iter()
            .map(|t| {
                let k = self.tracks.get(&t.unit).cloned().unwrap_or_default();
                json!({"unit": t.unit, "port": t.port, "path": t.path, "bad": k.bad,
                       "bad_since": k.first_bad.map(crate::engine::ts),
                       "cooldown_until": (k.cooldown_until > 0).then(|| crate::engine::ts(k.cooldown_until)),
                       "last_probe": k.last_probe.map(crate::engine::ts),
                       "last_up": k.last_up.map(crate::engine::ts),
                       "last_down": if k.last.is_empty() { Value::Null } else { json!(k.last) }})
            })
            .collect();
        json!({"enabled": self.cfg.enabled, "probe_s": self.cfg.probe_s, "fails": self.cfg.fails,
               "min_s": self.cfg.min_s, "cooldown_s": self.cfg.cooldown_s,
               "per_unit_hour": self.cfg.per_unit_hour, "timeout_s": self.cfg.timeout_s,
               "targets": tracks,
               "counts": {"rounds": c.rounds, "probes": c.probes, "down": c.down, "blips": c.blips,
                          "fired": c.fired, "held_wide": c.held_wide,
                          "held_not_active": c.held_not_active, "cooldowns": c.cooldowns}})
    }
}

/// Is this the internal freeze event?
pub fn is_frozen(e: &Event) -> bool {
    e.kind == FREEZE_KIND && e.source == FREEZE_SOURCE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> FreezeCfg {
        FreezeCfg {
            enabled: true,
            probe_s: 15,
            fails: 6,
            min_s: 90,
            cooldown_s: 300,
            per_unit_hour: 1,
            timeout_s: 5,
        }
    }

    fn tgt(u: &str) -> Target {
        Target {
            unit: u.into(),
            port: 1,
            path: "/health".into(),
        }
    }

    fn down() -> Probe {
        Probe::Down("HTTP 503".into())
    }

    #[test]
    fn defaults_validate_and_bounds_refuse() {
        assert!(FreezeCfg::default().validate().is_ok());
        assert!(!FreezeCfg::default().enabled, "off unless asked");
        for bad in [
            FreezeCfg {
                probe_s: 1,
                ..cfg()
            },
            FreezeCfg { fails: 1, ..cfg() },
            FreezeCfg { min_s: 10, ..cfg() },
            FreezeCfg {
                cooldown_s: 5,
                ..cfg()
            },
            FreezeCfg {
                per_unit_hour: 0,
                ..cfg()
            },
            FreezeCfg {
                per_unit_hour: 9,
                ..cfg()
            },
            FreezeCfg {
                timeout_s: 15,
                ..cfg()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn needs_n_consecutive_and_the_span() {
        let mut w = FreezeWatch::new(cfg());
        let t = tgt("a.service");
        // 6 bad probes 5 s apart: count reached, span (25 s) not
        for i in 0..6 {
            assert!(w.round(&[(t.clone(), down())], 1000 + i * 5).is_empty());
        }
        // keeps failing until 90 s after the first bad probe
        assert!(w.round(&[(t.clone(), down())], 1089).is_empty());
        let f = w.round(&[(t.clone(), down())], 1090);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].span_s, 90);
        assert!(!f[0].wide);
        assert_eq!(f[0].bad, 8);
        // fired: its own cooldown (300 s) now holds the unit
        for i in 1..=19 {
            assert!(w.round(&[(t.clone(), down())], 1090 + i * 15).is_empty());
        }
        // after it a fresh streak (6 probes, >= 90 s) is needed again; the
        // per-unit-hour cap in the rules decides whether it may restart
        for i in 0..6 {
            assert!(w.round(&[(t.clone(), down())], 1400 + i * 15).is_empty());
        }
        assert_eq!(w.round(&[(t.clone(), down())], 1490).len(), 1);
    }

    #[test]
    fn a_blip_resets_the_streak() {
        let mut w = FreezeWatch::new(cfg());
        let t = tgt("a.service");
        for i in 0..5 {
            w.round(&[(t.clone(), down())], 1000 + i * 15);
        }
        w.round(&[(t.clone(), Probe::Up(200))], 1075);
        for i in 0..5 {
            assert!(w.round(&[(t.clone(), down())], 1090 + i * 15).is_empty());
        }
        assert_eq!(w.counts.blips, 1);
        assert_eq!(w.counts.fired, 0);
        // a 404 is a live process, not a freeze
        w.round(&[(t.clone(), Probe::Up(404))], 1200);
        assert_eq!(w.counts.blips, 2);
    }

    #[test]
    fn cooldown_after_a_lifecycle_event() {
        let mut w = FreezeWatch::new(cfg());
        let t = tgt("a.service");
        for i in 0..5 {
            w.round(&[(t.clone(), down())], 1000 + i * 15);
        }
        // a deploy restart is seen on the stream at 1070
        w.cooldown("a", 1070 + 300);
        for i in 0..20 {
            assert!(
                w.round(&[(t.clone(), down())], 1075 + i * 15).is_empty(),
                "inside the cooldown"
            );
        }
        // after the cooldown a full new streak is needed
        let mut fired_at = None;
        for i in 0..20 {
            let now = 1370 + i * 15;
            if !w.round(&[(t.clone(), down())], now).is_empty() {
                fired_at = Some(now);
                break;
            }
        }
        assert_eq!(fired_at, Some(1370 + 90));
    }

    #[test]
    fn two_failing_at_once_is_wide() {
        let mut w = FreezeWatch::new(cfg());
        let (a, b) = (tgt("a.service"), tgt("b.service"));
        let mut fired = vec![];
        for i in 0..8 {
            fired = w.round(&[(a.clone(), down()), (b.clone(), down())], 1000 + i * 15);
            if !fired.is_empty() {
                break;
            }
        }
        assert_eq!(fired.len(), 2);
        assert!(fired.iter().all(|f| f.wide));
        assert_eq!(w.counts.held_wide, 2);
    }

    #[test]
    fn event_shape_and_marker() {
        let f = Fired {
            target: tgt("a.service"),
            bad: 6,
            span_s: 90,
            detail: "HTTP 503".into(),
            wide: false,
        };
        let e = f.event(1000);
        assert!(is_frozen(&e));
        assert_eq!(e.unit.as_deref(), Some("a.service"));
        assert_eq!(e.data["bad_probes"], 6);
        let mut spoof = e.clone();
        spoof.source = "someone".into();
        assert!(!is_frozen(&spoof));
    }

    #[test]
    fn targets_only_allowlisted_with_a_port() {
        let st = json!({"components": [
            {"unit": "tuner.service", "port": 8650},
            {"unit": "galion-desk.service", "port": 8646},
            {"unit": "link-watch-rs.service", "port": null},
            {"unit": "gaia-cortex.service", "port": 8674},
            {"name": "galion-nerve", "unit": null, "port": 8670}]});
        let allow = |u: &str| u == "tuner.service" || u == "link-watch-rs.service";
        let t = targets_from_state(&st, &allow);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].unit, "tuner.service");
        assert_eq!(t[0].path, "/health");
        assert_eq!(health_path("console.service"), "/");
        assert_eq!(health_path("galion-health"), "/status.json");
        assert_eq!(health_path("mock.service"), "/health");
    }

    #[test]
    fn active_only_when_explicit() {
        let u = |s: &str| StateRead::Unit {
            state: s.into(),
            status: "x".into(),
        };
        assert!(says_active(&u("active")));
        assert!(!says_active(&u("")));
        assert!(!says_active(&u("activating")));
        assert!(!says_active(&StateRead::Missing));
        assert!(!says_active(&StateRead::Unreachable("x".into())));
    }

    #[test]
    fn probe_classification() {
        use openmind_nerve::probe::HttpProbe;
        let h = |code: Option<u16>, err: Option<&str>| HttpProbe {
            code,
            ms: 1,
            error: err.map(str::to_string),
            body: String::new(),
        };
        assert_eq!(Probe::from_http(&h(Some(200), None)), Probe::Up(200));
        assert_eq!(Probe::from_http(&h(Some(302), None)), Probe::Up(302));
        assert_eq!(
            Probe::from_http(&h(Some(503), None)),
            Probe::Down("HTTP 503".into())
        );
        assert!(matches!(
            Probe::from_http(&h(None, Some("timed out\n"))),
            Probe::Down(d) if d == "timed out"
        ));
    }
}
