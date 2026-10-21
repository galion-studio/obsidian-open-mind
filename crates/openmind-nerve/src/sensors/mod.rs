// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex sensors: one file per detector behind one [`Sensor`] trait.
//!
//! A sensor reads the cycle's [`Input`] (and, only when [`Input::live`] is
//! true, the host itself, read-only) and reports [`Finding`]s, incident
//! [`Signal`]s and optional cortex sections through a [`Sink`]. The
//! [`Sensors`] registry runs them in order and assembles the [`Cortex`].
//!
//! # Adding a sensor (cortex plan S2)
//!
//! 1. New file `src/sensors/<name>.rs`: a type implementing [`Sensor`] and,
//!    if it has settings, `fn from_env(env: Env) -> Option<Self>` that reads
//!    its own `NERVE_<NAME>_*` variables (None = sensor off).
//! 2. Two lines here: `pub mod <name>;` in the module list and one
//!    `s.add(..)` / `s.add_opt(..)` line in [`Sensors::from_env`].
//! 3. A fixture, a unit test (drive it with [`Input::new`] and
//!    [`Sensors::run`]), a replay case and a `GALION_REAL=1` box test.
//!
//! Rules (docs/cortex.md section 6, plan section 3): read-only, no listen
//! socket, no Python, no raw log lines or addresses in titles/events (use
//! counts), never restart/ban/edit anything; must not block (budget a few
//! ms per cycle); never touch `engine.rs`, `cortex.rs`, `config.rs`.
//!
//! The built-in sensors ([`Sensors::builtin`]) are the detectors the cortex
//! had before the trait existed, moved here without behaviour change.
//! `NERVE_SENSORS_OFF=name,name` switches sensors off by [`Sensor::name`].

use crate::cortex::{Cortex, Finding, FindingKind, Input};
use crate::incident::Signal;
use serde_json::{json, Value};
use std::time::Instant;

pub mod components;
pub mod ddos;
pub mod edge;
pub mod listeners;
pub mod scanners;

/// Environment lookup handed to `from_env` constructors (tests pass a map).
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// One cortex detector.
///
/// Called once per engine cycle with that cycle's [`Input`]. `&mut self` lets
/// a sensor keep state between cycles (previous counters for a rate, a
/// warm-up baseline). It must be cheap, never block on the network for long,
/// and never act on what it sees.
pub trait Sensor: Send {
    /// Stable short name (`listeners`, `sshd-auth`): registry key, the
    /// default B33 `source` of its findings, and the `NERVE_SENSORS_OFF` key.
    fn name(&self) -> &'static str;

    /// Look at one cycle and report through `out`.
    fn sense(&mut self, input: &Input<'_>, out: &mut Sink<'_>);
}

/// Where a sensor reports. Routes findings by [`FindingKind`] and stamps
/// the sensor's name as the finding source when the sensor left it empty.
pub struct Sink<'c> {
    cx: &'c mut Cortex,
    source: &'static str,
    findings: usize,
    signals: usize,
}

impl<'c> Sink<'c> {
    /// A sink writing into `cx` on behalf of the sensor named `source`.
    pub fn new(cx: &'c mut Cortex, source: &'static str) -> Self {
        Sink {
            cx,
            source,
            findings: 0,
            signals: 0,
        }
    }

    /// The reporting sensor's name.
    pub fn source(&self) -> &'static str {
        self.source
    }

    /// Report a finding (vulnerabilities / bleeding / ddos by its kind).
    pub fn finding(&mut self, mut f: Finding) {
        if f.source.is_empty() {
            f.source = self.source.to_string();
        }
        self.findings += 1;
        match f.kind {
            FindingKind::Vulnerability => self.cx.vulnerabilities.push(f),
            FindingKind::Bleeding => self.cx.bleeding.push(f),
            FindingKind::Ddos => self.cx.ddos_findings.push(f),
        }
    }

    /// Report an incident signal (correlated by its `cause`, then scored).
    pub fn signal(&mut self, s: Signal) {
        self.signals += 1;
        self.cx.signals.push(s);
    }

    /// Set the cortex `exposure` section (the listeners sensor owns it).
    pub fn exposure(&mut self, v: Value) {
        self.cx.exposure = v;
    }

    /// Add one request-rate stream verdict to the `ddos.streams` section;
    /// `firing` raises the cortex-wide DDoS flag.
    pub fn ddos_stream(&mut self, v: Value, firing: bool) {
        self.cx.ddos.push(v);
        self.cx.ddos_firing |= firing;
    }
}

/// What one sensor did in the last cycle (for `/api/v1/state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorRun {
    pub name: &'static str,
    pub findings: usize,
    pub signals: usize,
    pub micros: u64,
}

/// The ordered sensor registry.
#[derive(Default)]
pub struct Sensors {
    list: Vec<Box<dyn Sensor>>,
    /// Per-sensor counts and time of the last [`Sensors::run`].
    pub last: Vec<SensorRun>,
}

