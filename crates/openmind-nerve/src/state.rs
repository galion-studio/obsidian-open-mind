// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Correlation: per-component sample history, restart detection, trends, and
//! the healthy / degrading / failing-soon / failing verdict.

use crate::config::{Kind, Spec};
use crate::util::{median, slope, ts};
use serde_json::{json, Value};
use std::collections::VecDeque;

pub const HISTORY: usize = 240;
/// A stop job logged up to this many seconds before the previous sample still
/// explains a restart first seen in this one (same-second probe race).
pub const PLANNED_SLACK_S: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Absent,
    Healthy,
    Degrading,
    FailingSoon,
    Failing,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Absent => "absent",
            Status::Healthy => "healthy",
            Status::Degrading => "degrading",
            Status::FailingSoon => "failing-soon",
            Status::Failing => "failing",
        }
    }
}

/// One observation of one component.
#[derive(Debug, Clone, Default)]
pub struct Sample {
    pub t: u64,
    pub present: bool,
    pub unit_active: Option<String>,
    pub unit_sub: Option<String>,
    pub unit_result: Option<String>,
    pub file_state: Option<String>,
    pub http_code: Option<u16>,
    pub latency_ms: Option<u64>,
    pub probe_error: Option<String>,
    /// cgroup memory (MemoryCurrent) in KiB, else main-PID VmRSS.
    pub mem_kb: Option<u64>,
    pub rss_kb: Option<u64>,
    pub mem_max_kb: Option<u64>,
    pub cpu_nsec: Option<u64>,
    pub cpu_pct: Option<f64>,
    pub pid: u32,
    pub n_restarts: u32,
    pub active_enter: u64,
    pub journal_errors: u32,
    pub journal_warnings: u32,
    pub last_error: Option<String>,
    pub pushed_errors: u32,
    pub tunnel_conns: Option<u64>,
    pub tunnel_requests: Option<u64>,
    pub tunnel_req_errors: Option<u64>,
    pub disk_total_kb: Option<u64>,
    pub disk_avail_kb: Option<u64>,
    pub ram_total_kb: Option<u64>,
    pub ram_avail_kb: Option<u64>,
    pub load1: Option<f64>,
    pub health_says: Option<bool>,
    pub extra: Value,
}

/// Thresholds for the verdict.
#[derive(Debug, Clone)]
pub struct Thresholds {
    pub mem_warn_pct: f64,
    pub mem_crit_pct: f64,
    pub errors_warn_5m: u32,
    pub errors_crit_5m: u32,
    pub disk_warn_pct: f64,
    pub disk_crit_pct: f64,
    pub ram_warn_avail_pct: f64,
    pub ram_crit_avail_pct: f64,
    pub eta_mem_s: f64,
    pub eta_disk_s: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            mem_warn_pct: 75.0,
            mem_crit_pct: 90.0,
            errors_warn_5m: 3,
            errors_crit_5m: 30,
            disk_warn_pct: 85.0,
            disk_crit_pct: 95.0,
            ram_warn_avail_pct: 15.0,
            ram_crit_avail_pct: 5.0,
            eta_mem_s: 1800.0,
            eta_disk_s: 86400.0,
        }
    }
}

/// History and verdict for one component.
#[derive(Debug, Clone)]
pub struct CompState {
    pub spec: Spec,
    pub hist: VecDeque<Sample>,
    pub restarts: VecDeque<u64>,
    /// Restarts that followed a journal stop / restart job (`unit.stopping`):
    /// shown, but not counted as degradation (slot 310).
    pub planned_restarts: VecDeque<u64>,
    /// Stop-job times already matched to a restart (one stop explains one restart).
    planned_used: VecDeque<u64>,
    pub consecutive_down: u32,
    pub status: Status,
    pub reasons: Vec<String>,
    pub status_since: u64,
    /// True when the latest sample shows a hard failure the reflex may act on.
    pub hard_down: bool,
}

impl CompState {
    pub fn new(spec: Spec) -> Self {
        CompState {
            spec,
            hist: VecDeque::new(),
            restarts: VecDeque::new(),
            planned_restarts: VecDeque::new(),
            planned_used: VecDeque::new(),
            consecutive_down: 0,
            status: Status::Healthy,
            reasons: vec![],
            status_since: 0,
            hard_down: false,
        }
    }

