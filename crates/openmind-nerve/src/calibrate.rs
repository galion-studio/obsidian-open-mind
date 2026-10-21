// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Threshold calibration from saved real capture windows (cortex plan T3.1).
//!
//! A replay ([`crate::replay`]) judges a capture only at its 2-3 snapshot
//! times. The capture's journal covers about 30 min, so this module slides
//! over the whole journal in 10 s steps and measures, at every step, what the
//! threshold-bearing sensors would see:
//!
//! | sensor | measured per step | window |
//! |---|---|---|
//! | `sshd` (T2.1, B41 4.1/4.2) | every [`Counts`] key, through the real [`SshAuth`] | 300 s |
//! | `edge-rate` (T2.12) | per-hostname failed-request rate and the access.rs median/MAD threshold, through the real [`EdgeHosts`] + [`Detector`] | 60 s rate, 28 min baseline |
//! | `edge` (dead origins) | failed requests/min on refused origin dials | 300 s |
//! | cloudflared error rate | journal error lines ([`journal_counts`]) | 300 s |
//! | `components` memory-heavy | `mem_kb` per service from the state snapshots | snapshot |
//!
//! Output holds counts only: no addresses, account names or hostnames
//! (hostname streams are reported as a count). Used by
//! `openmind-nerve calibrate DIR...` and `tests/calibration.rs`; the rules
//! (tighten only, never loosen in code) are in docs/cortex/calibration.md.

use crate::access::{DdosParams, Detector};
use crate::logscan::insights;
use crate::probe::{journal_counts, parse_journal_lines, JLine};
use crate::sensors::edgerate::{EdgeHosts, PREFIX};
use crate::sensors::sshd::{Counts, SshAuth, WINDOW_S};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Step between evaluations (the detector's bucket size).
pub const STEP_S: u64 = 10;

/// One threshold T3.1 looked at: its design value, the value in code now
/// and the measured basis. Every threshold here fires at `value >= line`
/// (or `>`), so tightening means a lower number; `tests/calibration.rs`
/// fails if `current > original` (never loosen in code).
#[derive(Debug, Clone, Copy)]
pub struct Calibrated {
    pub name: &'static str,
    pub original: f64,
    pub current: f64,
    pub basis: &'static str,
}

/// Thresholds calibrated (or deliberately kept) by T3.1.
pub const CALIBRATED: &[Calibrated] = &[
    Calibrated {
        name: "sshd invalid_user warn (per 300 s)",
        original: 30.0,
        current: crate::sensors::sshd::INVALID_USER_WARN as f64,
        basis: "tightened: highest real 300 s window 8 (replay179), 788 windows",
    },
    Calibrated {
        name: "components memory-heavy (KiB)",
        original: (512 * 1024) as f64,
        current: crate::sensors::components::MEMORY_HEAVY_KIB as f64,
        basis: "tightened: console 600 MiB-1.27 GiB, every other service <= 29.8 MiB",
    },
    Calibrated {
        name: "edge-rate per-host floor (req/s, NERVE_DDOS_MIN_RPS)",
        original: 10.0,
        current: 10.0,
        basis: "kept: a floor of 8 or lower turns one real dead-origin burst (replay158) into 2 episodes (a flap)",
    },
];

/// Candidate per-hostname floors (req/s) checked against the edge streams.
pub const EDGE_FLOORS: [f64; 6] = [10.0, 8.0, 6.0, 5.0, 4.0, 3.0];

/// max / p95 / p50 of one measured series (over all steps).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stat {
    pub n: usize,
    pub max: f64,
    pub p95: f64,
    pub p50: f64,
    pub min: f64,
}

impl Stat {
    pub fn of(v: &[f64]) -> Stat {
        if v.is_empty() {
            return Stat::default();
        }
        let mut s = v.to_vec();
        s.sort_by(|a, b| a.total_cmp(b));
        let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
        Stat {
            n: s.len(),
            max: s[s.len() - 1],
            p95: q(0.95),
            p50: q(0.5),
            min: s[0],
        }
    }

    pub fn to_json(&self) -> Value {
        let r = |x: f64| (x * 100.0).round() / 100.0;
        json!({"n": self.n, "max": r(self.max), "p95": r(self.p95), "p50": r(self.p50), "min": r(self.min)})
    }
}

/// Episodes (onsets) of a boolean series: how often it switches on.
pub fn episodes(v: &[bool]) -> usize {
    let mut n = 0;
    let mut prev = false;
    for &b in v {
        if b && !prev {
            n += 1;
        }
        prev = b;
    }
    n
}

