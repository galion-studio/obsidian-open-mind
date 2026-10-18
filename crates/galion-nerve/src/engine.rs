// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The nerve loop: collect -> correlate -> reflex -> alert -> state, and the
//! V1 self-eval report (JSON in the repo-check report shape, plus markdown).

use crate::alerts::{Alerts, Severity};
use crate::collect::collect;
use crate::config::{default_specs, Config, Kind, Spec, GAIA_FUNCTIONS};
use crate::hub::Hub;
use crate::reflex::{
    deploy_in_progress, status_needs_alert, Decision, Reflex, MAX_ACTIONS_PER_CYCLE,
};
use crate::state::{CompState, Status, Thresholds};
use crate::util::{kib, now_secs, ts, write_atomic};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Shared between the loop and the HTTP server.
#[derive(Default)]
pub struct Shared {
    pub hub: Hub,
    pub state: Value,
    pub report: Value,
    pub report_md: String,
    pub alerts: Value,
    pub audit: Value,
}

pub struct Engine {
    pub cfg: Config,
    pub comps: Vec<CompState>,
    pub th: Thresholds,
    pub reflex: Reflex,
    pub alerts: Alerts,
    pub shared: Arc<Mutex<Shared>>,
    pub cycle: u64,
    pub started: u64,
    last_scan: u64,
    last_cycle_ms: u64,
    probe_errors: Vec<String>,
    actions_since_report: Vec<Value>,
    pub reflexes_enabled: bool,
    pub write_files: bool,
}