    pub fn latest(&self) -> Option<&Sample> {
        self.hist.back()
    }

    /// Add a sample: derives CPU %, detects restarts, updates the verdict.
    pub fn push(&mut self, s: Sample, th: &Thresholds) {
        self.push_planned(s, th, &[], &[]);
    }

    /// Like [`push`](Self::push); `stops` = times of journal stop / restart jobs
    /// of this unit. A restart detected between the previous sample and this one
    /// with an unused stop job in that window is planned (operator, cutover,
    /// cortex restart) and goes to `planned_restarts`, not `restarts`.
    /// `crashes` = times of unplanned failures of this unit (`unit.failed`,
    /// `unit.exit`, ... not inside a stop job): a crash in the window wins, the
    /// restart is counted (VPS 2026-10-27 19:52 CET: operator restart of hermes-rs,
    /// then the new invocation crashed with exit 2 in the same second).
    pub fn push_planned(&mut self, mut s: Sample, th: &Thresholds, stops: &[u64], crashes: &[u64]) {
        if let Some(p) = self.hist.back() {
            if let (Some(a), Some(b)) = (p.cpu_nsec, s.cpu_nsec) {
                let dt = s.t.saturating_sub(p.t) as f64;
                if dt > 0.0 && b >= a {
                    s.cpu_pct = Some(((b - a) as f64 / 1e9 / dt * 100.0 * 10.0).round() / 10.0);
                }
            }
            if p.present && s.present {
                let by_counter = s.n_restarts.saturating_sub(p.n_restarts);
                let by_pid = p.pid != 0 && s.pid != 0 && p.pid != s.pid;
                let by_enter =
                    p.active_enter != 0 && s.active_enter != 0 && p.active_enter != s.active_enter;
                let n = by_counter.max(u32::from(by_pid || by_enter)).min(10);
                let lo = p.t.saturating_sub(PLANNED_SLACK_S);
                // multiset match: two stop jobs in the same second explain two restarts
                let mut used: Vec<u64> = self.planned_used.iter().copied().collect();
                let mut planned = 0;
                let crashed = crashes.iter().any(|t| *t > lo && *t <= s.t);
                let window = stops
                    .iter()
                    .copied()
                    .filter(|t| !crashed && *t > lo && *t <= s.t);
                for t in window {
                    if planned >= n {
                        break;
                    }
                    if let Some(i) = used.iter().position(|x| *x == t) {
                        used.swap_remove(i);
                    } else {
                        self.planned_used.push_back(t);
                        planned += 1;
                    }
                }
                for i in 0..n {
                    if i < planned {
                        self.planned_restarts.push_back(s.t);
                    } else {
                        self.restarts.push_back(s.t);
                    }
                }
            }
        }
        while self.restarts.front().is_some_and(|t| *t + 3600 < s.t) {
            self.restarts.pop_front();
        }
        while self
            .planned_restarts
            .front()
            .is_some_and(|t| *t + 3600 < s.t)
        {
            self.planned_restarts.pop_front();
        }
        while self.planned_used.front().is_some_and(|t| *t + 7200 < s.t)
            || self.planned_used.len() > 256
        {
            self.planned_used.pop_front();
        }
        if self.hist.len() >= HISTORY {
            self.hist.pop_front();
        }
        let now = s.t;
        self.hist.push_back(s);
        let (st, reasons, hard) = self.assess(now, th);
        if hard {
            self.consecutive_down += 1;
        } else {
            self.consecutive_down = 0;
        }
        self.hard_down = hard;
        if st != self.status || self.status_since == 0 {
            self.status_since = now;
        }
        self.status = st;
        self.reasons = reasons;
    }

    pub fn restarts_1h(&self, now: u64) -> usize {
        self.restarts.iter().filter(|t| **t + 3600 >= now).count()
    }

