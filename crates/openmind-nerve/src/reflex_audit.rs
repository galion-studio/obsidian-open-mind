// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Reflex safety (nerve plan T3.2), two read-only checks:
//!
//! 1. [`dry_replay`]: replay a capture directory (see [`crate::replay`])
//!    with the reflex layer switched back on but forced dry-run
//!    ([`Reflex::force_dry`]): every decision the live loop would take is
//!    logged and nothing is executed (no `systemctl`, no `systemd-run`, no
//!    files outside a throwaway directory). The safety bar for the live
//!    capture corpus is zero cloudflared restarts and zero rollbacks: the
//!    edge dead-route condition (tunnel routes to closed origin ports) must
//!    stay alert-only. `worst_case` arms the rollback path as if
//!    `galion-deploy` were installed and every allowlisted unit had a
//!    deploy backup from the last 24 h; `stretch` keeps the last captured
//!    condition going for that many more cycles (budget exhaustion).
//! 2. [`allowlist_audit`]: every reflex target of a component table checked
//!    against the allowlist / never-touch list: unit name shape, deny
//!    overlap, cloudflared and tuner-quick budgets, rollback eligibility.

use crate::config::{Config, Kind, Spec};
use crate::reflex::{is_backup_stamp, permitted, ALLOW, DENY};
use crate::replay::{replay_dir_with, ReplayOut};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Options for [`dry_replay`].
#[derive(Debug, Clone, Default)]
pub struct DryOpts {
    /// Extra cycles repeating the last capture window.
    pub stretch: usize,
    /// Pretend galion-deploy and a fresh backup of every unit exist.
    pub worst_case: bool,
}

/// Result of [`dry_replay`].
pub struct DryReplay {
    pub out: ReplayOut,
    /// Every reflex audit entry, in order (restart, rollback, hold, rate-limited).
    pub log: Vec<Value>,
    pub verdict: Value,
}