fn lock(s: &Arc<Mutex<Shared>>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    pub fn new(cfg: Config) -> Self {
        let specs: Vec<Spec> = default_specs(cfg.voice_unit.clone(), cfg.voice_port);
        let mut reflex = Reflex::new(cfg.data_dir.clone(), cfg.dry_run_env, cfg.fail_cycles);
        reflex.rollback_enabled = cfg.rollback;
        reflex.deploy_bin = cfg.deploy_bin.clone();
        reflex.deploy_backups = cfg.deploy_backups.clone();
        reflex.switch_dir = cfg.switch_dir.clone();
        let alerts = Alerts::new(
            Some(cfg.data_dir.join("alerts.json")),
            cfg.telegram_env.clone(),
            cfg.ext_alerts_per_hour,
        );
        let now = now_secs();
        Engine {
            comps: specs.into_iter().map(CompState::new).collect(),
            th: Thresholds::default(),
            reflex,
            alerts,
            shared: Arc::new(Mutex::new(Shared::default())),
            cycle: 0,
            started: now,
            // First journal window: the last 5 minutes.
            last_scan: now.saturating_sub(300),
            last_cycle_ms: 0,
            probe_errors: vec![],
            actions_since_report: vec![],
            reflexes_enabled: true,
            write_files: true,
            cfg,
        }
    }

    pub fn mode(&self) -> &'static str {
        if !self.reflexes_enabled {
            "observe-only"
        } else if self.reflex.dry_run() {
            "dry-run"
        } else {
            "live"
        }
    }

    /// One full cycle. Returns true when a self-eval report was written.
    pub fn cycle(&mut self) -> bool {
        let t0 = Instant::now();
        let now = now_secs();
        self.cycle += 1;
        let specs: Vec<Spec> = self.comps.iter().map(|c| c.spec.clone()).collect();
        // Snapshot the hub so probes never run while POST /signal waits on the lock.
        let hub = lock(&self.shared).hub.clone();
        let col = collect(&specs, &self.cfg, &hub, self.last_scan, now);
        self.last_scan = now;
        self.probe_errors = col.probe_errors;
        let cycle_ms_prev = self.last_cycle_ms;
        for c in self.comps.iter_mut() {
            if let Some(mut s) = col.samples.get(c.spec.name).cloned() {
                if c.spec.kind == Kind::SelfNerve {
                    s.extra["cycle_ms"] = json!(cycle_ms_prev);
                    if cycle_ms_prev > self.cfg.interval_s * 500 {
                        s.extra["degraded"] = json!(format!("cycle took {cycle_ms_prev} ms"));
                    }
                }
                c.push(s, &self.th);
            }
        }
        // Reflexes.
        if self.reflexes_enabled {
            let busy = deploy_in_progress();
            let mut acted = 0;
            for i in 0..self.comps.len() {
                let d = self.reflex.decide(&self.comps[i], now, busy);
                if d == Decision::None {
                    continue;
                }
                if matches!(d, Decision::Restart { .. } | Decision::Rollback { .. }) {
                    if acted >= MAX_ACTIONS_PER_CYCLE {
                        continue;
                    }
                    acted += 1;
                }
                let name = self.comps[i].spec.name;
                let before = self.reflex.recent.len();
                self.reflex.execute(name, &d, &mut self.alerts);
                if self.reflex.recent.len() > before {
                    if let Some(a) = self.reflex.recent.back() {
                        self.actions_since_report.push(a.clone());
                    }
                }
            }
        }
        // Status alerts (deduplicated per component+status for 30 min).
        for c in &self.comps {
            if let Some(sev) = status_needs_alert(c) {
                let key = format!("status:{}:{}", c.spec.name, c.status.as_str());
                self.alerts.raise(
                    &key,
                    sev,
                    c.spec.name,
                    &format!("{}: {}", c.status.as_str(), c.reasons.join("; ")),
                );
            }
        }
        self.last_cycle_ms = t0.elapsed().as_millis() as u64;
        let state = self.state_json(now);
        let write_report =
            self.cycle == 1 || self.cycle.is_multiple_of(self.cfg.report_every as u64);
        if self.write_files {
            let _ = write_atomic(
                &self.cfg.data_dir.join("state.json"),
                state.to_string().as_bytes(),
            );
        }
        let mut sh = lock(&self.shared);
        sh.state = state;
        sh.alerts = json!(self.alerts.recent.iter().rev().take(50).collect::<Vec<_>>());
        sh.audit = json!(self.reflex.recent.iter().rev().collect::<Vec<_>>());
        drop(sh);
        if write_report {
            let (r, md) = self.self_eval(now);
            if self.write_files {
                self.write_report(&r, &md);
            }
            let mut sh = lock(&self.shared);
            sh.report = r;
            sh.report_md = md;
            self.actions_since_report.clear();
        }
        write_report
    }

    fn worst(&self) -> Status {
        self.comps
            .iter()
            .map(|c| c.status)
            .max()
            .unwrap_or(Status::Healthy)
    }

    pub fn gaia_rollup(&self) -> Value {
        let mut m = serde_json::Map::new();
        for f in GAIA_FUNCTIONS {
            let cs: Vec<&CompState> = self.comps.iter().filter(|c| c.spec.gaia == f).collect();
            let present: Vec<&&CompState> =
                cs.iter().filter(|c| c.status != Status::Absent).collect();
            let st = present.iter().map(|c| c.status).max();
            m.insert(
                f.to_string(),
                json!({
                    "status": st.map(|s| s.as_str()).unwrap_or(if cs.is_empty() { "no-live-systems" } else { "absent" }),
                    "note": gaia_note(f),
                    "components": cs.iter().map(|c| format!("{}:{}", c.spec.name, c.status.as_str())).collect::<Vec<_>>(),
                }),
            );
        }
        Value::Object(m)
    }

    pub fn state_json(&self, now: u64) -> Value {
        let mut counts = serde_json::Map::new();
        for st in [
            Status::Healthy,
            Status::Degrading,
            Status::FailingSoon,
            Status::Failing,
            Status::Absent,
        ] {
            counts.insert(
                st.as_str().into(),
                json!(self.comps.iter().filter(|c| c.status == st).count()),
            );
        }
        let sh = lock(&self.shared);
        let signals = sh.hub.to_json(now);
        drop(sh);
        json!({
            "tool": "galion-nerve",
            "version": env!("CARGO_PKG_VERSION"),
            "ts": ts(now),
            "started": ts(self.started),
            "cycle": self.cycle,
            "interval_s": self.cfg.interval_s,
            "cycle_ms": self.last_cycle_ms,
            "mode": self.mode(),
            "system": {"status": self.worst().as_str(), "counts": counts},
            "components": self.comps.iter().map(|c| c.to_json(now)).collect::<Vec<_>>(),
            "gaia": self.gaia_rollup(),
            "signals": signals,
            "reflex": {"budgets": self.reflex.budgets_json(now), "recent": self.reflex.recent.iter().rev().take(10).collect::<Vec<_>>(),
                        "limits": {"restarts_per_hour_default": 3, "cloudflared_per_hour": 1, "max_actions_per_cycle": MAX_ACTIONS_PER_CYCLE, "rollback_per_unit_per_day": 1, "fail_cycles": self.cfg.fail_cycles},
                        "scaling": "single LXC: restarts/rollback only, no scaling"},
            "alerts": {"recent": self.alerts.recent.iter().rev().take(10).collect::<Vec<_>>(), "external": self.alerts.ext_status},
            "probe_errors": self.probe_errors,
        })
    }

    /// The V1 self-eval: findings in repo-check's shape (check, level, msg).
    pub fn self_eval(&self, now: u64) -> (Value, String) {
        let mut f: Vec<(String, &str, String, String)> = vec![];
        let mut add = |check: &str, level: &'static str, comp: &str, msg: String| {
            f.push((check.into(), level, comp.into(), msg))
        };
        for e in &self.probe_errors {
            add("probe", "error", "galion-nerve", e.clone());
        }
        for c in &self.comps {
            if c.hist.is_empty() {
                add(
                    "coverage",
                    "error",
                    c.spec.name,
                    "no sample this cycle".into(),
                );
                continue;
            }
            let r = c.reasons.join("; ");
            match c.status {
                Status::Failing if c.spec.critical => add("component", "error", c.spec.name, r),
                Status::Failing | Status::FailingSoon | Status::Degrading => {
                    add("component", "warn", c.spec.name, r)
                }
                Status::Absent => add("component", "info", c.spec.name, r),
                Status::Healthy => {}
            }
            if let Some(ok) = c.latest().and_then(|s| s.health_says) {
                let nerve_ok = !matches!(c.status, Status::Failing);
                if ok != nerve_ok {
                    add(
                        "agreement",
                        "warn",
                        c.spec.name,
                        format!("galion-health ok={ok} but nerve says {}", c.status.as_str()),
                    );
                }
            }
        }
        if let Some(me) = self
            .comps
            .iter()
            .find(|c| c.spec.kind == Kind::SelfNerve)
            .and_then(|c| c.latest())
        {
            if let Some(r) = me.rss_kb {
                if r > 32 * 1024 {
                    add(
                        "self-rss",
                        "warn",
                        "galion-nerve",
                        format!("RSS {} > 32 MiB budget", kib(r)),
                    );
                }
            }
        }
        if self.last_cycle_ms > self.cfg.interval_s * 500 {
            add(
                "cycle-time",
                "warn",
                "galion-nerve",
                format!(
                    "cycle {} ms > half the {} s interval",
                    self.last_cycle_ms, self.cfg.interval_s
                ),
            );
        }
        if !self.cfg.addr.ip().is_loopback() {
            add(
                "loopback",
                "error",
                "galion-nerve",
                format!("bound to {}", self.cfg.addr),
            );
        }
        if self.write_files && !crate::reflex::data_dir_ok(&self.cfg.data_dir) {
            add(
                "storage",
                "error",
                "galion-nerve",
                format!("{} not writable", self.cfg.data_dir.display()),
            );
        }
        let stamp = ts(now);
        if !(stamp.ends_with("+02:00") || stamp.ends_with("+01:00")) {
            add(
                "time",
                "error",
                "galion-nerve",
                format!("timestamp {stamp} is not Europe/Zagreb"),
            );
        }
        for c in &self.comps {
            if let Some(u) = c.spec.unit {
                let used = self.reflex.used(u, now, self.reflex.dry_run());
                if c.spec.restarts_per_hour > 0 && used >= c.spec.restarts_per_hour as usize {
                    add(
                        "reflex-budget",
                        "warn",
                        c.spec.name,
                        format!(
                            "restart budget {used}/{} per hour used",
                            c.spec.restarts_per_hour
                        ),
                    );
                }
            }
        }
        let pushed: Vec<String> = {
            let sh = lock(&self.shared);
            sh.hub
                .sources
                .iter()
                .filter(|(_, s)| now.saturating_sub(s.last_seen) < 300)
                .map(|(k, _)| k.clone())
                .collect()
        };
        add(
            "push",
            "info",
            "galion-nerve",
            if pushed.is_empty() {
                "no push sources live; all components on pull probes".into()
            } else {
                format!("live push sources: {}", pushed.join(","))
            },
        );
        add(
            "alerts-channel",
            "info",
            "galion-nerve",
            format!(
                "journald + alerts.json; external: {}",
                self.alerts.ext_status
            ),
        );
        add(
            "mode",
            "info",
            "galion-nerve",
            format!("reflex mode {}", self.mode()),
        );
        let errors = f.iter().filter(|x| x.1 == "error").count();
        let warnings = f.iter().filter(|x| x.1 == "warn").count();
        let findings: Vec<Value> = f
            .iter()
            .map(|(c, l, comp, m)| json!({"check": c, "level": l, "component": comp, "msg": m}))
            .collect();
        let comps: Vec<Value> = self
            .comps
            .iter()
            .map(|c| {
                let s = c.latest().cloned().unwrap_or_default();
                json!({"name": c.spec.name, "status": c.status.as_str(), "gaia": c.spec.gaia, "unit_state": s.unit_active,
                       "http": s.http_code, "latency_ms": s.latency_ms, "mem_kb": s.mem_kb, "cpu_pct": s.cpu_pct,
                       "restarts_1h": c.restarts_1h(now), "log_errors": s.journal_errors, "reasons": c.reasons, "detail": detail(&s)})
            })
            .collect();
        let report = json!({
            "tool": "galion-nerve", "version": env!("CARGO_PKG_VERSION"), "ts": stamp, "cycle": self.cycle,
            "mode": self.mode(), "pass": errors == 0, "errors": errors, "warnings": warnings,
            "system": self.worst().as_str(), "gaia": self.gaia_rollup(),
            "findings": findings, "components": comps,
            "reflex_actions": self.actions_since_report,
        });
        let md = render_md(&report);
        (report, md)
    }

    fn write_report(&self, r: &Value, md: &str) {
        let dir = self.cfg.data_dir.join("self-eval");
        let stamp = r["ts"].as_str().unwrap_or("").replace([':', '-'], "");
        let _ = write_atomic(
            &dir.join("latest.json"),
            serde_json::to_string_pretty(r)
                .unwrap_or_default()
                .as_bytes(),
        );
        let _ = write_atomic(&dir.join("latest.md"), md.as_bytes());
        let _ = write_atomic(
            &dir.join(format!(
                "self-eval-{}.json",
                stamp.chars().take(15).collect::<String>()
            )),
            r.to_string().as_bytes(),
        );
        // Keep the newest 96 timestamped reports (~16 h at the default cadence).
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut v: Vec<_> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("self-eval-"))
                })
                .collect();
            v.sort();
            let n = v.len().saturating_sub(96);
            for p in v.into_iter().take(n) {
                let _ = std::fs::remove_file(p);
            }
        }
        let lvl = if r["pass"] == true {
            Severity::Info
        } else {
            Severity::Warn
        };
        if lvl != Severity::Info {
            eprintln!(
                "<4>galion-nerve self-eval FAIL: {} error(s), {} warning(s), system {}",
                r["errors"], r["warnings"], r["system"]
            );
        } else {
            eprintln!(
                "<6>galion-nerve self-eval PASS: {} warning(s), system {}",
                r["warnings"], r["system"]
            );
        }
    }
}

