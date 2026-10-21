// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The nerve loop: collect -> correlate -> cortex -> incidents -> reflex ->
//! alert -> state, and the self-eval report (JSON in the repo-check report
//! shape plus an explained score, and markdown).
//!
//! [`Engine::cycle`] runs live probes; [`Engine::cycle_with`] takes an
//! already collected [`Collected`] so `replay` runs captured live data
//! through exactly the same pipeline.

use crate::access::{parse_line, DdosParams, DdosVerdict, Detector, Hit, Tail};
use crate::alerts::{Alerts, Severity};
use crate::collect::{collect, Collected};
use crate::config::{specs_for, Config, Kind, Spec, GAIA_FUNCTIONS};
use crate::cortex::{self, Cortex};
use crate::hub::Hub;
use crate::incident::{Correlator, Explain, Sev, Signal};
use crate::logscan::LogInsights;
use crate::probe::Listener;
use crate::reflex::{
    deploy_in_progress, status_needs_alert, Decision, Reflex, MAX_ACTIONS_PER_CYCLE,
};
use crate::score::{score, Score};
use crate::sensors::Sensors;
use crate::state::{CompState, Status, Thresholds};
use crate::util::{kib, now_secs, ts, write_atomic};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// How long journal insights (dead origins, scanner hits) stay in view.
pub const LOG_WINDOW_S: u64 = 1800;

/// Pushed events (errors, slow requests, caps) stay in incident view this long.
pub const PUSH_WINDOW_S: u64 = 120;

/// Shared between the loop and the HTTP server.
#[derive(Default)]
pub struct Shared {
    pub hub: Hub,
    pub state: Value,
    pub report: Value,
    pub report_md: String,
    pub alerts: Value,
    pub audit: Value,
    /// Request streams; the HTTP server records its own requests here.
    pub detector: Detector,
    /// Event mode hub (`NERVE_MODE=event`); None in the polling mode.
    pub evt: Option<Arc<crate::evt::Hub>>,
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
    skipped: Vec<String>,
    actions_since_report: Vec<Value>,
    pub reflexes_enabled: bool,
    /// Replay: fixed answer for "is a deploy running" instead of scanning /proc.
    pub busy_override: Option<bool>,
    pub write_files: bool,
    pub tails: Vec<(Tail, Option<String>)>,
    pub logs: LogInsights,
    pub listeners: Option<Vec<Listener>>,
    pub incidents: Correlator,
    pub cortex: Cortex,
    /// Cortex sensors, kept across cycles (stateful sensors).
    pub sensors: Sensors,
    /// Live host cycle (false in replay: host-reading sensors stay idle).
    pub live: bool,
    pub ddos: Vec<DdosVerdict>,
    pub score: Option<Score>,
    tunnel_prev: HashMap<String, (u64, u64)>,
    /// Port ownership map (component table + env).
    pub owners: crate::owners::OwnerMap,
    /// Holders of unowned listeners, from the last read.
    pub port_procs: std::collections::BTreeMap<u16, crate::owners::ProcInfo>,
    /// Clock of the last cycle (live: now; replay: the capture time).
    pub now: u64,
}