/// Per-floor result for the edge host streams.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloorResult {
    pub floor: f64,
    /// Steps where any host stream is elevated or flooding with this floor.
    pub hot_steps: usize,
    pub flood_steps: usize,
    /// Onsets of "any host stream hot".
    pub episodes: usize,
}

/// Everything measured on one capture.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub name: String,
    pub journal_lines: usize,
    /// Journal span in seconds (first .. last stamped line).
    pub span_s: u64,
    pub steps: usize,
    pub ssh_lines: usize,
    pub sshd: BTreeMap<&'static str, Stat>,
    pub edge_host_streams: usize,
    /// Highest 60 s failed-request rate of any host stream, req/s.
    pub edge_rps: Stat,
    /// Highest requests_60s of any host stream.
    pub edge_req_60s: Stat,
    /// Data-driven part of the threshold (5 x median, median + 8 x 1.4826
    /// x MAD), at the step and stream with the highest rate.
    pub edge_data_threshold_at_peak: f64,
    pub edge_floors: Vec<FloorResult>,
    /// Failed requests/min on refused dials over the last 300 s.
    pub dead_origin_per_min: Stat,
    /// cloudflared journal error lines in the last 300 s.
    pub cloudflared_errors_5m: Stat,
    /// Largest `mem_kb` per service over the snapshots (KiB), by component.
    pub mem_kb: BTreeMap<String, u64>,
}

