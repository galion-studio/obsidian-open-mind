// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 355 regression: the staged coding drop-in must pass the same path
//! rules the gateway enforces at start-up. Before slot 355 the staged
//! worktree (/var/lib/hermes-rs/work/openmind) sat under the denied /var/lib,
//! so it could never start; the same mistake crash-looped live hermes-rs on
//! 2026-10-27 19:45 CET. Pure path checks, nothing is created.

use hermes_tools::config::{branch_ok, worktree_path_ok};
use std::path::{Path, PathBuf};

const DROPIN: &str = include_str!("../../../deploy/staged/hermes-rs.service.d/coding-tools.conf");
const NOLOOP: &str =
    include_str!("../../../deploy/staged/hermes-rs.service.d/40-config-no-loop.conf");
const UNIT: &str = include_str!("../../../deploy/staged/hermes-rs.service");

fn env_of(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| *k == key)
        .map(|(_, v)| v.trim().to_string())
        .next_back()
}

fn rw_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("ReadWritePaths="))
        .flat_map(|v| v.split_whitespace())
        .map(|p| PathBuf::from(p.trim_start_matches(['-', '+'])))
        .collect()
}

fn writable(p: &Path, rw: &[PathBuf]) -> bool {
    rw.iter().any(|r| p.starts_with(r))
}

#[test]
fn staged_worktree_passes_the_deny_list() {
    let wt = env_of(DROPIN, "HERMES_RS_CODING_WORKTREE").expect("worktree set");
    worktree_path_ok(Path::new(&wt), &[]).unwrap_or_else(|e| panic!("staged worktree: {e}"));
    let br = env_of(DROPIN, "HERMES_RS_CODING_BRANCH").expect("branch set");
    branch_ok(&br).unwrap();
}

#[test]
fn staged_paths_are_writable_inside_the_unit() {
    let mut rw = rw_paths(UNIT);
    rw.extend(rw_paths(DROPIN));
    let wt = PathBuf::from(env_of(DROPIN, "HERMES_RS_CODING_WORKTREE").unwrap());
    let st = PathBuf::from(env_of(DROPIN, "HERMES_RS_CODING_STATE").unwrap());
    assert!(
        writable(&wt, &rw),
        "worktree {wt:?} not in ReadWritePaths {rw:?}"
    );
    assert!(
        writable(&st, &rw),
        "state {st:?} not in ReadWritePaths {rw:?}"
    );
    assert!(!st.starts_with(&wt), "state must live outside the worktree");
}

#[test]
fn old_var_lib_values_are_refused() {
    for p in [
        "/var/lib/hermes-rs/work/openmind",
        "/var/lib/hermes-rs/coding-worktree",
    ] {
        let e = worktree_path_ok(Path::new(p), &[]).unwrap_err();
        assert!(e.contains("/var/lib"), "{e}");
    }
}

#[test]
fn no_loop_dropin_stops_on_exit_2_only() {
    let v: Vec<&str> = NOLOOP
        .lines()
        .filter_map(|l| l.trim().strip_prefix("RestartPreventExitStatus="))
        .collect();
    assert_eq!(v, ["2"]);
    assert!(NOLOOP.contains("[Service]"));
}

// ---- Slot 370: start limit + restart back-off (45-start-limit.conf) -------

const STARTLIMIT: &str =
    include_str!("../../../deploy/staged/hermes-rs.service.d/45-start-limit.conf");
const ONERROR: &str =
    include_str!("../../../deploy/staged/hermes-rs.service.d/41-coding-on-error.conf");

/// Last `Key=` value inside `[section]` (systemd: later assignments win).
fn key_in(text: &str, section: &str, key: &str) -> Option<String> {
    let mut cur = String::new();
    let mut out = None;
    for l in text.lines().map(str::trim) {
        if l.starts_with('[') && l.ends_with(']') {
            cur = l.to_string();
        } else if cur == section {
            if let Some(v) = l.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
                out = Some(v.trim().to_string());
            }
        }
    }
    out
}

fn secs(v: &str) -> f64 {
    let v = v.trim();
    if let Some(n) = v.strip_suffix("ms") {
        n.parse::<f64>().unwrap() / 1000.0
    } else if let Some(n) = v.strip_suffix("min") {
        n.parse::<f64>().unwrap() * 60.0
    } else {
        v.trim_end_matches('s').parse().unwrap()
    }
}

/// systemd 255 restart policy, as in src/core/service.c
/// (service_restart_usec_next) and the unit start rate limit.
#[derive(Clone, Copy, Debug)]
struct Policy {
    restart_s: f64,
    steps: u32,
    max_delay_s: f64,
    burst: u32,
    interval_s: f64,
}

impl Policy {
    /// Delay before the n-th automatic restart (n = 1, 2, ...).
    fn delay(&self, n: u32) -> f64 {
        if n <= 1 || self.steps == 0 || self.restart_s >= self.max_delay_s {
            return self.restart_s;
        }
        if n > self.steps {
            return self.max_delay_s;
        }
        self.restart_s
            * (self.max_delay_s / self.restart_s).powf(f64::from(n - 1) / f64::from(self.steps))
    }