fn lock(s: &Arc<Mutex<Shared>>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    pub fn new(cfg: Config) -> Result<Self, String> {
        let specs = specs_for(&cfg)?;
        Ok(Self::with_specs(cfg, specs))
    }

    pub fn with_specs(cfg: Config, specs_list: Vec<Spec>) -> Self {
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
        let shared = Shared {
            detector: Detector {
                params: DdosParams {
                    min_rps: cfg.ddos_min_rps,
                    min_requests: cfg.ddos_min_requests,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let tails = cfg
            .access_logs
            .iter()
            .map(|a| {
                (
                    Tail::new(&a.name, a.path.clone(), false),
                    a.component.clone(),
                )
            })
            .collect();
        Engine {
            comps: specs_list.iter().cloned().map(CompState::new).collect(),
            th: Thresholds::default(),
            reflex,
            alerts,
            shared: Arc::new(Mutex::new(shared)),
            cycle: 0,
            started: now,
            // First journal window: the last 5 minutes.
            last_scan: now.saturating_sub(300),
            last_cycle_ms: 0,
            probe_errors: vec![],
            skipped: vec![],
            actions_since_report: vec![],
            reflexes_enabled: cfg.reflex,
            busy_override: None,
            write_files: true,
            tails,
            logs: LogInsights::default(),
            listeners: None,
            incidents: Correlator::default(),
            cortex: Cortex::default(),
            sensors: Sensors::from_env(&|k| std::env::var(k).ok()),
            live: true,
            ddos: vec![],
            score: None,
            tunnel_prev: HashMap::new(),
            owners: crate::owners::OwnerMap::for_config(&specs_list, &cfg),
            port_procs: Default::default(),
            now,
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

    /// One full live cycle. Returns true when a self-eval report was written.
    pub fn cycle(&mut self) -> bool {
        let t0 = Instant::now();
        let now = now_secs();
        let specs: Vec<Spec> = self.comps.iter().map(|c| c.spec.clone()).collect();
        // Snapshot the hub so probes never run while POST /signal waits on the lock.
        let hub = lock(&self.shared).hub.clone();
        let col = collect(&specs, &self.cfg, &hub, self.last_scan, now);
        self.last_scan = now;
        let hits = self.poll_tails(now);
        self.cycle_with(col, hits, now, t0)
    }

    /// New access-log lines, parsed (live tails).
    fn poll_tails(&mut self, now: u64) -> Vec<(String, Option<String>, Hit)> {
        let mut out = vec![];
        for (t, comp) in self.tails.iter_mut() {
            match t.poll() {
                Ok(lines) => {
                    for l in lines {
                        if let Some(h) = parse_line(&l, now) {
                            out.push((t.name.clone(), comp.clone(), h));
                        }
                    }
                }
                Err(e) => self
                    .probe_errors
                    .push(format!("access log {}: {e}", t.path.display())),
            }
        }
        out
    }

    /// Run one cycle over collected inputs at time `now`.
    pub fn cycle_with(
        &mut self,
        col: Collected,
        hits: Vec<(String, Option<String>, Hit)>,
        now: u64,
        t0: Instant,
    ) -> bool {
        self.cycle += 1;
        self.now = now;
        let tail_errs: Vec<String> = self
            .probe_errors
            .iter()
            .filter(|e| e.starts_with("access log"))
            .cloned()
            .collect();
        self.probe_errors = col.probe_errors;
        self.probe_errors.extend(tail_errs.into_iter().take(4));
        self.skipped = col.skipped;
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
        // Cortex inputs: sliding journal window, listeners, request streams.
        self.logs.merge(crate::logscan::insights(&col.lines));
        self.logs.expire(now.saturating_sub(LOG_WINDOW_S));
        if col.listeners.is_some() {
            self.listeners = col.listeners;
            self.port_procs = col.port_procs;
        }
        {
            let mut sh = lock(&self.shared);
            for (name, comp, h) in &hits {
                sh.detector.stream(name, comp.as_deref()).record(h);
            }
            for c in &self.comps {
                if c.spec.kind != Kind::Tunnel {
                    continue;
                }
                let Some(s) = c.latest() else { continue };
                if let (Some(r), Some(e)) = (s.tunnel_requests, s.tunnel_req_errors) {
                    let key = c.spec.name.to_string();
                    if let Some((r0, e0)) = self.tunnel_prev.get(&key).copied() {
                        if r >= r0 {
                            sh.detector
                                .stream(&format!("edge-{key}"), Some(c.spec.name))
                                .record_count(now, r - r0, e.saturating_sub(e0));
                        }
                    }
                    self.tunnel_prev.insert(key, (r, e));
                }
            }
            self.ddos = sh.detector.evaluate(now);
        }
        let own_port = self.cfg.addr.port();
        self.cortex = self.sensors.run(&cortex::Input {
            now,
            comps: &self.comps,
            logs: &self.logs,
            listeners: self.listeners.as_deref(),
            ddos: &self.ddos,
            known_ports: &self.cfg.known_ports,
            public_ok_ports: &self.cfg.public_ok_ports,
            listen_scope: self.cfg.listen_scope,
            own_port,
            loading_grace_s: self.cfg.loading_grace_s,
            journal: &col.lines,
            live: self.live,
            owners: &self.owners,
            procs: &self.port_procs,
        });
        let mut sigs = cortex::component_signals(&self.comps, now, self.cfg.loading_grace_s);
        sigs.extend(self.cortex.signals.iter().cloned());
        // Pushed events (errors, slow requests, spend/conn caps, starts).
        {
            let hub = lock(&self.shared).hub.clone();
            let comps = &self.comps;
            let map = |src: &str| match comps.iter().find(|c| c.spec.name == src) {
                Some(c) => (c.spec.group.to_string(), c.spec.name.to_string()),
                None => (src.to_string(), src.to_string()),
            };
            sigs.extend(hub.incident_signals(now.saturating_sub(PUSH_WINDOW_S), &map));
        }
        let comps_snapshot: Vec<(String, String, &'static str)> = self
            .comps
            .iter()
            .map(|c| {
                (
                    c.spec.name.to_string(),
                    c.spec.group.to_string(),
                    c.status.as_str(),
                )
            })
            .collect();
        self.incidents
            .update(now, sigs, &|cause, ss| explain(cause, ss, &comps_snapshot));
        self.cortex.attach_owners(&mut self.incidents, &self.comps);
        // Reflexes.
        if self.reflexes_enabled {
            let busy = self.busy_override.unwrap_or_else(deploy_in_progress);
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
                self.reflex.execute_at(name, &d, &mut self.alerts, now);
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
        // Incident alerts: one per incident id (and per severity change).
        let incs: Vec<(String, Sev, String, String)> = self
            .incidents
            .open_sorted()
            .iter()
            .map(|i| (i.id.clone(), i.sev, i.cause.clone(), i.title.clone()))
            .collect();
        for (id, sev, cause, title) in incs {
            let s = match sev {
                Sev::Critical => Severity::Crit,
                Sev::Error | Sev::Warn => Severity::Warn,
                Sev::Info => continue,
            };
            self.alerts.raise(
                &format!("incident:{id}:{}", sev.as_str()),
                s,
                &cause,
                &format!("incident {id} [{}]: {title}", sev.as_str()),
            );
        }
        self.last_cycle_ms = t0.elapsed().as_millis() as u64;
        let (r, md) = self.self_eval(now);
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
        // The report is always current in the API; files are written on cadence.
        sh.report = r.clone();
        sh.report_md = md.clone();
        drop(sh);
        if write_report {
            if self.write_files {
                self.write_report(&r, &md);
            }
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

    /// System status from correlated incidents (not from the single worst
    /// component): critical -> failing, error -> degraded, warn -> degrading.
    pub fn system_status(&self) -> &'static str {
        match self.incidents.open.values().map(|i| i.sev).max() {
            Some(Sev::Critical) => "failing",
            Some(Sev::Error) => "degraded",
            Some(Sev::Warn) => "degrading",
            _ => "healthy",
        }
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
        let sc = self
            .score
            .as_ref()
            .map(Score::to_json)
            .unwrap_or(Value::Null);
        let mut st = json!({
            "tool": "openmind-nerve",
            "service": "galion-nerve",
            "version": env!("CARGO_PKG_VERSION"),
            "ts": ts(now),
            "started": ts(self.started),
            "cycle": self.cycle,
            "interval_s": self.cfg.interval_s,
            "cycle_ms": self.last_cycle_ms,
            "mode": self.mode(),
            "system": {"status": self.system_status(), "components_worst": self.worst().as_str(), "counts": counts,
                       "score": sc["score"], "grade": sc["grade"], "pass": sc["pass"], "verdict": sc["verdict"],
                       "open_incidents": self.incidents.open.len(), "ddos_firing": self.cortex.ddos_firing},
            "incidents": self.incidents.to_json(),
            "cortex": self.cortex.to_json(),
            "cortex_sensors": self.sensors.last_json(),
            "cortex_events": self.cortex.nerve_events(now),
            "self_eval": sc,
            "components": self.comps.iter().map(|c| c.to_json(now)).collect::<Vec<_>>(),
            "gaia": self.gaia_rollup(),
            "signals": signals,
            "logs": self.logs.to_json(),
            "reflex": {"budgets": self.reflex.budgets_json(now), "recent": self.reflex.recent.iter().rev().take(10).collect::<Vec<_>>(),
                        "limits": {"restarts_per_hour_default": 3, "cloudflared_per_hour": 1, "max_actions_per_cycle": MAX_ACTIONS_PER_CYCLE, "rollback_per_unit_per_day": 1, "fail_cycles": self.cfg.fail_cycles},
                        "scaling": "single LXC: restarts/rollback only, no scaling"},
            "alerts": {"recent": self.alerts.recent.iter().rev().take(10).collect::<Vec<_>>(), "external": self.alerts.ext_status},
            "probe_errors": self.probe_errors,
            "probes_skipped": self.skipped,
        });
        // Only present after a state-dir load problem (keeps old goldens).
        for (k, n) in [
            ("reflex", &self.reflex.load_note),
            ("alerts", &self.alerts.load_note),
        ] {
            if let Some(n) = n {
                st[k]["load_note"] = json!(n);
            }
        }
        st
    }

    /// The self-eval: findings in repo-check's shape (check, level,
    /// component, msg) plus the explained score.
    pub fn self_eval(&mut self, now: u64) -> (Value, String) {
        let mut f: Vec<(String, &str, String, String)> = vec![];
        let mut add = |check: &str, level: &'static str, comp: &str, msg: String| {
            f.push((check.into(), level, comp.into(), msg))
        };
        for e in &self.probe_errors {
            add("probe", "error", "galion-nerve", e.clone());
        }
        for s in &self.skipped {
            add("probe", "info", "galion-nerve", format!("skipped: {s}"));
        }
        for i in self.incidents.open_sorted() {
            let lvl = if i.sev >= Sev::Error { "error" } else { "warn" };
            add(
                "incident",
                lvl,
                &i.cause,
                format!("{} [{}] {} -- {}", i.id, i.sev.as_str(), i.title, i.why),
            );
        }
        for v in &self.cortex.vulnerabilities {
            let lvl = match v.sev {
                Sev::Critical | Sev::Error => "error",
                Sev::Warn => "warn",
                Sev::Info => "info",
            };
            add(
                "vulnerability",
                lvl,
                &v.component,
                format!("{} ({})", v.title, v.evidence.join("; ")),
            );
        }
        for b in &self.cortex.bleeding {
            let lvl = if b.sev == Sev::Info { "info" } else { "warn" };
            add("bleeding", lvl, &b.component, b.title.clone());
        }
        for d in &self.ddos {
            if d.level != "normal" {
                add(
                    "ddos",
                    if d.level == "flood" { "error" } else { "warn" },
                    &d.stream,
                    d.explain(),
                );
            }
        }
        let mut self_warns = vec![];
        for c in &self.comps {
            if c.hist.is_empty() {
                // A replay marks components its capture does not name as absent
                // (T2.3): reported, not an error. A live cycle always samples.
                if c.status == crate::state::Status::Absent {
                    add("coverage", "info", c.spec.name, c.reasons.join("; "));
                } else {
                    add(
                        "coverage",
                        "error",
                        c.spec.name,
                        "no sample this cycle".into(),
                    );
                }
                continue;
            }
            if c.status != Status::Healthy {
                add(
                    "component",
                    "info",
                    c.spec.name,
                    format!("{}: {}", c.status.as_str(), c.reasons.join("; ")),
                );
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
                    let w = format!("RSS {} > 32 MiB budget", kib(r));
                    add("self-rss", "warn", "galion-nerve", w.clone());
                    self_warns.push(w);
                }
            }
        }
        if self.last_cycle_ms > self.cfg.interval_s * 500 {
            let w = format!(
                "cycle {} ms > half the {} s interval",
                self.last_cycle_ms, self.cfg.interval_s
            );
            add("cycle-time", "warn", "galion-nerve", w.clone());
            self_warns.push(w);
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
        let incs = self.incidents.open_sorted();
        let sc = score(&incs, &self.cortex, &self.probe_errors, &self_warns);
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
            "tool": "openmind-nerve", "version": env!("CARGO_PKG_VERSION"), "ts": stamp, "cycle": self.cycle,
            "mode": self.mode(), "pass": sc.pass && errors == 0, "errors": errors, "warnings": warnings,
            "score": sc.score, "grade": sc.grade, "verdict": sc.verdict,
            "deductions": sc.to_json()["deductions"].clone(),
            "system": self.system_status(), "components_worst": self.worst().as_str(),
            "incidents": self.incidents.open_sorted().iter().map(|i| i.to_json()).collect::<Vec<_>>(),
            "cortex": self.cortex.to_json(),
            "gaia": self.gaia_rollup(),
            "findings": findings, "components": comps,
            "reflex_actions": self.actions_since_report,
        });
        self.score = Some(sc);
        let md = render_md(&report);
        (report, md)
    }

    fn write_report(&self, r: &Value, md: &str) {
        let dir = self.cfg.data_dir.join("self-eval");
        let _ = write_atomic(
            &dir.join("latest.json"),
            serde_json::to_string_pretty(r)
                .unwrap_or_default()
                .as_bytes(),
        );
        let _ = write_atomic(&dir.join("latest.md"), md.as_bytes());
        let _ = write_atomic(
            &dir.join(self_eval_file_name(r["ts"].as_str().unwrap_or(""))),
            r.to_string().as_bytes(),
        );
        // Keep the newest SELF_EVAL_KEEP timestamped reports (~16 h at the
        // default cadence), newest by instant, not by name (DST fold).
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let v: Vec<(String, u64)> = rd
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if !n.starts_with("self-eval-") {
                        return None;
                    }
                    let mtime = e
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    Some((n, mtime))
                })
                .collect();
            for n in self_eval_prune(v, SELF_EVAL_KEEP) {
                let _ = std::fs::remove_file(dir.join(n));
            }
        }
        eprintln!(
            "<{}>galion-nerve self-eval {}: score {} ({}), {} error(s), {} warning(s), system {}",
            if r["pass"] == true { 6 } else { 4 },
            if r["pass"] == true { "PASS" } else { "FAIL" },
            r["score"],
            r["grade"].as_str().unwrap_or(""),
            r["errors"],
            r["warnings"],
            r["system"]
        );
    }
}

/// Timestamped self-eval reports kept in `self-eval/` (~16 h at 10 min).
pub const SELF_EVAL_KEEP: usize = 96;

/// File name for the timestamped self-eval report written at `stamp`
/// (Europe/Zagreb RFC 3339, e.g. `2026-10-25T02:34:35+02:00`):
/// `self-eval-20261025T023435+0200.json`.
///
/// The offset is part of the name. Without it the hour 02:00-02:59 on the
/// last Sunday of October (CEST +02:00, then again as CET +01:00) maps to the
/// same names twice and the CET reports overwrote the CEST ones (six reports
/// lost per year at the default 10 min cadence; slot 157, 2026-10-21).
pub fn self_eval_file_name(stamp: &str) -> String {
    format!("self-eval-{}.json", stamp.replace([':', '-'], ""))
}

/// The instant a `self-eval-*.json` name stands for: the offset form
/// (`self-eval-20261025T023435+0200.json`) parses exactly; the legacy
/// offset-less form (`self-eval-20261025T023435.json`, ambiguous inside the
/// October fold) has no instant and returns `None`.
pub fn self_eval_file_instant(name: &str) -> Option<u64> {
    let c = name.strip_prefix("self-eval-")?.strip_suffix(".json")?;
    if c.len() != 20 || c.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let iso = format!(
        "{}-{}-{}T{}:{}:{}{}",
        c.get(0..4)?,
        c.get(4..6)?,
        c.get(6..8)?,
        c.get(9..11)?,
        c.get(11..13)?,
        c.get(13..15)?,
        c.get(15..20)?
    );
    crate::util::parse_rfc3339(&iso)
}

/// Which `self-eval-*` files to delete so the newest `keep` remain. Order is
/// by instant: the name's own instant when it carries an offset, else the
/// file's mtime (legacy names), ties by name. Never by name alone: inside the
/// October fold `...T020435+0100` is an hour later than `...T023435+0200`
/// but sorts before it.
pub fn self_eval_prune(mut files: Vec<(String, u64)>, keep: usize) -> Vec<String> {
    files.sort_by(|a, b| {
        let ka = self_eval_file_instant(&a.0).unwrap_or(a.1);
        let kb = self_eval_file_instant(&b.0).unwrap_or(b.1);
        ka.cmp(&kb).then_with(|| a.0.cmp(&b.0))
    });
    let n = files.len().saturating_sub(keep);
    files.into_iter().take(n).map(|f| f.0).collect()
}

/// Words for an incident cause: title, why it matters, the next step.
pub fn explain(cause: &str, ss: &[Signal], comps: &[(String, String, &'static str)]) -> Explain {
    let top = ss
        .iter()
        .max_by_key(|s| s.sev)
        .map(|s| s.msg.as_str())
        .unwrap_or("");
    let top_comp = ss
        .iter()
        .max_by_key(|s| s.sev)
        .map(|s| s.component.as_str())
        .unwrap_or("");
    let short = |m: &str| -> String {
        let t: String = m.chars().take(220).collect();
        if m.chars().count() > 220 {
            format!("{t}...")
        } else {
            t
        }
    };
    let names: Vec<&str> = {
        let mut v: Vec<&str> = ss.iter().map(|s| s.component.as_str()).collect();
        v.sort();
        v.dedup();
        v
    };
    if let Some(stream) = cause.strip_prefix("ddos:") {
        let target = ss.first().map(|s| s.component.as_str()).unwrap_or(stream);
        let others = ss.len().saturating_sub(1);
        return Explain {
            title: format!("request flood on {target} (stream {stream})"),
            why: format!(
                "{}{}",
                short(top),
                if others > 0 { format!("; {others} related signal(s) from the target folded in") } else { String::new() }
            ),
            next: "check the top source IPs; block or rate-limit them at the edge (Cloudflare WAF rule / rate limit) -- owner approval; the nerve never changes the edge".into(),
        };
    }
    if let Some(group) = cause.strip_prefix("spend-cap:") {
        return Explain {
            title: format!("{group}: spend cap hit (provider calls refused)"),
            why: format!(
                "{}; the service refuses paid calls until the cap resets (Europe/Zagreb day or session), so users get 402s",
                short(top)
            ),
            next: "expected when a budget is reached; if it is not expected, look for a runaway loop or cron job, otherwise raise the cap (owner approval: real money)".into(),
        };
    }
    if let Some(comp) = cause.strip_prefix("scan:") {
        return Explain {
            title: format!("scanner/exploit probes against {comp}"),
            why: short(top),
            next: "deny dotfiles and admin paths at the edge, serve production builds only (owner approval)".into(),
        };
    }
    match cause {
        "edge" => Explain {
            title: format!("edge: {}", short(top)),
            why: "public hostnames answer with Cloudflare error pages; every failed request is a visitor or bot turned away, and the error counters hide real faults".into(),
            next: "start the services behind the refused ports, or remove their ingress rules/DNS records (cloudflared config + DNS: owner approval)".into(),
        },
        "stack-wide" => Explain {
            title: format!("stack-wide failure: {} components down at once ({})", names.len(), names.join(", ")),
            why: "several unrelated groups failing in the same cycle points at the host, the network or systemd, not at each service".into(),
            next: "check the host first (disk, RAM, systemd, network), before restarting services one by one".into(),
        },
        group => {
            let members: Vec<String> = comps
                .iter()
                .filter(|c| c.1 == group)
                .map(|c| format!("{}:{}", c.0, c.2))
                .collect();
            Explain {
                title: if top_comp == group {
                    format!("{group}: {}", short(top))
                } else {
                    format!("{group}: {top_comp} {}", short(top))
                },
                why: format!(
                    "{} signal(s) in group {group} ({}): one cause, one incident",
                    ss.len(),
                    members.join(", ")
                ),
                next: if top.contains("disk") || top.contains("RAM available") {
                    "free disk/RAM on the host (old build dirs, logs, caches); deleting anything needs the owner's approval".into()
                } else if top.contains("stuck loading") {
                    "the dependency never finished loading: read its logs, check the model files, then restart that one service (owner approval)".into()
                } else {
                    "read the unit journal; the reflex restarts allowlisted units within budget".into()
                },
            }
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
        "# openmind-nerve self-eval (galion-nerve)\n\n- time: {} (Europe/Zagreb)\n- cycle: {}, mode: {}\n- result: **{}**, score **{}/100 (grade {})**, {} error(s), {} warning(s)\n- system: **{}** (worst single component: {})\n- verdict: {}\n\n",
        r["ts"].as_str().unwrap_or(""),
        r["cycle"],
        r["mode"].as_str().unwrap_or(""),
        if r["pass"] == true { "PASS" } else { "FAIL" },
        r["score"],
        r["grade"].as_str().unwrap_or("?"),
        r["errors"],
        r["warnings"],
        r["system"].as_str().unwrap_or(""),
        r["components_worst"].as_str().unwrap_or("?"),
        r["verdict"].as_str().unwrap_or("")
    ));
    let txt = |v: &Value| v.as_str().unwrap_or("").replace('|', "/");
    s.push_str("## Score: what cost points\n\n| points | what | why |\n|---|---|---|\n");
    let ded = r["deductions"].as_array().cloned().unwrap_or_default();
    if ded.is_empty() {
        s.push_str("| 0 | nothing | - |\n");
    }
    for d in &ded {
        s.push_str(&format!(
            "| -{} | {} | {} |\n",
            d["points"],
            txt(&d["what"]),
            txt(&d["why"])
        ));
    }
    s.push_str("\n## Incidents (correlated)\n\n");
    let incs = r["incidents"].as_array().cloned().unwrap_or_default();
    if incs.is_empty() {
        s.push_str("none open\n");
    }
    for i in &incs {
        s.push_str(&format!(
            "### {} [{}] {}\n\n- open since {} ({} cycle(s)), components: {}\n- why: {}\n- next: {}\n- signals:\n",
            txt(&i["id"]), txt(&i["severity"]), txt(&i["title"]), txt(&i["opened"]), i["cycles"],
            i["components"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            txt(&i["why"]), txt(&i["next"])
        ));
        for sg in i["signals"].as_array().into_iter().flatten() {
            s.push_str(&format!(
                "  - {} {}: {}\n",
                txt(&sg["severity"]),
                txt(&sg["component"]),
                txt(&sg["msg"])
            ));
        }
        s.push('\n');
    }
    s.push_str("## Cortex\n\n### Vulnerabilities\n\n");
    let section = |s: &mut String, arr: &Value| {
        let a = arr.as_array().cloned().unwrap_or_default();
        if a.is_empty() {
            s.push_str("none\n");
        }
        for f in a {
            s.push_str(&format!(
                "- **{}** {}: {}\n",
                txt(&f["severity"]),
                txt(&f["component"]),
                txt(&f["title"])
            ));
            for e in f["evidence"].as_array().into_iter().flatten() {
                s.push_str(&format!("  - {}\n", txt(e)));
            }
            if !txt(&f["fix"]).is_empty() {
                s.push_str(&format!("  - fix: {}\n", txt(&f["fix"])));
            }
        }
    };
    section(&mut s, &r["cortex"]["vulnerabilities"]);
    s.push_str("\n### Bleeding\n\n");
    section(&mut s, &r["cortex"]["bleeding"]);
    s.push_str(&format!(
        "\n### DDoS (request-rate anomaly)\n\nfiring: **{}**\n\n",
        r["cortex"]["ddos"]["firing"]
    ));
    for d in r["cortex"]["ddos"]["streams"]
        .as_array()
        .into_iter()
        .flatten()
    {
        s.push_str(&format!(
            "- [{}] {}\n",
            txt(&d["level"]),
            txt(&d["explain"])
        ));
    }
    s.push('\n');
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

    /// 2026-10-25T02:00:00+02:00 Zagreb (one hour
    /// before the fall-back to CET).
    const FOLD_START: u64 = 1_792_886_400;

    #[test]
    fn self_eval_names_are_unique_across_the_october_fold() {
        // Every 10 min from 01:04:35+02:00 to 04:04:35+01:00: four hours of
        // instants, the 02:xx hour twice.
        let ts: Vec<u64> = (0..25).map(|k| FOLD_START - 3600 + 275 + k * 600).collect();
        let names: Vec<String> = ts
            .iter()
            .map(|t| self_eval_file_name(&crate::util::ts(*t)))
            .collect();
        let uniq: std::collections::BTreeSet<&String> = names.iter().collect();
        assert_eq!(uniq.len(), names.len(), "{names:?}");
        assert!(names.contains(&"self-eval-20261025T020435+0200.json".to_string()));
        assert!(names.contains(&"self-eval-20261025T020435+0100.json".to_string()));
        for (n, t) in names.iter().zip(&ts) {
            assert_eq!(self_eval_file_instant(n), Some(*t), "{n}");
        }
        // The old 15-char name collided on exactly the six 02:x4:35 reports.
        let old: std::collections::BTreeSet<String> = ts
            .iter()
            .map(|t| {
                crate::util::ts(*t)
                    .replace([':', '-'], "")
                    .chars()
                    .take(15)
                    .collect()
            })
            .collect();
        assert_eq!(ts.len() - old.len(), 6);
    }

    #[test]
    fn self_eval_prune_keeps_the_newest_by_instant() {
        let a = "self-eval-20261025T023435+0200.json"; // CEST, first pass
        let b = "self-eval-20261025T020435+0100.json"; // CET, 30 min after a
        let c = "self-eval-20261025T025435+0100.json";
        assert_eq!(self_eval_file_instant(a), Some(FOLD_START + 2075));
        assert_eq!(self_eval_file_instant(b), Some(FOLD_START + 2075 + 1800));
        // By name b < c < a; by instant a < b < c.
        let files = vec![(c.to_string(), 0), (a.to_string(), 0), (b.to_string(), 0)];
        assert_eq!(self_eval_prune(files.clone(), 2), vec![a.to_string()]);
        assert_eq!(
            self_eval_prune(files.clone(), 1),
            vec![a.to_string(), b.to_string()]
        );
        assert!(self_eval_prune(files, 3).is_empty());
        // Legacy offset-less names fall back to their mtime.
        let legacy = "self-eval-20261021T065435.json";
        assert_eq!(self_eval_file_instant(legacy), None);
        let files = vec![(legacy.to_string(), 1_792_558_475), (a.to_string(), 0)];
        assert_eq!(self_eval_prune(files, 1), vec![legacy.to_string()]);
        assert_eq!(self_eval_file_instant("self-eval-garbage+0100.json"), None);
        assert_eq!(self_eval_file_instant("latest.json"), None);
    }

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