/// One-line facts for tunnels, the host and push links.
pub fn detail(s: &crate::state::Sample) -> String {
    let mut d = vec![];
    if let Some(c) = s.tunnel_conns {
        d.push(format!(
            "{c} edge conns, {} req, {} req errors",
            s.tunnel_requests.unwrap_or(0),
            s.tunnel_req_errors.unwrap_or(0)
        ));
    }
    if let (Some(t), Some(a)) = (s.disk_total_kb, s.disk_avail_kb) {
        d.push(format!(
            "disk / {:.0}% used ({} free of {})",
            100.0 - a as f64 * 100.0 / t.max(1) as f64,
            kib(a),
            kib(t)
        ));
    }
    if let (Some(t), Some(a)) = (s.ram_total_kb, s.ram_avail_kb) {
        d.push(format!("RAM {} available of {}", kib(a), kib(t)));
    }
    if let Some(l) = s.load1 {
        d.push(format!("load1 {l}"));
    }
    if let Some(m) = s.extra.get("mouth").and_then(Value::as_str) {
        d.push(m.to_string());
    }
    if let Some(a) = s.extra.get("push_age_s").and_then(Value::as_u64) {
        d.push(format!("last push {a} s ago"));
    }
    d.join(", ")
}

/// What the nerve itself contributes to each Gaia function.
pub fn gaia_note(f: &str) -> &'static str {
    match f {
        "AETHER" => "nerve: this hub (signals in, state out)",
        "MINERVA" => "alerting: nerve alerts to journald + alerts.json",
        "DEMETER" => "boot-units: nerve restart reflex",
        "HADES" => "rollback: nerve rollback reflex via galion-deploy, 1 per unit per day",
        _ => "",
    }
}