    /// Start times of a service that fails at once on every start, until the
    /// start limit refuses a start or `horizon_s` passes. Returns
    /// (start times, refused at).
    fn run(&self, horizon_s: f64) -> (Vec<f64>, Option<f64>) {
        let mut starts: Vec<f64> = vec![0.0];
        let mut t = 0.0;
        let mut n = 0;
        loop {
            n += 1;
            t += self.delay(n);
            if t > horizon_s {
                return (starts, None);
            }
            let in_window = starts.iter().filter(|s| t - **s < self.interval_s).count() as u32;
            if in_window >= self.burst {
                return (starts, Some(t));
            }
            starts.push(t);
        }
    }
}

fn live_like() -> Policy {
    // Base unit values + systemd defaults (DefaultStartLimitIntervalSec=10s,
    // DefaultStartLimitBurst=5; VPS `systemctl show hermes-rs` 2026-10-28
    // 06:21 CET: StartLimitIntervalUSec=10s StartLimitBurst=5 RestartUSec=3s).
    Policy {
        restart_s: secs(&key_in(UNIT, "[Service]", "RestartSec").unwrap()),
        steps: 0,
        max_delay_s: f64::INFINITY,
        burst: 5,
        interval_s: 10.0,
    }
}

fn staged() -> Policy {
    let mut p = live_like();
    p.steps = key_in(STARTLIMIT, "[Service]", "RestartSteps")
        .unwrap()
        .parse()
        .unwrap();
    p.max_delay_s = secs(&key_in(STARTLIMIT, "[Service]", "RestartMaxDelaySec").unwrap());
    p.burst = key_in(STARTLIMIT, "[Unit]", "StartLimitBurst")
        .unwrap()
        .parse()
        .unwrap();
    p.interval_s = secs(&key_in(STARTLIMIT, "[Unit]", "StartLimitIntervalSec").unwrap());
    p
}

#[test]
fn live_start_limit_never_trips() {
    // Reproduces 2026-10-27 19:45 CET: >= 13 starts in 46 s, never refused.
    let (starts, refused) = live_like().run(600.0);
    assert!(refused.is_none(), "live limit tripped at {refused:?}");
    let in_46 = starts.iter().filter(|t| **t <= 46.0).count();
    assert!(in_46 >= 13, "{in_46} starts in 46 s");
}

#[test]
fn staged_start_limit_trips_within_its_window() {
    let p = staged();
    let (starts, refused) = p.run(3600.0);
    let at = refused.expect("staged limit must trip");
    assert_eq!(starts.len() as u32, p.burst, "{starts:?}");
    assert!(
        at < p.interval_s,
        "refused at {at} s, window {}",
        p.interval_s
    );
    // Still a few real retries for a short hiccup (bind busy etc.).
    assert!(
        starts.len() >= 4 && starts[starts.len() - 1] >= 30.0,
        "{starts:?}"
    );
    // Back-off grows to the max and never beyond it.
    assert!((p.delay(1) - p.restart_s).abs() < 1e-9);
    assert!((p.delay(p.steps + 1) - p.max_delay_s).abs() < 1e-9);
    for n in 1..10 {
        assert!(p.delay(n + 1) >= p.delay(n) && p.delay(n) <= p.max_delay_s);
    }
}

#[test]
fn start_limit_keys_sit_in_the_right_sections() {
    // systemd 255 on the VPS (verify, 2026-10-28 06:24 CET): StartLimitIntervalSec=
    // in [Service] is "Unknown key ... ignoring" (only the legacy StartLimitBurst=
    // is still accepted there), which would silently keep the 10 s default.
    for k in ["StartLimitIntervalSec", "StartLimitBurst"] {
        assert!(
            key_in(STARTLIMIT, "[Unit]", k).is_some(),
            "{k} not in [Unit]"
        );
        assert!(
            key_in(STARTLIMIT, "[Service]", k).is_none(),
            "{k} in [Service]"
        );
    }
    for k in ["RestartSteps", "RestartMaxDelaySec"] {
        assert!(
            key_in(STARTLIMIT, "[Service]", k).is_some(),
            "{k} not in [Service]"
        );
    }
    // No StartLimitAction=reboot or similar surprises.
    assert!(!STARTLIMIT.contains("StartLimitAction"));
    // Restart policy itself stays the base unit's.
    assert!(key_in(STARTLIMIT, "[Service]", "Restart").is_none());
}

#[test]
fn on_error_option_dropin_matches_the_code_value() {
    assert_eq!(
        env_of(ONERROR, "HERMES_RS_CODING_ON_ERROR").as_deref(),
        Some("disable")
    );
    // Same value as the coding drop-in, so both paths behave the same.
    assert_eq!(
        env_of(ONERROR, "HERMES_RS_CODING_ON_ERROR"),
        env_of(DROPIN, "HERMES_RS_CODING_ON_ERROR")
    );
    assert!(ONERROR.contains("[Service]"));
}