    /// Planned restarts (after a stop / restart job) in the last hour.
    pub fn planned_restarts_1h(&self, now: u64) -> usize {
        self.planned_restarts
            .iter()
            .filter(|t| **t + 3600 >= now)
            .count()
    }

    /// Verdict for the latest sample: (status, reasons, hard_down).
    pub fn assess(&self, now: u64, th: &Thresholds) -> (Status, Vec<String>, bool) {
        let Some(s) = self.hist.back() else {
            return (Status::Absent, vec!["no samples".into()], false);
        };
        let spec = &self.spec;
        let mut st = Status::Healthy;
        let mut why: Vec<String> = vec![];
        let mut hard = false;
        let mut bump = |lvl: Status, r: String, st: &mut Status| {
            if lvl > *st {
                *st = lvl;
            }
            why.push(format!("{}: {r}", lvl.as_str()));
        };
        if !s.present {
            return if spec.expected {
                (
                    Status::Failing,
                    vec!["failing: unit/endpoint not found".into()],
                    false,
                )
            } else {
                (
                    Status::Absent,
                    vec![format!("absent: {}", spec.note)],
                    false,
                )
            };
        }
        let sev = if spec.critical {
            Status::Failing
        } else {
            Status::Degrading
        };
        match s.unit_active.as_deref() {
            Some("failed") => {
                hard = true;
                bump(
                    sev,
                    format!(
                        "unit failed (result {})",
                        s.unit_result.as_deref().unwrap_or("?")
                    ),
                    &mut st,
                )
            }
            Some("inactive") => {
                hard = true;
                bump(sev, "unit inactive".into(), &mut st)
            }
            Some("activating") | Some("deactivating") | Some("reloading") => bump(
                Status::Degrading,
                format!("unit {}", s.unit_active.as_deref().unwrap_or("")),
                &mut st,
            ),
            _ => {}
        }
        if spec.port.is_some() && !matches!(s.http_code, Some(c) if (200..400).contains(&c)) {
            let what = match (s.http_code, &s.probe_error) {
                (Some(c), _) => format!("health probe HTTP {c}"),
                (None, Some(e)) => format!("health probe down ({e})"),
                _ => "health probe down".into(),
            };
            if spec.critical {
                hard = true;
            }
            bump(sev, what, &mut st);
        }
        if spec.kind == Kind::Tunnel {
            match s.tunnel_conns {
                Some(0) => {
                    hard = spec.critical;
                    bump(sev, "tunnel has 0 edge connections".into(), &mut st)
                }
                Some(c) if c < 2 && spec.critical => bump(
                    Status::Degrading,
                    format!("only {c} edge connection(s)"),
                    &mut st,
                ),
                None if s.unit_active.as_deref() == Some("active") => {
                    bump(Status::Degrading, "metrics port unreadable".into(), &mut st)
                }
                _ => {}
            }
            // Origin error rate over the last ~10 min (counters are cumulative).
            if let (Some(old), Some(req), Some(err)) = (
                self.hist
                    .iter()
                    .find(|x| x.t + 600 >= now && x.pid == s.pid),
                s.tunnel_requests,
                s.tunnel_req_errors,
            ) {
                if let (Some(r0), Some(e0)) = (old.tunnel_requests, old.tunnel_req_errors) {
                    let (dr, de) = (req.saturating_sub(r0), err.saturating_sub(e0));
                    if dr >= 20 && de * 4 >= dr {
                        bump(
                            Status::Degrading,
                            format!(
                                "tunnel origin errors {de}/{dr} requests in {} min",
                                (now - old.t) / 60
                            ),
                            &mut st,
                        );
                    }
                }
            }
        }
        let r1h = self.restarts_1h(now);
        if r1h >= 3 {
            bump(
                Status::FailingSoon,
                format!("{r1h} restarts in the last hour"),
                &mut st,
            );
        } else if r1h >= 1 {
            bump(
                Status::Degrading,
                format!("{r1h} restart(s) in the last hour"),
                &mut st,
            );
        }
        if let (Some(m), Some(mx)) = (s.mem_kb, s.mem_max_kb) {
            let pct = m as f64 * 100.0 / mx.max(1) as f64;
            if pct >= th.mem_crit_pct {
                bump(
                    Status::FailingSoon,
                    format!("memory {pct:.0}% of MemoryMax"),
                    &mut st,
                );
            } else if pct >= th.mem_warn_pct {
                bump(
                    Status::Degrading,
                    format!("memory {pct:.0}% of MemoryMax"),
                    &mut st,
                );
            }
        }
        // RSS trend over the last 30 min of the same process.
        let pts: Vec<(f64, f64)> = self
            .hist
            .iter()
            .filter(|x| x.t + 1800 >= now && x.pid == s.pid)
            .filter_map(|x| Some((x.t as f64, x.mem_kb? as f64)))
            .collect();
        if pts.len() >= 10 {
            if let Some(k) = slope(&pts) {
                let first = pts[0].1.max(1.0);
                let last = pts[pts.len() - 1].1;
                let rising =
                    pts.windows(2).filter(|w| w[1].1 >= w[0].1).count() * 10 >= (pts.len() - 1) * 8;
                // Ignore tiny processes: +25% of 3 MiB is noise, not a leak.
                if k > 0.0 && last / first > 1.25 && last - first >= 8192.0 && rising {
                    bump(
                        Status::Degrading,
                        format!(
                            "memory rising {:.0}% over {} min ({:.0} KiB/min)",
                            (last / first - 1.0) * 100.0,
                            (pts[pts.len() - 1].0 - pts[0].0) as u64 / 60,
                            k * 60.0
                        ),
                        &mut st,
                    );
                    let limit = s.mem_max_kb.or(s.ram_total_kb).map(|x| x as f64);
                    if let Some(l) = limit {
                        let eta = (l - last) / k;
                        if eta > 0.0 && eta < th.eta_mem_s {
                            bump(
                                Status::FailingSoon,
                                format!(
                                    "memory limit reached in ~{} min at this rate",
                                    eta as u64 / 60
                                ),
                                &mut st,
                            );
                        }
                    }
                }
            }
        }
        // Latency: last 3 probes against the median of the earlier ones.
        let lats: Vec<f64> = self
            .hist
            .iter()
            .filter_map(|x| x.latency_ms.map(|l| l as f64))
            .collect();
        if lats.len() >= 8 {
            let (base, tail) = lats.split_at(lats.len() - 3);
            if let Some(m) = median(base) {
                let thr = (m * 3.0).max(m + 200.0);
                if tail.iter().all(|l| *l > 2000.0) {
                    bump(
                        Status::FailingSoon,
                        format!("latency > 2 s for 3 probes (median {m:.0} ms)"),
                        &mut st,
                    );
                } else if tail.iter().all(|l| *l > thr) {
                    bump(
                        Status::Degrading,
                        format!("latency rising: {:.0} ms vs median {m:.0} ms", tail[2]),
                        &mut st,
                    );
                }
            }
        }
        let errs: u32 = self
            .hist
            .iter()
            .filter(|x| x.t + 300 > now)
            .map(|x| x.journal_errors + x.pushed_errors)
            .sum();
        if errs >= th.errors_crit_5m {
            bump(
                Status::FailingSoon,
                format!("{errs} errors in 5 min"),
                &mut st,
            );
        } else if errs >= th.errors_warn_5m {
            let last = match s.last_error.as_deref() {
                Some(l) if !l.is_empty() => format!(" (last: {l})"),
                _ => String::new(),
            };
            bump(
                Status::Degrading,
                format!("{errs} errors in 5 min{last}"),
                &mut st,
            );
        }
        if s.health_says == Some(false) && st == Status::Healthy {
            bump(
                Status::Degrading,
                "galion-health reports not ok".into(),
                &mut st,
            );
        }
        if spec.kind == Kind::Host {
            if let (Some(t), Some(a)) = (s.disk_total_kb, s.disk_avail_kb) {
                let used = 100.0 - a as f64 * 100.0 / t.max(1) as f64;
                if used >= th.disk_crit_pct {
                    bump(
                        Status::FailingSoon,
                        format!("disk {used:.0}% used"),
                        &mut st,
                    );
                } else if used >= th.disk_warn_pct {
                    bump(Status::Degrading, format!("disk {used:.0}% used"), &mut st);
                }
                let dp: Vec<(f64, f64)> = self
                    .hist
                    .iter()
                    .filter(|x| x.t + 3600 >= now)
                    .filter_map(|x| Some((x.t as f64, x.disk_avail_kb? as f64)))
                    .collect();
                if dp.len() >= 10 {
                    if let Some(k) = slope(&dp) {
                        if k < 0.0 {
                            let eta = a as f64 / -k;
                            if eta < th.eta_disk_s {
                                bump(
                                    Status::FailingSoon,
                                    format!("disk full in ~{:.1} h at this rate", eta / 3600.0),
                                    &mut st,
                                );
                            }
                        }
                    }
                }
            }
            if let (Some(t), Some(a)) = (s.ram_total_kb, s.ram_avail_kb) {
                let av = a as f64 * 100.0 / t.max(1) as f64;
                if av < th.ram_crit_avail_pct {
                    bump(
                        Status::FailingSoon,
                        format!("RAM available {av:.0}%"),
                        &mut st,
                    );
                } else if av < th.ram_warn_avail_pct {
                    bump(
                        Status::Degrading,
                        format!("RAM available {av:.0}%"),
                        &mut st,
                    );
                }
            }
        }
        if let Some(e) = s.extra.get("degraded").and_then(Value::as_str) {
            bump(Status::Degrading, e.to_string(), &mut st);
        }
        (st, why, hard)
    }

