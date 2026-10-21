// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Reflex layer: threshold-triggered remediation with no human in the loop,
//! limited to a hard allowlist.
//!
//! Allowed: `systemctl restart --no-block <allowlisted unit>`, and rollback of
//! a recently deployed binary through `galion-deploy <svc> <previous binary>`
//! (ws/foundation; it backs up, installs, health-checks and auto-reverts).
//! Never: deleting data, cloudflared config or DNS, ssh, firewall, the
//! galion-control key, the tuner quick tunnel, the nerve itself. The
//! switch-day rollback scripts (`rollback*.sh`, "NOT run automatically") are
//! only named in alerts as the manual next step.
//! Limits: per-unit restarts per hour (3, cloudflared 1), max 2 actions per
//! cycle, 1 rollback per unit per 24 h, deploy-in-progress hold, dry-run
//! switch, and an audit line for every decision.

use crate::alerts::{Alerts, Severity};
use crate::state::{CompState, Status};
use crate::util::{append_line, now_secs, run, ts, write_atomic};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Units the reflex may restart. Anything else is refused.
pub const ALLOW: &[&str] = &[
    "openmind-memory.service",
    "hermes-rs.service",
    "link-watch-rs.service",
    "galion-desk.service",
    "tuner.service",
    "galion-one.service",
    "console.service",
    "galion-health.service",
    "cloudflared.service",
    "galion-voice.service",
];

/// Never touched, even if someone adds them to the allowlist by mistake.
pub const DENY: &[&str] = &[
    "ssh",
    "fail2ban",
    "ufw",
    "nftables",
    "iptables",
    "firewall",
    "galion-control",
    "systemd-",
    "dbus",
    "networkd",
    "resolved",
    "tuner-quick",
    "galion-nerve",
    "cron",
    "postfix",
];

pub const MAX_ACTIONS_PER_CYCLE: usize = 2;