fn sshd_keys(c: &Counts) -> [(&'static str, u64); 8] {
    [
        ("failed_password", c.failed_password),
        ("invalid_user", c.invalid_user),
        ("accepted_password", c.accepted_password),
        ("accepted_publickey_root", c.accepted_publickey_root),
        ("accepted_publickey_other", c.accepted_publickey_other),
        ("max_auth_attempts", c.max_auth_attempts),
        ("distinct_fail_sources", c.distinct_fail_sources),
        ("preauth_other", c.preauth_other),
    ]
}

/// Measure one capture directory (`journal.jsonl` + `state-*.json`).
pub fn calibrate_dir(name: &str, dir: &Path) -> Result<Report, String> {
    let read = |n: &str| std::fs::read_to_string(dir.join(n));
    let journal = read("journal.jsonl").unwrap_or_default();
    let mut lines: Vec<JLine> = parse_journal_lines(&journal, &[])
        .into_iter()
        .filter(|l| l.t > 0)
        .collect();
    lines.sort_by_key(|l| l.t);
    let mut r = Report {
        name: name.to_string(),
        journal_lines: lines.len(),
        ..Default::default()
    };
    r.mem_kb = snapshot_mem(dir)?;
    let (Some(first), Some(last)) = (lines.first().map(|l| l.t), lines.last().map(|l| l.t)) else {
        return Ok(r);
    };
    r.span_s = last - first;
    r.ssh_lines = lines
        .iter()
        .filter(|l| {
            l.unit == "ssh.service" || l.unit == "sshd.service" || l.unit.starts_with("sshd@")
        })
        .count();

    let mut ssh = SshAuth::new(vec!["ssh.service".into(), "sshd.service".into()], None);
    let mut hosts = EdgeHosts::default();
    let mut det = Detector {
        params: DdosParams::default(),
        ..Default::default()
    };
    let mut sshd_series: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    let (mut rps, mut req) = (vec![], vec![]);
    let mut peak = (0.0f64, 0.0f64);
    let mut floor_hot: Vec<Vec<bool>> = vec![vec![]; EDGE_FLOORS.len()];
    let mut floor_flood = vec![0usize; EDGE_FLOORS.len()];
    let (mut dead, mut cf_err) = (vec![], vec![]);
    let logs = crate::logscan::LogInsights::default();
    let mut fed = 0usize;
    // Steps start at the first line (the edge detector needs the whole
    // journal; with less than 60 s of baseline its threshold is the floor,
    // the most sensitive case). 300 s window series are recorded only once
    // the window is whole.
    let mut now = first;
    now -= now % STEP_S;
    while now <= last + STEP_S {
        let upto = lines.partition_point(|l| l.t <= now);
        let new = &lines[fed..upto];
        fed = upto;
        // sshd through the real sensor (it keeps its own 300 s window).
        let mut i = crate::cortex::Input::new(now, &[], &logs);
        i.journal = new;
        let c = ssh.observe(&i);
        let whole = now >= first + WINDOW_S;
        if whole {
            for (k, v) in sshd_keys(&c) {
                sshd_series.entry(k).or_default().push(v as f64);
            }
        }
        // edge host streams through the real feeder + detector.
        hosts.feed(new, now, &mut det);
        let vs: Vec<_> = det
            .evaluate(now)
            .into_iter()
            .filter(|v| v.stream.starts_with(PREFIX))
            .collect();
        let top = vs.iter().max_by(|a, b| a.rps.total_cmp(&b.rps));
        rps.push(top.map(|v| v.rps).unwrap_or(0.0));
        req.push(top.map(|v| v.requests_60s as f64).unwrap_or(0.0));
        if let Some(v) = top {
            if v.rps > peak.0 {
                let data = (v.baseline_rps * 5.0).max(v.baseline_rps + 8.0 * 1.4826 * v.mad_rps);
                peak = (v.rps, data);
            }
        }
        for (k, floor) in EDGE_FLOORS.iter().enumerate() {
            let p = DdosParams {
                min_rps: *floor,
                ..DdosParams::default()
            };
            let mut hot = false;
            for s in det
                .streams
                .values_mut()
                .filter(|s| s.name.starts_with(PREFIX))
            {
                // `evaluate` only updates peak/flood bookkeeping; a clone keeps
                // the main stream untouched.
                let v = s.clone().evaluate(now, &p);
                if v.level != "normal" {
                    hot = true;
                }
                if v.level == "flood" {
                    floor_flood[k] += 1;
                }
            }
            floor_hot[k].push(hot);
        }
        // dead origins and cloudflared errors over the last 300 s.
        let from = lines.partition_point(|l| l.t + WINDOW_S <= now);
        let win = &lines[from..upto];
        let li = insights(win);
        let failed: u32 = li.dead_origins.values().map(|d| d.failed_requests).sum();
        if whole {
            dead.push(failed as f64 * 60.0 / WINDOW_S as f64);
        }
        if whole {
            let jc = journal_counts(win, &["cloudflared.service"]);
            cf_err.push(jc.get("cloudflared.service").map(|j| j.errors).unwrap_or(0) as f64);
        }
        r.steps += 1;
        now += STEP_S;
    }
    r.sshd = sshd_series.iter().map(|(k, v)| (*k, Stat::of(v))).collect();
    r.edge_host_streams = hosts.hosts();
    r.edge_rps = Stat::of(&rps);
    r.edge_req_60s = Stat::of(&req);
    r.edge_data_threshold_at_peak = peak.1;
    r.edge_floors = EDGE_FLOORS
        .iter()
        .enumerate()
        .map(|(k, f)| FloorResult {
            floor: *f,
            hot_steps: floor_hot[k].iter().filter(|b| **b).count(),
            flood_steps: floor_flood[k],
            episodes: episodes(&floor_hot[k]),
        })
        .collect();
    r.dead_origin_per_min = Stat::of(&dead);
    r.cloudflared_errors_5m = Stat::of(&cf_err);
    Ok(r)
}

/// Largest `mem_kb` per component over the `state-*.json` snapshots.
fn snapshot_mem(dir: &Path) -> Result<BTreeMap<String, u64>, String> {
    let mut m = BTreeMap::new();
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("state") && n.ends_with(".json"))
        })
        .collect();
    names.sort();
    for p in names {
        let Ok(t) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&t) else {
            continue;
        };
        for c in v["components"].as_array().into_iter().flatten() {
            if let (Some(n), Some(kb)) = (c["name"].as_str(), c["mem_kb"].as_u64()) {
                let e = m.entry(n.to_string()).or_insert(0);
                *e = (*e).max(kb);
            }
        }
    }
    Ok(m)
}