impl Sensors {
    /// No sensors.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The pre-trait cortex detectors, in their historic order (the order
    /// fixes the signal order and so incident ids; do not reorder).
    pub fn builtin() -> Self {
        let mut s = Self::empty();
        s.add(listeners::Listeners)
            .add(scanners::Scanners)
            .add(edge::DeadOrigins)
            .add(components::ComponentBleeding)
            .add(ddos::RateStreams);
        s
    }

    /// Built-ins plus every registered S2 sensor that is configured, minus
    /// the names listed in `NERVE_SENSORS_OFF`.
    pub fn from_env(env: Env) -> Self {
        let mut s = Self::builtin();
        // S2 sensors: one line each, e.g. `s.add_opt(sshd::SshAuth::from_env(env));`
        let off: Vec<String> = env("NERVE_SENSORS_OFF")
            .unwrap_or_default()
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        s.list.retain(|x| !off.iter().any(|o| o == x.name()));
        s
    }

    /// Append a sensor (runs after the ones already registered).
    pub fn add<S: Sensor + 'static>(&mut self, sensor: S) -> &mut Self {
        self.list.push(Box::new(sensor));
        self
    }

    /// Append a sensor when its `from_env` returned one.
    pub fn add_opt<S: Sensor + 'static>(&mut self, sensor: Option<S>) -> &mut Self {
        if let Some(s) = sensor {
            self.add(s);
        }
        self
    }

    /// Registered sensor names, in run order.
    pub fn names(&self) -> Vec<&'static str> {
        self.list.iter().map(|s| s.name()).collect()
    }

    /// Run every sensor once over `input` and assemble the cortex
    /// (findings sorted worst first, then by id; signals in sensor order).
    pub fn run(&mut self, input: &Input) -> Cortex {
        let mut cx = Cortex::default();
        self.last.clear();
        for s in self.list.iter_mut() {
            let t = Instant::now();
            let name = s.name();
            let mut sink = Sink::new(&mut cx, name);
            s.sense(input, &mut sink);
            let (findings, signals) = (sink.findings, sink.signals);
            self.last.push(SensorRun {
                name,
                findings,
                signals,
                micros: t.elapsed().as_micros() as u64,
            });
        }
        let order = |a: &Finding, b: &Finding| b.sev.cmp(&a.sev).then(a.id.cmp(&b.id));
        cx.vulnerabilities.sort_by(order);
        cx.bleeding.sort_by(order);
        cx.ddos_findings.sort_by(order);
        cx
    }

    /// `last` as JSON (`[{name, findings, signals, us}]`).
    pub fn last_json(&self) -> Value {
        self.last
            .iter()
            .map(|r| json!({"name": r.name, "findings": r.findings, "signals": r.signals, "us": r.micros}))
            .collect::<Vec<_>>()
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incident::Sev;
    use crate::logscan::LogInsights;

    /// A stateful S2-style sensor: counts journal lines matching a pattern
    /// and reports the delta since the previous cycle.
    struct Counter {
        seen: usize,
        host_reads: usize,
    }

    impl Sensor for Counter {
        fn name(&self) -> &'static str {
            "counter"
        }
        fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
            if i.live {
                self.host_reads += 1; // a real sensor would read the host here
            }
            let n = i
                .journal
                .iter()
                .filter(|l| l.msg.contains("Failed password"))
                .count();
            self.seen += n;
            if n >= 2 {
                out.finding(
                    Finding::new(
                        FindingKind::Ddos,
                        "ssh-flood",
                        Sev::Critical,
                        "host",
                        format!("{n} failed passwords"),
                    )
                    .with_source("auth.log")
                    .with_cause("ssh"),
                );
                out.signal(Signal::new("ssh", "host", Sev::Critical, "flood"));
            } else if n == 1 {
                out.finding(Finding::new(
                    FindingKind::Vulnerability,
                    "ssh-fail",
                    Sev::Info,
                    "host",
                    "1 failed password",
                ));
            }
        }
    }

    fn jl(msg: &str) -> crate::probe::JLine {
        crate::probe::JLine {
            t: 1,
            unit: "ssh.service".into(),
            prio: 6,
            msg: msg.into(),
        }
    }

    #[test]
    fn builtin_order_is_fixed() {
        assert_eq!(
            Sensors::builtin().names(),
            ["listeners", "scanners", "edge", "components", "ddos"]
        );
        assert_eq!(
            Sensors::from_env(&|_| None).names(),
            Sensors::builtin().names()
        );
    }

    #[test]
    fn sensors_off_by_name() {
        let env = |k: &str| (k == "NERVE_SENSORS_OFF").then(|| " edge, ddos ,nope".to_string());
        assert_eq!(
            Sensors::from_env(&env).names(),
            ["listeners", "scanners", "components"]
        );
    }

    #[test]
    fn add_opt_and_empty() {
        let mut s = Sensors::empty();
        s.add_opt(None::<Counter>).add_opt(Some(Counter {
            seen: 0,
            host_reads: 0,
        }));
        assert_eq!(s.names(), ["counter"]);
        let logs = LogInsights::default();
        let cx = s.run(&Input::new(5, &[], &logs));
        assert!(cx.findings().next().is_none());
        assert_eq!(
            s.last,
            [SensorRun {
                name: "counter",
                findings: 0,
                signals: 0,
                micros: s.last[0].micros
            }]
        );
    }

    #[test]
    fn sink_routes_by_kind_stamps_source_and_keeps_state() {
        let mut s = Sensors::builtin();
        s.add(Counter {
            seen: 0,
            host_reads: 0,
        });
        let logs = LogInsights::default();
        let lines = [
            jl("Failed password for x"),
            jl("Failed password for y"),
            jl("Accepted publickey"),
        ];
        let mut i = Input::new(1_792_520_000, &[], &logs);
        i.journal = &lines;
        let cx = s.run(&i);
        assert_eq!(cx.ddos_findings.len(), 1);
        assert_eq!(cx.ddos_findings[0].source, "auth.log");
        assert!(cx.vulnerabilities.is_empty() && cx.bleeding.is_empty());
        assert!(
            !cx.ddos_firing,
            "a ddos finding alone does not raise the stream flag"
        );
        assert_eq!(
            cx.signals
                .iter()
                .map(|s| s.cause.as_str())
                .collect::<Vec<_>>(),
            ["ssh"]
        );
        assert_eq!(cx.to_json()["ddos"]["findings"][0]["kind"], "ddos");
        let run = s.last.iter().find(|r| r.name == "counter").unwrap();
        assert_eq!((run.findings, run.signals), (1, 1));
        // Second cycle: one line, info finding stamped with the sensor name.
        let one = [jl("Failed password for z")];
        i.journal = &one;
        let cx = s.run(&i);
        assert_eq!(cx.vulnerabilities[0].source, "counter");
        assert!(cx.ddos_findings.is_empty());
        assert!(
            cx.to_json()["ddos"].get("findings").is_none(),
            "no key when empty (unchanged JSON)"
        );
        // Live flag is passed through; replay/tests are not live.
        i.live = true;
        s.run(&i);
        assert!(s.names().contains(&"counter"));
    }

    #[test]
    fn b41_severity_and_kind_round_trip() {
        for (sev, b41) in [
            (Sev::Info, "info"),
            (Sev::Warn, "warn"),
            (Sev::Error, "high"),
            (Sev::Critical, "crit"),
        ] {
            assert_eq!(sev.b41(), b41);
            assert_eq!(Sev::parse(b41), Some(sev));
            assert_eq!(Sev::parse(sev.as_str()), Some(sev));
        }
        assert_eq!(Sev::parse("panic"), None);
        for k in [
            FindingKind::Vulnerability,
            FindingKind::Bleeding,
            FindingKind::Ddos,
        ] {
            assert_eq!(FindingKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(FindingKind::parse("bleed"), None);
    }

    #[test]
    fn nerve_events_highest_severity_wins_per_source_kind() {
        let mut cx = Cortex::default();
        let mut sink = Sink::new(&mut cx, "listeners");
        sink.finding(
            Finding::new(FindingKind::Vulnerability, "b", Sev::Warn, "host", "w")
                .with_evidence(vec!["LISTEN secret-host:1".into()]),
        );
        sink.finding(Finding::new(
            FindingKind::Vulnerability,
            "a",
            Sev::Error,
            "host",
            "e",
        ));
        sink.finding(Finding::new(
            FindingKind::Bleeding,
            "i",
            Sev::Info,
            "host",
            "info only",
        ));
        let mut sink = Sink::new(&mut cx, "auth.log");
        sink.finding(Finding::new(
            FindingKind::Ddos,
            "f",
            Sev::Critical,
            "host",
            "flood",
        ));
        // 2026-10-25 02:00:00+01:00 is the first CET second (DST fold).
        let ev = cx.nerve_events(1_792_890_000);
        assert_eq!(ev.len(), 2, "{ev:?}");
        assert_eq!(ev[0]["source"], "auth.log");
        assert_eq!(
            (ev[0]["kind"].as_str(), ev[0]["severity"].as_str()),
            (Some("ddos"), Some("crit"))
        );
        assert_eq!(ev[1]["source"], "listeners");
        assert_eq!(ev[1]["severity"], "high");
        assert_eq!(ev[1]["data"]["findings"], 2);
        assert_eq!(ev[1]["data"]["top"]["id"], "a");
        assert_eq!(ev[1]["ts"], "2026-10-25T02:00:00+01:00");
        let one = cx
            .vulnerabilities
            .iter()
            .find(|f| f.id == "b")
            .unwrap()
            .to_nerve_event(1_792_889_999);
        assert_eq!(one["ts"], "2026-10-25T02:59:59+02:00");
        assert_eq!(one["data"]["evidence_n"], 1);
        assert!(
            !one.to_string().contains("secret-host"),
            "evidence never leaves in events"
        );
    }
}