/// `YYYYMMDD-HHMMSS+HHMM` (galion-deploy backup stamp, Zagreb offset).
pub fn is_backup_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(i, c)| match i {
            8 => *c == b'-',
            15 => *c == b'+' || *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

pub fn permitted(unit: &str) -> Result<(), &'static str> {
    if DENY.iter().any(|d| unit.contains(d)) {
        return Err("unit is on the never-touch list");
    }
    if !ALLOW.contains(&unit) {
        return Err("unit is not on the reflex allowlist");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    None,
    Restart {
        unit: String,
        reason: String,
    },
    Rollback {
        unit: String,
        backup: PathBuf,
        reason: String,
    },
    /// Wanted to act but a limit said no.
    Limited {
        unit: String,
        reason: String,
        hint: String,
    },
    /// Condition looks intentional or transient; alert only.
    Hold {
        unit: String,
        reason: String,
    },
}

pub struct Reflex {
    pub data_dir: PathBuf,
    pub env_dry: bool,
    pub rollback_enabled: bool,
    pub deploy_bin: PathBuf,
    pub deploy_backups: PathBuf,
    pub switch_dir: PathBuf,
    pub fail_cycles: u32,
    restarts: HashMap<String, VecDeque<u64>>,
    dry_restarts: HashMap<String, VecDeque<u64>>,
    rollbacks: HashMap<String, u64>,
    /// Dry-run rollbacks, kept apart like `dry_restarts` so a dry run obeys the
    /// same 1-per-unit-per-24 h limit the live run would (T3.2), in memory only.
    dry_rollbacks: HashMap<String, u64>,
    pub recent: VecDeque<Value>,
    pub persist: bool,
    /// Replay / audit runs: always dry-run, whatever NERVE_DRY_RUN or the
    /// `mode` file say, so nothing is ever executed.
    pub force_dry: bool,
    /// When set, every audit entry is also appended here (unbounded; replay only).
    pub log: Option<Vec<Value>>,
}

impl Reflex {
    pub fn new(data_dir: PathBuf, env_dry: bool, fail_cycles: u32) -> Self {
        let mut r = Reflex {
            data_dir,
            env_dry,
            rollback_enabled: true,
            deploy_bin: PathBuf::from("/opt/galion/rust/bin/galion-deploy"),
            deploy_backups: PathBuf::from("/var/backups/galion/deploy"),
            switch_dir: PathBuf::from("/root/galion-switch-20261018-2140"),
            fail_cycles,
            restarts: HashMap::new(),
            dry_restarts: HashMap::new(),
            rollbacks: HashMap::new(),
            dry_rollbacks: HashMap::new(),
            recent: VecDeque::new(),
            persist: true,
            force_dry: false,
            log: None,
        };
        r.load();
        r
    }

    /// `<data_dir>/mode` ("live" or "dry-run") overrides NERVE_DRY_RUN at runtime.
    pub fn dry_run(&self) -> bool {
        if self.force_dry {
            return true;
        }
        match std::fs::read_to_string(self.data_dir.join("mode")) {
            Ok(m) if m.trim() == "live" => false,
            Ok(m) if m.trim() == "dry-run" => true,
            _ => self.env_dry,
        }
    }

    fn state_path(&self) -> PathBuf {
        self.data_dir.join("reflex.json")
    }

    fn load(&mut self) {
        let Ok(s) = std::fs::read_to_string(self.state_path()) else {
            return;
        };
        let Ok(v) = serde_json::from_str::<Value>(&s) else {
            return;
        };
        if let Some(m) = v.get("restarts").and_then(Value::as_object) {
            for (k, arr) in m {
                let q: VecDeque<u64> = arr
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                    .collect();
                self.restarts.insert(k.clone(), q);
            }
        }
        if let Some(m) = v.get("rollbacks").and_then(Value::as_object) {
            for (k, t) in m {
                if let Some(t) = t.as_u64() {
                    self.rollbacks.insert(k.clone(), t);
                }
            }
        }
    }

    fn save(&self) {
        if !self.persist {
            return;
        }
        let v = json!({"restarts": self.restarts, "rollbacks": self.rollbacks});
        let _ = write_atomic(&self.state_path(), v.to_string().as_bytes());
    }

    pub fn used(&self, unit: &str, now: u64, dry: bool) -> usize {
        let m = if dry {
            &self.dry_restarts
        } else {
            &self.restarts
        };
        m.get(unit)
            .map(|q| q.iter().filter(|t| **t + 3600 > now).count())
            .unwrap_or(0)
    }

    fn newest_backup(&self, svc: &str, now: u64) -> Option<PathBuf> {
        let rd = std::fs::read_dir(&self.deploy_backups).ok()?;
        let pre = format!("{svc}-");
        let mut best: Option<(u64, PathBuf)> = None;
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            // Only `<svc>-<YYYYMMDD-HHMMSS+HHMM>.bin`, exactly what galion-deploy
            // writes: `tuner-` must not pick up a `tuner-quick-...bin` backup
            // (T3.2 audit: cross-unit rollback).
            let Some(stamp) = n.strip_prefix(&pre).and_then(|x| x.strip_suffix(".bin")) else {
                continue;
            };
            if !is_backup_stamp(stamp) {
                continue;
            }
            let Some(secs) = e
                .metadata()
                .ok()
                .filter(|m| m.is_file())
                .and_then(|m| m.modified().ok())
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
            else {
                continue;
            };
            if best.as_ref().is_none_or(|b| secs > b.0) {
                best = Some((secs, e.path()));
            }
        }
        // Only a deploy in the last 24 h makes "the new binary broke it" plausible.
        best.filter(|b| b.0 + 86400 > now).map(|b| b.1)
    }

    fn manual_hint(&self, svc: &str) -> String {
        let mut h = vec![];
        for f in ["rollback-gaps.sh", "rollback.sh"] {
            let p = self.switch_dir.join(f);
            if p.exists() {
                h.push(format!(
                    "{} (manual; switches back to the pre-switch stack)",
                    p.display()
                ));
            }
        }
        if self.deploy_bin.exists() {
            h.push(format!(
                "{} {svc} <known-good binary>",
                self.deploy_bin.display()
            ));
        }
        if h.is_empty() {
            "inspect journalctl -u the unit".into()
        } else {
            h.join("; ")
        }
    }

    /// Decide for one component. Pure apart from reading the backup dir.
    pub fn decide(&self, c: &CompState, now: u64, deploy_busy: bool) -> Decision {
        let Some(unit) = c.spec.unit else {
            return Decision::None;
        };
        if c.spec.restarts_per_hour == 0 {
            return Decision::None;
        }
        let Some(s) = c.latest() else {
            return Decision::None;
        };
        if !s.present || s.unit_active.is_none() {
            return Decision::None;
        }
        let mem_pressure = c.hist.len() >= 2
            && c.hist
                .iter()
                .rev()
                .take(2)
                .all(|x| match (x.mem_kb, x.mem_max_kb) {
                    (Some(m), Some(mx)) => m * 100 >= mx * 95,
                    _ => false,
                });
        let down = c.hard_down && c.consecutive_down >= self.fail_cycles;
        if !down && !mem_pressure {
            return Decision::None;
        }
        let reason = if down {
            format!(
                "{} for {} cycles: {}",
                c.status.as_str(),
                c.consecutive_down,
                c.reasons.join("; ")
            )
        } else {
            "memory >= 95% of MemoryMax for 2 cycles (pre-emptive restart)".to_string()
        };
        if let Err(e) = permitted(unit) {
            return Decision::Hold {
                unit: unit.into(),
                reason: format!("{reason}; not acting: {e}"),
            };
        }
        let active = s.unit_active.as_deref().unwrap_or("");
        if active == "inactive" && s.unit_result.as_deref() == Some("success") {
            return Decision::Hold {
                unit: unit.into(),
                reason: "stopped cleanly (operator stop?); not restarting".into(),
            };
        }
        if active == "inactive" && s.file_state.as_deref() != Some("enabled") {
            return Decision::Hold {
                unit: unit.into(),
                reason: "disabled and stopped (intentional switch?); not restarting".into(),
            };
        }
        if matches!(active, "activating" | "deactivating" | "reloading") {
            return Decision::Hold {
                unit: unit.into(),
                reason: format!("unit {active}; waiting"),
            };
        }
        if deploy_busy {
            return Decision::Hold {
                unit: unit.into(),
                reason: "a deploy/rollback is running; holding reflexes".into(),
            };
        }
        let dry = self.dry_run();
        if self.used(unit, now, dry) < c.spec.restarts_per_hour as usize {
            return Decision::Restart {
                unit: unit.into(),
                reason,
            };
        }
        let svc = unit.trim_end_matches(".service");
        if self.rollback_enabled
            && down
            && self.deploy_bin.exists()
            && (if dry {
                &self.dry_rollbacks
            } else {
                &self.rollbacks
            })
            .get(unit)
            .is_none_or(|t| *t + 86400 < now)
            && unit != "cloudflared.service"
        {
            if let Some(b) = self.newest_backup(svc, now) {
                return Decision::Rollback {
                    unit: unit.into(),
                    backup: b,
                    reason: format!("restart budget ({}/h) exhausted and still failing after a deploy in the last 24 h", c.spec.restarts_per_hour),
                };
            }
        }
        Decision::Limited {
            unit: unit.into(),
            reason: format!(
                "restart budget {}/h exhausted; reflex stopped for this unit",
                c.spec.restarts_per_hour
            ),
            hint: self.manual_hint(svc),
        }
    }

    fn audit(&mut self, entry: Value) {
        if self.persist {
            let _ = append_line(&self.data_dir.join("audit.jsonl"), &entry.to_string());
        }
        if self.recent.len() >= 50 {
            self.recent.pop_front();
        }
        if let Some(l) = self.log.as_mut() {
            l.push(entry.clone());
        }
        self.recent.push_back(entry);
    }

    /// Carry out one decision (or log what would be done in dry-run).
    pub fn execute(&mut self, comp: &str, d: &Decision, alerts: &mut Alerts) -> bool {
        self.execute_at(comp, d, alerts, now_secs())
    }

    /// [`Reflex::execute`] at an explicit time: the engine passes its cycle time
    /// so a replay keeps the restart budget on the capture clock (T3.2).
    pub fn execute_at(&mut self, comp: &str, d: &Decision, alerts: &mut Alerts, now: u64) -> bool {
        let dry = self.dry_run();
        let mode = if dry { "dry-run" } else { "live" };
        match d {
            Decision::None => false,
            Decision::Hold { unit, reason } => {
                if alerts.raise(
                    &format!("hold:{unit}"),
                    Severity::Warn,
                    comp,
                    &format!("needs attention, reflex holding: {reason}"),
                ) {
                    self.audit(json!({"ts": ts(now), "mode": mode, "action": "hold", "unit": unit, "component": comp, "reason": reason, "result": "no action"}));
                }
                false
            }
            Decision::Limited { unit, reason, hint } => {
                if alerts.raise(
                    &format!("limit:{unit}"),
                    Severity::Crit,
                    comp,
                    &format!("{reason}. Next step: {hint}"),
                ) {
                    self.audit(json!({"ts": ts(now), "mode": mode, "action": "rate-limited", "unit": unit, "component": comp, "reason": reason, "result": "stopped; alerted"}));
                }
                false
            }
            Decision::Restart { unit, reason } => {
                if permitted(unit).is_err() {
                    return false;
                }
                let (result, ok) = if dry {
                    (
                        "would run: systemctl restart --no-block ".to_string() + unit,
                        true,
                    )
                } else {
                    match run(
                        "systemctl",
                        &["restart", "--no-block", unit],
                        Duration::from_secs(30),
                    ) {
                        Some(o) if o.code == 0 => ("restart queued".to_string(), true),
                        Some(o) => (
                            format!("systemctl exit {}: {}", o.code, o.stderr.trim()),
                            false,
                        ),
                        None => ("systemctl timed out".to_string(), false),
                    }
                };
                let m = if dry {
                    &mut self.dry_restarts
                } else {
                    &mut self.restarts
                };
                m.entry(unit.clone()).or_default().push_back(now);
                for q in m.values_mut() {
                    while q.front().is_some_and(|t| *t + 3600 <= now) {
                        q.pop_front();
                    }
                }
                let used = self.used(unit, now, dry);
                self.audit(json!({"ts": ts(now), "mode": mode, "action": "restart", "unit": unit, "component": comp, "reason": reason, "result": result, "ok": ok, "budget_used_1h": used}));
                alerts.raise(
                    &format!("restart:{unit}:{now}"),
                    Severity::Warn,
                    comp,
                    &format!("[{mode}] restart {unit} ({used} this hour): {reason}"),
                );
                self.save();
                ok && !dry
            }
            Decision::Rollback {
                unit,
                backup,
                reason,
            } => {
                let svc = unit.trim_end_matches(".service");
                let deploy = self.deploy_bin.display().to_string();
                let b = backup.display().to_string();
                let (result, ok) = if dry {
                    (
                        format!("would run: {deploy} {svc} {b} (via systemd-run)"),
                        true,
                    )
                } else {
                    let name = format!("--unit=galion-nerve-rollback-{svc}-{now}");
                    match run(
                        "systemd-run",
                        &["--collect", "--quiet", &name, &deploy, svc, &b],
                        Duration::from_secs(30),
                    ) {
                        Some(o) if o.code == 0 => (format!("started {deploy} {svc} {b}"), true),
                        Some(o) => (
                            format!("systemd-run exit {}: {}", o.code, o.stderr.trim()),
                            false,
                        ),
                        None => ("systemd-run timed out".to_string(), false),
                    }
                };
                if dry {
                    self.dry_rollbacks.insert(unit.clone(), now);
                } else {
                    self.rollbacks.insert(unit.clone(), now);
                    self.save();
                }
                self.audit(json!({"ts": ts(now), "mode": mode, "action": "rollback", "unit": unit, "component": comp, "reason": reason, "result": result, "ok": ok}));
                alerts.raise(
                    &format!("rollback:{unit}"),
                    Severity::Crit,
                    comp,
                    &format!("[{mode}] rollback {unit} to {b}: {reason}"),
                );
                ok && !dry
            }
        }
    }

    pub fn budgets_json(&self, now: u64) -> Value {
        let mut m = serde_json::Map::new();
        for u in ALLOW {
            m.insert(
                u.to_string(),
                json!({"live_1h": self.used(u, now, false), "dry_1h": self.used(u, now, true), "last_rollback": self.rollbacks.get(*u).map(|t| ts(*t))}),
            );
        }
        Value::Object(m)
    }
}