fn scratch(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = format!(
        "{}-{}",
        crate::util::now_secs(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    std::env::temp_dir().join(format!("nerve-reflex-dry-{tag}-{}-{n}", std::process::id()))
}

/// Replay `dir` with reflexes on, forced dry. `cfg.data_dir` is replaced by
/// a throwaway directory (so a live `reflex.json` budget or `mode` file is
/// never read), removed again before returning.
pub fn dry_replay(dir: &Path, mut cfg: Config, opts: &DryOpts) -> Result<DryReplay, String> {
    let tmp = scratch("data");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    cfg.data_dir = tmp.join("data");
    let deploy = tmp.join("galion-deploy-stub");
    let backups = tmp.join("backups");
    if opts.worst_case {
        // Never executed (force_dry); it only has to exist for decide().
        std::fs::write(&deploy, b"not a program\n").map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
        for u in ALLOW {
            let svc = u.trim_end_matches(".service");
            std::fs::write(backups.join(format!("{svc}-20261021-000000+0200.bin")), b"")
                .map_err(|e| e.to_string())?;
        }
    }
    let worst = opts.worst_case;
    let res = replay_dir_with(dir, cfg, opts.stretch, &mut |e| {
        e.reflexes_enabled = true;
        e.busy_override = Some(false);
        e.reflex.force_dry = true;
        e.reflex.persist = false;
        e.reflex.log = Some(vec![]);
        e.reflex.recent.clear();
        e.reflex.rollback_enabled = true;
        e.reflex.switch_dir = tmp.join("no-switch-dir");
        if worst {
            e.reflex.deploy_bin = deploy.clone();
            e.reflex.deploy_backups = backups.clone();
        } else {
            e.reflex.deploy_bin = tmp.join("no-galion-deploy");
            e.reflex.deploy_backups = tmp.join("no-backups");
        }
    });
    let _ = std::fs::remove_dir_all(&tmp);
    let mut out = res?;
    let log = out.engine.reflex.log.take().unwrap_or_default();
    let verdict = verdict(&out, &log, opts);
    Ok(DryReplay { out, log, verdict })
}

fn verdict(out: &ReplayOut, log: &[Value], opts: &DryOpts) -> Value {
    let mut by: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for a in log {
        let action = a["action"].as_str().unwrap_or("?").to_string();
        let unit = a["unit"].as_str().unwrap_or("?").to_string();
        *by.entry(action).or_default().entry(unit).or_default() += 1;
    }
    let count = |action: &str| by.get(action).map(|m| m.values().sum()).unwrap_or(0usize);
    let cf_restarts = by
        .get("restart")
        .and_then(|m| m.get("cloudflared.service"))
        .copied()
        .unwrap_or(0);
    let rollbacks = count("rollback");
    let sh = out.engine.shared.lock().unwrap_or_else(|x| x.into_inner());
    let edge: Vec<Value> = sh.report["incidents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["cause"] == "edge")
        .map(|i| json!({"id": i["id"], "severity": i["severity"], "title": i["title"]}))
        .collect();
    let cf = out
        .engine
        .comps
        .iter()
        .find(|c| c.spec.name == "cloudflared")
        .map(|c| {
            json!({"status": c.status.as_str(), "hard_down": c.hard_down,
                   "consecutive_down": c.consecutive_down, "reasons": c.reasons})
        });
    let pass = cf_restarts == 0 && rollbacks == 0;
    json!({
        "cycles": out.cycles,
        "stretch": opts.stretch,
        "worst_case": opts.worst_case,
        "mode": "dry-run (forced; nothing executed)",
        "actions": by,
        "restarts": count("restart"),
        "cloudflared_restarts": cf_restarts,
        "rollbacks": rollbacks,
        "holds": count("hold"),
        "rate_limited": count("rate-limited"),
        "edge_incidents": edge,
        "edge_alert_only": cf_restarts == 0 && !edge.is_empty(),
        "cloudflared": cf,
        "score": sh.report["score"],
        "grade": sh.report["grade"],
        "pass": pass,
    })
}

/// A systemd unit name the reflex may pass to `systemctl` as one argument.
pub fn unit_name_ok(u: &str) -> bool {
    !u.is_empty()
        && !u.starts_with('-')
        && u.ends_with(".service")
        && u.len() <= 128
        && u.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@'))
}

/// Allowlist / never-touch audit of the reflex targets of `specs`.
/// `violations` empty = safe.
pub fn allowlist_audit(specs: &[Spec]) -> Value {
    let mut violations: Vec<String> = vec![];
    let mut notes: Vec<String> = vec![];
    for u in ALLOW {
        if !unit_name_ok(u) {
            violations.push(format!("allowlist entry {u:?} is not a plain unit name"));
        }
        if let Some(d) = DENY.iter().find(|d| u.contains(*d)) {
            violations.push(format!("allowlist entry {u} matches never-touch {d:?}"));
        }
        if !specs.iter().any(|s| s.unit == Some(*u)) {
            notes.push(format!(
                "allowlist entry {u} has no component in this table (dormant)"
            ));
        }
    }
    let mut rows = vec![];
    for s in specs {
        let Some(unit) = s.unit else {
            rows.push(json!({"component": s.name, "unit": null, "reflex": "none (no unit)"}));
            continue;
        };
        let perm = permitted(unit);
        let acts = s.restarts_per_hour > 0;
        let reflex = match (acts, perm) {
            (false, _) => "never (restarts_per_hour 0)".to_string(),
            (true, Ok(())) => format!("restart <= {}/h", s.restarts_per_hour),
            (true, Err(e)) => format!("hold only ({e})"),
        };
        let rollback = acts
            && perm.is_ok()
            && unit != "cloudflared.service"
            && matches!(s.kind, Kind::Service);
        if s.kind == Kind::SelfNerve && acts {
            violations.push(format!("{}: the nerve may not restart itself", s.name));
        }
        if unit == "cloudflared.service" && s.restarts_per_hour > 1 {
            violations.push(format!(
                "cloudflared budget {}/h > 1/h",
                s.restarts_per_hour
            ));
        }
        if unit.contains("tuner-quick") && s.restarts_per_hour > 0 {
            violations.push("tuner-quick must never be restarted (URL changes)".into());
        }
        if acts && perm.is_err() {
            notes.push(format!(
                "{} ({unit}) asks for restarts but is not permitted: alert/hold only",
                s.name
            ));
        }
        if !unit_name_ok(unit) && acts && perm.is_ok() {
            violations.push(format!("{}: unit {unit:?} not a plain unit name", s.name));
        }
        rows.push(
            json!({"component": s.name, "unit": unit, "restarts_per_hour": s.restarts_per_hour,
            "reflex": reflex, "rollback_eligible": rollback, "critical": s.critical}),
        );
    }
    // Backup selection must stay per unit: no allowlisted service name is a
    // `<other>-` prefix that a stamped name could alias.
    for a in ALLOW {
        let sa = a.trim_end_matches(".service");
        for b in ALLOW {
            let sb = b.trim_end_matches(".service");
            if a != b && sb.starts_with(&format!("{sa}-")) {
                let rest = &sb[sa.len() + 1..];
                let probe = format!("{rest}-20261021-000000+0200");
                if is_backup_stamp(&probe) {
                    violations.push(format!("backup of {sb} could be taken for {sa}"));
                } else {
                    notes.push(format!(
                        "{sa}- is a prefix of {sb}; stamp check keeps their backups apart"
                    ));
                }
            }
        }
    }
    json!({
        "allow": ALLOW,
        "deny": DENY,
        "components": rows,
        "violations": violations,
        "notes": notes,
        "pass": violations.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_specs, parse_specs};
    use std::path::PathBuf;

    #[test]
    fn default_table_passes_the_audit() {
        let a = allowlist_audit(&default_specs(None, None));
        assert_eq!(a["pass"], true, "{a:#}");
        let rows = a["components"].as_array().unwrap();
        let row = |n: &str| rows.iter().find(|r| r["component"] == n).unwrap().clone();
        assert_eq!(row("cloudflared")["rollback_eligible"], false);
        assert_eq!(row("cloudflared")["reflex"], "restart <= 1/h");
        assert_eq!(row("tuner-quick")["reflex"], "never (restarts_per_hour 0)");
        assert_eq!(row("galion-nerve")["reflex"], "none (no unit)");
        assert_eq!(row("hermes-rs")["rollback_eligible"], true);
    }

    #[test]
    fn hostile_tables_are_flagged_or_held() {
        // A components file that names ssh / an option-looking unit gets hold only.
        let s = parse_specs(
            r#"[{"name":"x","unit":"sshd.service","restarts_per_hour":5},
                {"name":"y","unit":"--now.service","restarts_per_hour":5},
                {"name":"q","unit":"tuner-quick.service","restarts_per_hour":1}]"#,
        )
        .unwrap();
        let a = allowlist_audit(&s);
        let notes = a["notes"].to_string();
        assert!(notes.contains("sshd.service"), "{a:#}");
        assert!(notes.contains("--now.service"), "{a:#}");
        assert!(a["violations"].to_string().contains("tuner-quick"), "{a:#}");
        assert_eq!(a["pass"], false);
        // NERVE_VOICE_UNIT pointing at a never-touch unit: hold only.
        let v = default_specs(Some("ssh.service".into()), Some(9000));
        let a = allowlist_audit(&v);
        assert!(a["notes"].to_string().contains("ssh.service"), "{a:#}");
    }

    #[test]
    fn unit_names() {
        assert!(unit_name_ok("hermes-rs.service"));
        assert!(unit_name_ok("getty@tty1.service"));
        assert!(!unit_name_ok("-H.service"));
        assert!(!unit_name_ok("a b.service"));
        assert!(!unit_name_ok("a;b.service"));
        assert!(!unit_name_ok("hermes-rs"));
    }

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-replay")
    }

    fn cfg() -> Config {
        Config::from_env(&|k| (k == "NERVE_DATA_DIR").then(|| "/nonexistent/nerve-186".into()))
            .unwrap()
    }

    /// Copy of the fixture with `edit` applied to every component of every state.
    fn mutated(tag: &str, edit: &dyn Fn(&mut Value)) -> PathBuf {
        let d = scratch(tag);
        std::fs::create_dir_all(&d).unwrap();
        for e in std::fs::read_dir(fixture()).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let mut text = std::fs::read_to_string(e.path()).unwrap();
            if n.starts_with("state") {
                let mut v: Value = serde_json::from_str(&text).unwrap();
                for c in v["components"].as_array_mut().unwrap() {
                    edit(c);
                }
                text = v.to_string();
            }
            std::fs::write(d.join(n), text).unwrap();
        }
        d
    }

    #[test]
    fn fixture_edge_dead_routes_stay_alert_only() {
        for opts in [
            DryOpts::default(),
            DryOpts {
                stretch: 130,
                worst_case: true,
            },
        ] {
            let r = dry_replay(&fixture(), cfg(), &opts).unwrap();
            let v = &r.verdict;
            assert_eq!(v["pass"], true, "{v:#}");
            assert_eq!(v["cloudflared_restarts"], 0);
            assert_eq!(v["rollbacks"], 0);
            assert_eq!(v["restarts"], 0, "{v:#}");
            assert_eq!(v["edge_alert_only"], true, "{v:#}");
            assert!(r.log.is_empty());
            assert_eq!(v["cycles"], 3 + opts.stretch);
        }
    }

    #[test]
    fn positive_control_failed_unit_restarts_then_rolls_back() {
        // hermes-rs failed for the whole capture: the dry replay must see the
        // reflex act (3 restarts in the hour), then a rollback in worst case.
        let d = mutated("hermes-failed", &|c| {
            if c["name"] == "hermes-rs" {
                c["unit_state"] = json!("failed");
                c["http"] = Value::Null;
            }
        });
        let r = dry_replay(
            &d,
            cfg(),
            &DryOpts {
                stretch: 20,
                worst_case: true,
            },
        )
        .unwrap();
        let v = &r.verdict;
        assert_eq!(v["actions"]["restart"]["hermes-rs.service"], 3, "{v:#}");
        assert_eq!(v["rollbacks"], 1, "{v:#}");
        assert_eq!(v["pass"], false);
        let rb = r.log.iter().find(|a| a["action"] == "rollback").unwrap();
        assert!(
            rb["result"].as_str().unwrap().starts_with("would run:"),
            "{rb}"
        );
        assert!(r.log.iter().all(|a| a["mode"] == "dry-run"));
        // Without the deploy tool there is no rollback: budget stop + alert.
        let r = dry_replay(
            &d,
            cfg(),
            &DryOpts {
                stretch: 20,
                worst_case: false,
            },
        )
        .unwrap();
        assert_eq!(r.verdict["rollbacks"], 0);
        assert_eq!(r.verdict["rate_limited"], 1, "{:#}", r.verdict);
        assert_eq!(r.verdict["pass"], true);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn positive_control_cloudflared_down_is_once_per_hour_never_rolled_back() {
        let d = mutated("cf-zero", &|c| {
            if c["name"] == "cloudflared" {
                c["tunnel"]["ha_connections"] = json!(0);
            }
        });
        let r = dry_replay(
            &d,
            cfg(),
            &DryOpts {
                stretch: 30,
                worst_case: true,
            },
        )
        .unwrap();
        let v = &r.verdict;
        assert_eq!(v["cloudflared_restarts"], 1, "{v:#}");
        assert_eq!(v["rollbacks"], 0, "{v:#}");
        assert_eq!(v["pass"], false);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn force_dry_beats_a_live_mode_file() {
        let d = scratch("mode");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("mode"), "live\n").unwrap();
        let mut r = crate::reflex::Reflex::new(d.clone(), false, 3);
        assert!(!r.dry_run());
        r.force_dry = true;
        assert!(r.dry_run());
        let _ = std::fs::remove_dir_all(d);
    }
}