    pub fn to_json(&self, now: u64) -> Value {
        let s = self.latest().cloned().unwrap_or_default();
        let sp = &self.spec;
        let push_age = s.extra.get("push_age_s").cloned().unwrap_or(Value::Null);
        json!({
            "name": sp.name,
            "status": self.status.as_str(),
            "status_since": if self.status_since > 0 { Value::from(ts(self.status_since)) } else { Value::Null },
            "reasons": self.reasons,
            "gaia": {"function": sp.gaia, "system": sp.gaia_system},
            "unit": sp.unit,
            "unit_state": s.unit_active,
            "unit_sub": s.unit_sub,
            "enabled": s.file_state,
            "pid": if s.pid > 0 { Value::from(s.pid) } else { Value::Null },
            "port": sp.port,
            "http": s.http_code,
            "latency_ms": s.latency_ms,
            "mem_kb": s.mem_kb,
            "rss_kb": s.rss_kb,
            "mem_max_kb": s.mem_max_kb,
            "cpu_pct": s.cpu_pct,
            "restarts_1h": self.restarts_1h(now),
            "planned_restarts_1h": self.planned_restarts_1h(now),
            "n_restarts_systemd": s.n_restarts,
            "log_errors_cycle": s.journal_errors,
            "log_warnings_cycle": s.journal_warnings,
            "last_error": s.last_error,
            "pushed_errors_cycle": s.pushed_errors,
            "tunnel": if sp.kind == Kind::Tunnel { json!({"ha_connections": s.tunnel_conns, "requests": s.tunnel_requests, "request_errors": s.tunnel_req_errors}) } else { Value::Null },
            "host": if sp.kind == Kind::Host { json!({"disk_total_kb": s.disk_total_kb, "disk_avail_kb": s.disk_avail_kb, "ram_total_kb": s.ram_total_kb, "ram_avail_kb": s.ram_avail_kb, "load1": s.load1}) } else { Value::Null },
            "galion_health_ok": s.health_says,
            "connection": if sp.kind == Kind::SelfNerve { Value::from("self") } else if push_age.is_null() { Value::from("pull") } else { Value::from("push+pull") },
            "push_age_s": push_age,
            "reflex_restarts_per_hour": sp.restarts_per_hour,
            "consecutive_down": self.consecutive_down,
            "samples": self.hist.len(),
            "note": sp.note,
            "extra": s.extra,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_specs;

    fn spec(name: &str) -> Spec {
        default_specs(None, None)
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
    }

    fn ok(t: u64) -> Sample {
        Sample {
            t,
            present: true,
            unit_active: Some("active".into()),
            http_code: Some(200),
            latency_ms: Some(2),
            pid: 10,
            mem_kb: Some(4000),
            mem_max_kb: Some(100_000),
            ..Default::default()
        }
    }

    #[test]
    fn healthy_then_failing_counts_down_cycles() {
        let th = Thresholds::default();
        let mut c = CompState::new(spec("hermes-rs"));
        c.push(ok(100), &th);
        assert_eq!(c.status, Status::Healthy, "{:?}", c.reasons);
        for i in 1..=3 {
            let mut s = ok(100 + i * 30);
            s.http_code = None;
            s.probe_error = Some("connect: refused".into());
            c.push(s, &th);
        }
        assert_eq!(c.status, Status::Failing);
        assert_eq!(c.consecutive_down, 3);
        c.push(ok(300), &th);
        assert_eq!(c.consecutive_down, 0);
    }

    #[test]
    fn restarts_make_failing_soon() {
        let th = Thresholds::default();
        let mut c = CompState::new(spec("tuner"));
        for i in 0..5u64 {
            let mut s = ok(1000 + i * 30);
            s.pid = 10 + i as u32;
            c.push(s, &th);
        }
        assert_eq!(c.restarts_1h(1200), 4);
        assert_eq!(c.status, Status::FailingSoon);
        // An hour later they age out.
        let mut s = ok(1000 + 4 * 30 + 3700);
        s.pid = 14;
        c.push(s, &th);
        assert_eq!(c.restarts_1h(1000 + 4 * 30 + 3700), 0);
        assert_eq!(c.status, Status::Healthy, "{:?}", c.reasons);
    }

    /// Slot 310: restarts that follow a journal stop job are planned and do
    /// not degrade; a restart without one still does; one stop explains one
    /// restart; two stops in the same second explain two.
    #[test]
    fn planned_restarts_do_not_degrade() {
        let th = Thresholds::default();
        let mut c = CompState::new(spec("tuner"));
        c.push_planned(ok(1000), &th, &[], &[]);
        // operator restart at 1010, cortex restart at 1012 (VPS 17:25:04 / 17:25:06 shape)
        let stops = vec![1010, 1012];
        let mut s = ok(1030);
        s.pid = 11;
        c.push_planned(s, &th, &stops, &[]);
        assert_eq!((c.restarts_1h(1030), c.planned_restarts_1h(1030)), (0, 1));
        assert_eq!(c.status, Status::Healthy, "{:?}", c.reasons);
        let mut s = ok(1060);
        s.pid = 12;
        s.n_restarts = 2; // counter says 2 restarts in this window
        c.push_planned(s, &th, &[1010, 1012, 1050, 1050], &[]);
        assert_eq!((c.restarts_1h(1060), c.planned_restarts_1h(1060)), (0, 3));
        assert_eq!(c.to_json(1060)["planned_restarts_1h"], 3);
        // a crash: no stop job in the window -> counted, degrading
        let mut s = ok(1090);
        s.pid = 13;
        c.push_planned(s, &th, &[1010, 1012, 1050, 1050], &[]);
        assert_eq!((c.restarts_1h(1090), c.planned_restarts_1h(1090)), (1, 3));
        assert_eq!(c.status, Status::Degrading, "{:?}", c.reasons);
        // a stop job older than the previous sample (minus PLANNED_SLACK_S) never
        // explains a later restart
        let mut s = ok(1200);
        s.pid = 14;
        c.push_planned(s, &th, &[1010, 1012, 1050, 1050, 1080], &[]);
        assert_eq!(c.restarts_1h(1200), 2);
        // crash wins: a stop job and a crash of the new invocation in the same
        // window (VPS hermes-rs 2026-10-27 19:52 CET shape) -> counted
        let mut s = ok(1230);
        s.pid = 15;
        c.push_planned(s, &th, &[1220], &[1220]);
        assert_eq!((c.restarts_1h(1230), c.planned_restarts_1h(1230)), (3, 3));
        // a crash outside the window does not cancel a later planned restart
        let mut s = ok(1260);
        s.pid = 16;
        c.push_planned(s, &th, &[1220, 1250], &[1220]);
        assert_eq!((c.restarts_1h(1260), c.planned_restarts_1h(1260)), (3, 4));
    }

    #[test]
    fn rising_rss_degrades_and_projects() {
        let th = Thresholds::default();
        let mut c = CompState::new(spec("openmind-memory"));
        for i in 0..20u64 {
            let mut s = ok(i * 30);
            s.mem_kb = Some(20_000 + i * 2_000);
            c.push(s, &th);
        }
        assert_eq!(c.status, Status::FailingSoon, "{:?}", c.reasons);
        assert!(c.reasons.iter().any(|r| r.contains("memory rising")));
        assert!(c.reasons.iter().any(|r| r.contains("limit reached")));
        assert!(!c.hard_down);
    }

    #[test]
    fn latency_spike_degrades() {
        let th = Thresholds::default();
        let mut c = CompState::new(spec("tuner"));
        for i in 0..10u64 {
            let mut s = ok(i * 30);
            s.latency_ms = Some(if i >= 7 { 900 } else { 3 });
            c.push(s, &th);
        }
        assert_eq!(c.status, Status::Degrading, "{:?}", c.reasons);
    }

    #[test]
    fn absent_and_noncritical() {
        let th = Thresholds::default();
        let mut v = CompState::new(spec("voice-worker"));
        v.push(
            Sample {
                t: 1,
                present: false,
                ..Default::default()
            },
            &th,
        );
        assert_eq!(v.status, Status::Absent);
        let mut m = CompState::new(spec("galion-one-mouth"));
        let mut s = ok(1);
        s.http_code = Some(503);
        m.push(s, &th);
        assert_eq!(m.status, Status::Degrading);
        assert!(!m.hard_down);
    }

    #[test]
    fn tunnel_and_host() {
        let th = Thresholds::default();
        let mut t = CompState::new(spec("cloudflared"));
        let mut s = ok(1);
        s.tunnel_conns = Some(0);
        t.push(s, &th);
        assert_eq!(t.status, Status::Failing);
        assert!(t.hard_down);
        let mut h = CompState::new(spec("lxc-host"));
        h.push(
            Sample {
                t: 1,
                present: true,
                disk_total_kb: Some(100),
                disk_avail_kb: Some(10),
                ram_total_kb: Some(100),
                ram_avail_kb: Some(50),
                ..Default::default()
            },
            &th,
        );
        assert_eq!(h.status, Status::Degrading, "{:?}", h.reasons);
    }

    #[test]
    fn tunnel_error_rate_degrades() {
        let th = Thresholds::default();
        let mut t = CompState::new(spec("cloudflared"));
        for i in 0..5u64 {
            let mut s = ok(i * 60);
            s.tunnel_conns = Some(4);
            s.tunnel_requests = Some(100 + i * 20);
            s.tunnel_req_errors = Some(10 + i * 10);
            t.push(s, &th);
        }
        assert_eq!(t.status, Status::Degrading, "{:?}", t.reasons);
        assert!(t.reasons[0].contains("40/80"));
    }

    #[test]
    fn journal_errors_degrade() {
        let th = Thresholds::default();
        let mut c = CompState::new(spec("console"));
        let mut s = ok(1);
        s.journal_errors = 4;
        s.last_error = Some("boom".into());
        c.push(s, &th);
        assert_eq!(c.status, Status::Degrading);
        assert!(c.reasons[0].contains("boom"));
    }
}

#[cfg(test)]
mod tiny_tests {
    use super::*;
    use crate::config::default_specs;

    #[test]
    fn tiny_rss_growth_is_not_a_leak() {
        let th = Thresholds::default();
        let spec = default_specs(None, None)
            .into_iter()
            .find(|s| s.name == "galion-nerve")
            .unwrap();
        let mut c = CompState::new(spec);
        for i in 0..20u64 {
            let s = Sample {
                t: i * 5,
                present: true,
                mem_kb: Some(3_000 + i * 100),
                extra: serde_json::json!({}),
                ..Default::default()
            };
            c.push(s, &th);
        }
        assert_eq!(c.status, Status::Healthy, "{:?}", c.reasons);
    }
}