/// True while galion-deploy or a rollback script is running.
pub fn deploy_in_progress() -> bool {
    !crate::probe::procs_matching(&[
        "galion-deploy ",
        "rollback.sh",
        "rollback-gaps.sh",
        "switch.sh",
    ])
    .is_empty()
}

pub fn status_needs_alert(c: &CompState) -> Option<Severity> {
    match c.status {
        Status::Failing if c.spec.critical => Some(Severity::Crit),
        Status::Failing | Status::FailingSoon => Some(Severity::Warn),
        _ => None,
    }
}

pub fn data_dir_ok(p: &Path) -> bool {
    std::fs::create_dir_all(p).is_ok() && append_line(&p.join(".probe"), "").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_specs;
    use crate::state::{Sample, Thresholds};

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nerve-reflex-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn down_comp(name: &str, cycles: u32, active: &str, result: &str, enabled: &str) -> CompState {
        let spec = default_specs(None, None)
            .into_iter()
            .find(|s| s.name == name)
            .unwrap();
        let mut c = CompState::new(spec);
        for i in 0..cycles {
            c.push(
                Sample {
                    t: 1000 + i as u64 * 30,
                    present: true,
                    unit_active: Some(active.into()),
                    unit_result: Some(result.into()),
                    file_state: Some(enabled.into()),
                    probe_error: Some("refused".into()),
                    ..Default::default()
                },
                &Thresholds::default(),
            );
        }
        c
    }

    fn quiet_alerts() -> Alerts {
        let mut a = Alerts::new(None, None, 0);
        a.to_journal = false;
        a
    }

    #[test]
    fn allow_and_deny() {
        assert!(permitted("hermes-rs.service").is_ok());
        assert!(permitted("ssh.service").is_err());
        assert!(permitted("sshd.service").is_err());
        assert!(permitted("tuner-quick.service").is_err());
        assert!(permitted("galion-nerve.service").is_err());
        assert!(permitted("galion-control.service").is_err());
        assert!(permitted("nftables.service").is_err());
        assert!(permitted("random.service").is_err());
        for u in ALLOW {
            assert!(!DENY.iter().any(|d| u.contains(d)), "{u}");
        }
    }

    #[test]
    fn t23_observe_only_specs_never_get_a_decision() {
        // Slot 200: the five T2.3 specs, hard-down for 10 cycles, with a
        // deploy backup and galion-deploy present (worst case): no decision.
        let d = tmp("t23");
        let bk = d.join("backups");
        std::fs::create_dir_all(&bk).unwrap();
        let dep = d.join("galion-deploy");
        std::fs::write(&dep, "").unwrap();
        let mut r = Reflex::new(d.clone(), true, 1);
        r.deploy_bin = dep;
        r.deploy_backups = bk.clone();
        for n in [
            "openmind-queue",
            "tanga-proto",
            "galion-nerve-shadow",
            "galion-backup",
            "hades-gate",
        ] {
            std::fs::write(bk.join(format!("{n}-20261021-170000+0200.bin")), "").unwrap();
            let c = down_comp(n, 10, "failed", "exit-code", "enabled");
            assert_eq!(r.decide(&c, 1300, false), Decision::None, "{n}");
            assert!(
                permitted(c.spec.unit.unwrap()).is_err(),
                "{n}: not in ALLOW"
            );
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn waits_for_fail_cycles_then_rate_limits() {
        let d = tmp("rl");
        let mut r = Reflex::new(d.clone(), true, 3);
        r.deploy_bin = d.join("no-deploy");
        let mut a = quiet_alerts();
        let c2 = down_comp("hermes-rs", 2, "failed", "exit-code", "enabled");
        assert_eq!(r.decide(&c2, 1100, false), Decision::None);
        let c = down_comp("hermes-rs", 3, "failed", "exit-code", "enabled");
        let now = now_secs();
        for _ in 0..3 {
            let dec = r.decide(&c, now, false);
            assert!(matches!(dec, Decision::Restart { .. }), "{dec:?}");
            r.execute("hermes-rs", &dec, &mut a);
        }
        assert!(matches!(r.decide(&c, now, false), Decision::Limited { .. }));
        // Dry-run actions never touch the live budget.
        assert_eq!(r.used("hermes-rs.service", now, false), 0);
        assert_eq!(r.recent.len(), 3);
        assert_eq!(r.recent[0]["mode"], "dry-run");
        assert!(d.join("audit.jsonl").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn cloudflared_once_per_hour_and_holds() {
        let d = tmp("cf");
        let mut r = Reflex::new(d.clone(), true, 1);
        let mut a = quiet_alerts();
        let c = down_comp("cloudflared", 1, "failed", "exit-code", "enabled");
        let now = now_secs();
        let dec = r.decide(&c, now, false);
        assert!(matches!(dec, Decision::Restart { .. }));
        r.execute("cloudflared", &dec, &mut a);
        assert!(matches!(r.decide(&c, now, false), Decision::Limited { .. }));
        let stopped = down_comp("tuner", 3, "inactive", "success", "enabled");
        assert!(matches!(
            r.decide(&stopped, now, false),
            Decision::Hold { .. }
        ));
        let disabled = down_comp("tuner", 3, "inactive", "exit-code", "disabled");
        assert!(matches!(
            r.decide(&disabled, now, false),
            Decision::Hold { .. }
        ));
        let busy = down_comp("tuner", 3, "failed", "exit-code", "enabled");
        assert!(matches!(r.decide(&busy, now, true), Decision::Hold { .. }));
        let quick = down_comp("tuner-quick", 5, "failed", "exit-code", "enabled");
        assert_eq!(r.decide(&quick, now, false), Decision::None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn mode_file_switch_and_rollback_choice() {
        let d = tmp("mode");
        let mut r = Reflex::new(d.clone(), true, 1);
        assert!(r.dry_run());
        std::fs::write(d.join("mode"), "live\n").unwrap();
        assert!(!r.dry_run());
        std::fs::write(d.join("mode"), "dry-run").unwrap();
        assert!(r.dry_run());
        // Exhausted budget + recent backup + deploy tool -> rollback.
        let bk = d.join("backups");
        std::fs::create_dir_all(&bk).unwrap();
        std::fs::write(bk.join("tuner-20261018-220000+0200.bin"), b"\x7fELF").unwrap();
        std::fs::write(d.join("deploy"), b"#!/bin/sh\n").unwrap();
        r.deploy_backups = bk;
        r.deploy_bin = d.join("deploy");
        let c = down_comp("tuner", 1, "failed", "exit-code", "enabled");
        let now = now_secs();
        r.dry_restarts
            .insert("tuner.service".into(), VecDeque::from(vec![now, now, now]));
        match r.decide(&c, now, false) {
            Decision::Rollback { backup, .. } => {
                assert!(backup.ends_with("tuner-20261018-220000+0200.bin"))
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn budget_survives_restart_of_nerve() {
        let d = tmp("persist");
        let now = now_secs();
        {
            let mut r = Reflex::new(d.clone(), true, 1);
            r.restarts
                .insert("tuner.service".into(), VecDeque::from(vec![now, now]));
            r.save();
        }
        let r = Reflex::new(d.clone(), true, 1);
        assert_eq!(r.used("tuner.service", now, false), 2);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn rollback_never_takes_another_units_backup() {
        let d = tmp("xunit");
        let bk = d.join("backups");
        std::fs::create_dir_all(&bk).unwrap();
        // Only a tuner-quick backup and junk: tuner must not roll back to them.
        std::fs::write(bk.join("tuner-quick-20261021-043200+0200.bin"), b"x").unwrap();
        std::fs::write(bk.join("tuner-new.bin"), b"x").unwrap();
        std::fs::create_dir_all(bk.join("tuner-20261021-043300+0200.bin")).unwrap();
        std::fs::write(d.join("deploy"), b"#!/bin/sh\n").unwrap();
        let mut r = Reflex::new(d.clone(), true, 1);
        r.deploy_backups = bk.clone();
        r.deploy_bin = d.join("deploy");
        let now = now_secs();
        r.dry_restarts
            .insert("tuner.service".into(), VecDeque::from(vec![now, now, now]));
        let c = down_comp("tuner", 1, "failed", "exit-code", "enabled");
        assert!(matches!(r.decide(&c, now, false), Decision::Limited { .. }));
        std::fs::write(bk.join("tuner-20261021-043200+0200.bin"), b"x").unwrap();
        match r.decide(&c, now, false) {
            Decision::Rollback { backup, .. } => {
                assert!(backup.ends_with("tuner-20261021-043200+0200.bin"))
            }
            other => panic!("{other:?}"),
        }
        assert!(is_backup_stamp("20261021-043200+0200"));
        assert!(!is_backup_stamp("quick-20261021-043200+0200"));
        assert!(!is_backup_stamp("20261018-2200+0200"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn execute_at_keeps_budget_on_the_given_clock() {
        let d = tmp("clock");
        let mut r = Reflex::new(d.clone(), true, 1);
        r.persist = false;
        let mut a = quiet_alerts();
        let c = down_comp("tuner", 1, "failed", "exit-code", "enabled");
        let t0 = 1_790_000_000;
        for _ in 0..3 {
            let dec = r.decide(&c, t0, false);
            assert!(matches!(dec, Decision::Restart { .. }));
            r.execute_at("tuner", &dec, &mut a, t0);
        }
        assert!(matches!(
            r.decide(&c, t0 + 60, false),
            Decision::Limited { .. }
        ));
        // An hour later on the same clock the budget is back.
        assert!(matches!(
            r.decide(&c, t0 + 3601, false),
            Decision::Restart { .. }
        ));
        let _ = std::fs::remove_dir_all(d);
    }
}