impl Report {
    pub fn to_json(&self) -> Value {
        let r2 = |x: f64| (x * 100.0).round() / 100.0;
        json!({
            "name": self.name,
            "journal_lines": self.journal_lines,
            "span_s": self.span_s,
            "steps": self.steps,
            "ssh_lines": self.ssh_lines,
            "sshd_300s": self.sshd.iter().map(|(k, s)| (k.to_string(), s.to_json())).collect::<serde_json::Map<_, _>>(),
            "edge": {
                "host_streams": self.edge_host_streams,
                "rps_60s": self.edge_rps.to_json(),
                "requests_60s": self.edge_req_60s.to_json(),
                "data_threshold_at_peak": r2(self.edge_data_threshold_at_peak),
                "floors": self.edge_floors.iter().map(|f| json!({
                    "min_rps": f.floor, "hot_steps": f.hot_steps, "flood_steps": f.flood_steps, "episodes": f.episodes
                })).collect::<Vec<_>>(),
            },
            "dead_origin_failed_per_min_300s": self.dead_origin_per_min.to_json(),
            "cloudflared_errors_300s": self.cloudflared_errors_5m.to_json(),
            "mem_kb_max": self.mem_kb,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrated_thresholds_only_tighten() {
        for c in CALIBRATED {
            assert!(
                c.current <= c.original,
                "{} loosened in code: {} -> {}",
                c.name,
                c.original,
                c.current
            );
            assert!(c.current > 0.0, "{}", c.name);
        }
        assert_eq!(crate::sensors::sshd::INVALID_USER_WARN, 20);
        assert_eq!(crate::sensors::components::MEMORY_HEAVY_KIB, 256 * 1024);
        assert_eq!(DdosParams::default().min_rps, 10.0);
    }

    #[test]
    fn stat_and_episodes() {
        let s = Stat::of(&[0.0, 1.0, 2.0, 3.0, 10.0]);
        assert_eq!((s.n, s.max, s.p50, s.min), (5, 10.0, 2.0, 0.0));
        assert_eq!(Stat::of(&[]), Stat::default());
        assert_eq!(episodes(&[false, true, true, false, true, false]), 2);
        assert_eq!(episodes(&[true, true]), 1);
        assert_eq!(episodes(&[]), 0);
    }

    #[test]
    fn measures_a_synthetic_window() {
        let d = std::env::temp_dir().join(format!("calib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let t0: u64 = 1_792_520_000;
        let mut j = String::new();
        for k in 0..600u64 {
            // one invalid user every 20 s; a 2-minute burst of failed edge
            // requests to one host (3 per second) at minute 12.
            if k % 20 == 0 {
                j.push_str(&format!(
                    "{{\"_SYSTEMD_UNIT\":\"ssh.service\",\"PRIORITY\":\"6\",\"MESSAGE\":\"Invalid user u from 198.51.100.7 port 4000\",\"__REALTIME_TIMESTAMP\":\"{}\"}}\n",
                    (t0 + k * 3) * 1_000_000
                ));
            }
        }
        for s in 720..840u64 {
            for _ in 0..3 {
                j.push_str(&format!(
                    "{{\"_SYSTEMD_UNIT\":\"cloudflared.service\",\"PRIORITY\":\"6\",\"MESSAGE\":\"ERR Request failed error=\\\"dial tcp [::1]:4001: connect: connection refused\\\" dest=https://a.example.test/x\",\"__REALTIME_TIMESTAMP\":\"{}\"}}\n",
                    (t0 + s) * 1_000_000
                ));
            }
        }
        std::fs::write(d.join("journal.jsonl"), j).unwrap();
        std::fs::write(
            d.join("state-1.json"),
            r#"{"ts":"2026-10-21T10:00:00+02:00","components":[{"name":"console","mem_kb":600000}]}"#,
        )
        .unwrap();
        let r = calibrate_dir("synthetic", &d).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        // 300 s window holds 300/60 = 5 invalid users (one per 60 s).
        assert_eq!(r.sshd["invalid_user"].max, 5.0, "{:?}", r.sshd);
        assert_eq!(r.sshd["failed_password"].max, 0.0);
        assert_eq!(r.edge_host_streams, 1);
        // rps = requests in the buckets touching the last 60 s / 60 (access.rs).
        assert!(
            r.edge_rps.max >= 3.0 && r.edge_rps.max <= 3.5,
            "{:?}",
            r.edge_rps
        );
        // 3 req/s: hot with a 5 or 6 req/s floor (elevated at half), not at 10.
        let f = |x: f64| r.edge_floors.iter().find(|f| f.floor == x).unwrap().clone();
        assert_eq!(f(10.0).hot_steps, 0);
        assert!(f(6.0).hot_steps > 0 && f(6.0).episodes == 1, "{:?}", f(6.0));
        assert_eq!(f(6.0).flood_steps, 0);
        // No flood at any floor: 3 req/s is under min_requests (300 per minute).
        assert!(f(3.0).hot_steps > 0 && f(3.0).flood_steps == 0, "{:?}", f(3.0));
        // 3 refused failures/s for 120 s: 360 in 300 s = 72/min at the peak.
        assert!(
            (r.dead_origin_per_min.max - 72.0).abs() < 1e-9,
            "{:?}",
            r.dead_origin_per_min
        );
        assert_eq!(r.mem_kb["console"], 600000);
        let js = r.to_json().to_string();
        assert!(
            !js.contains("198.51.100") && !js.contains("example.test"),
            "{js}"
        );
    }
}
