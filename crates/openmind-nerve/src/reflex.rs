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
    pub recent: VecDeque<Value>,
    pub persist: bool,
    /// Top-level keys of `reflex.json` this version does not know (written by
    /// an older or newer nerve). Kept and written back unchanged on save.
    extra: serde_json::Map<String, Value>,
    /// `reflex.json` existed but was unreadable: the live restart budget is
    /// treated as spent until this instant (fail closed, no budget reset).
    pub budget_unknown_until: u64,
    /// What happened while loading the state dir (None when clean).
    pub load_note: Option<String>,
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
            recent: VecDeque::new(),
            persist: true,
            extra: serde_json::Map::new(),
            budget_unknown_until: 0,
            load_note: None,
        };
        r.load();
        r
    }

    /// `<data_dir>/mode` ("live" or "dry-run") overrides NERVE_DRY_RUN at runtime.
    pub fn dry_run(&self) -> bool {
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
        let path = self.state_path();
        let s = match std::fs::read(&path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.load_failed(&path, &format!("unreadable: {e}"));
                return;
            }
        };
        let v = match serde_json::from_str::<Value>(&s) {
            Ok(Value::Object(m)) => m,
            Ok(_) => {
                self.load_failed(&path, "not a JSON object");
                return;
            }
            Err(e) => {
                self.load_failed(&path, &format!("bad JSON: {e}"));
                return;
            }
        };
        for (k, val) in v {
            match k.as_str() {
                // Timestamps: integers, floats, numeric strings or RFC 3339
                // (older layouts), a bare number is one restart.
                "restarts" => {
                    for (unit, arr) in val.as_object().into_iter().flatten() {
                        let q: VecDeque<u64> = match arr {
                            Value::Array(a) => {
                                a.iter().filter_map(crate::util::json_secs).collect()
                            }
                            other => crate::util::json_secs(other).into_iter().collect(),
                        };
                        let mut q: Vec<u64> = q.into();
                        q.sort_unstable();
                        self.restarts.insert(unit.clone(), q.into());
                    }
                }
                "rollbacks" => {
                    for (unit, t) in val.as_object().into_iter().flatten() {
                        if let Some(t) = crate::util::json_secs(t) {
                            self.rollbacks.insert(unit.clone(), t);
                        }
                    }
                }
                _ => {
                    self.extra.insert(k, val);
                }
            }
        }
    }

    /// `reflex.json` exists but cannot be used: keep a copy, and treat the
    /// live restart budget as spent for an hour (the window it covers)
    /// rather than silently starting from zero.
    fn load_failed(&mut self, path: &Path, why: &str) {
        let now = now_secs();
        let copy = crate::util::quarantine(path, now);
        self.budget_unknown_until = now + 3600;
        self.load_note = Some(format!(
            "{} {why}; copy at {}; live restarts held until {}",
            path.display(),
            copy.map(|p| p.display().to_string())
                .unwrap_or_else(|| "(copy failed)".into()),
            ts(self.budget_unknown_until)
        ));
        eprintln!(
            "<4>galion-nerve reflex: {}",
            self.load_note.as_deref().unwrap_or("")
        );
    }

    fn save(&self) {
        if !self.persist {
            return;
        }
        let mut v = self.extra.clone();
        v.insert("restarts".into(), json!(self.restarts));
        v.insert("rollbacks".into(), json!(self.rollbacks));
        let _ = write_atomic(&self.state_path(), Value::Object(v).to_string().as_bytes());
    }

    /// True when another restart of `unit` fits the hourly budget.
    pub fn budget_left(&self, unit: &str, now: u64, dry: bool, per_hour: usize) -> bool {
        if !dry && now < self.budget_unknown_until {
            return false;
        }
        self.used(unit, now, dry) < per_hour
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
            if !(n.starts_with(&pre) && n.ends_with(".bin")) {
                continue;
            }
            let m = e.metadata().ok()?.modified().ok()?;
            let secs = m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
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
        if self.budget_left(unit, now, dry, c.spec.restarts_per_hour as usize) {
            return Decision::Restart {
                unit: unit.into(),
                reason,
            };
        }
        let svc = unit.trim_end_matches(".service");
        if self.rollback_enabled
            && down
            && (dry || now >= self.budget_unknown_until)
            && self.deploy_bin.exists()
            && self.rollbacks.get(unit).is_none_or(|t| *t + 86400 < now)
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
        self.recent.push_back(entry);
    }

    /// Carry out one decision (or log what would be done in dry-run).
    pub fn execute(&mut self, comp: &str, d: &Decision, alerts: &mut Alerts) -> bool {
        let now = now_secs();
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
                if !dry {
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
        std::fs::write(bk.join("tuner-20261018-2200+0200.bin"), b"\x7fELF").unwrap();
        std::fs::write(d.join("deploy"), b"#!/bin/sh\n").unwrap();
        r.deploy_backups = bk;
        r.deploy_bin = d.join("deploy");
        let c = down_comp("tuner", 1, "failed", "exit-code", "enabled");
        let now = now_secs();
        r.dry_restarts
            .insert("tuner.service".into(), VecDeque::from(vec![now, now, now]));
        match r.decide(&c, now, false) {
            Decision::Rollback { backup, .. } => {
                assert!(backup.ends_with("tuner-20261018-2200+0200.bin"))
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
}