fn with_detail(main: String, extra: &str) -> String {
    match (main.is_empty(), extra.is_empty()) {
        (true, true) => "-".into(),
        (true, false) => format!("({extra})"),
        (false, true) => main,
        (false, false) => format!("{main} ({extra})"),
    }
}

pub fn render_md(r: &Value) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "# galion-nerve self-eval\n\n- time: {} (Europe/Zagreb)\n- cycle: {}, mode: {}\n- result: **{}** ({} error(s), {} warning(s))\n- system: **{}**\n\n",
        r["ts"].as_str().unwrap_or(""),
        r["cycle"],
        r["mode"].as_str().unwrap_or(""),
        if r["pass"] == true { "PASS" } else { "FAIL" },
        r["errors"],
        r["warnings"],
        r["system"].as_str().unwrap_or("")
    ));
    s.push_str("## Components\n\n| component | Gaia | status | unit | http | latency | mem | cpu | restarts 1h | log errors | reasons |\n|---|---|---|---|---|---|---|---|---|---|---|\n");
    let d = |v: &Value| {
        if v.is_null() {
            "-".to_string()
        } else {
            v.to_string().trim_matches('"').to_string()
        }
    };
    for c in r["components"].as_array().into_iter().flatten() {
        let mem = c["mem_kb"].as_u64().map(kib).unwrap_or_else(|| "-".into());
        let reasons: Vec<String> = c["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(|s| s.replace('|', "/")))
            .collect();
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            d(&c["name"]),
            d(&c["gaia"]),
            d(&c["status"]),
            d(&c["unit_state"]),
            d(&c["http"]),
            c["latency_ms"]
                .as_u64()
                .map(|x| format!("{x} ms"))
                .unwrap_or("-".into()),
            mem,
            c["cpu_pct"]
                .as_f64()
                .map(|x| format!("{x}%"))
                .unwrap_or("-".into()),
            d(&c["restarts_1h"]),
            d(&c["log_errors"]),
            with_detail(reasons.join("; "), c["detail"].as_str().unwrap_or(""))
        ));
    }
    s.push_str("\n## Gaia functions\n\n| function | status | components |\n|---|---|---|\n");
    if let Some(g) = r["gaia"].as_object() {
        for (k, v) in g {
            let cs: Vec<String> = v["components"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(String::from))
                .collect();
            s.push_str(&format!(
                "| {k} | {} | {} |\n",
                d(&v["status"]),
                with_detail(cs.join(", "), v["note"].as_str().unwrap_or(""))
            ));
        }
    }
    s.push_str("\n## Findings\n\n");
    for x in r["findings"].as_array().into_iter().flatten() {
        s.push_str(&format!(
            "- {} `{}` {}: {}\n",
            d(&x["level"]),
            d(&x["check"]),
            d(&x["component"]),
            d(&x["msg"])
        ));
    }
    s.push_str("\n## Reflex actions since last report\n\n");
    let acts = r["reflex_actions"].as_array().cloned().unwrap_or_default();
    if acts.is_empty() {
        s.push_str("none\n");
    }
    for a in acts {
        s.push_str(&format!(
            "- {} [{}] {} {}: {} -> {}\n",
            d(&a["ts"]),
            d(&a["mode"]),
            d(&a["action"]),
            d(&a["unit"]),
            d(&a["reason"]),
            d(&a["result"])
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_renders_report_shape() {
        let r = json!({"ts": "2026-10-18T22:30:00+02:00", "cycle": 1, "mode": "dry-run", "pass": true, "errors": 0, "warnings": 1,
            "system": "degrading", "gaia": {"AETHER": {"status": "healthy", "components": ["cloudflared:healthy"]}},
            "findings": [{"check": "component", "level": "warn", "component": "x", "msg": "m"}],
            "components": [{"name": "tuner", "gaia": "DEMETER", "status": "healthy", "unit_state": "active", "http": 200, "latency_ms": 1, "mem_kb": 2048, "cpu_pct": 0.1, "restarts_1h": 0, "log_errors": 0, "reasons": []}],
            "reflex_actions": []});
        let md = render_md(&r);
        assert!(md.contains("**PASS**"));
        assert!(md.contains(
            "| tuner | DEMETER | healthy | active | 200 | 1 ms | 2.0 MiB | 0.1% | 0 | 0 | - |"
        ));
        assert!(md.contains("| AETHER | healthy | cloudflared:healthy |"));
        assert!(md.contains("- warn `component` x: m"));
    }
}
