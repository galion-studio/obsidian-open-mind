// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Event-driven nerve mode (`NERVE_MODE=event`).
//!
//! The polling nerve wakes every `NERVE_INTERVAL` seconds (30 s live = 120
//! full probe cycles an hour). In event mode the main thread sleeps in the
//! kernel until something happens:
//!
//! - [`journal`]: one `journalctl -f -o json` child with server-side
//!   matches (systemd unit failures, exits, restarts, start-limit hits, OOM
//!   kills on every unit; started/stopped/finished of tracked units; kernel
//!   err+). Blocking pipe read.
//! - [`watch`]: inotify on task/state files and dirs (swarm slot records,
//!   POSEIDON queue log, cert dirs). Blocking read.
//! - [`pidwatch`]: pidfd per tracked service main process. Blocking poll.
//! - HTTP: `POST /event`, `POST /signal`, `POST /actions` are events too.
//! - [`threshold`]: disk and certificate expiry *crossings* (only the edge is
//!   an event), checked on the slow liveness heartbeat (default 15 min) and
//!   when a watched cert file changes.
//!
//! Every accepted event lands in the [`Hub`] ring (NDJSON stream for the
//! cortex: `GET /events/stream`). Severity >= high events go through the
//! [`coalesce::Coalescer`] (dedupe + burst debounce) and one batch reaches
//! the [`escalate::Escalator`] (default: delegate to the cortex, no spend).
//! Contract: `docs/nerve-event-contract.md`.

pub mod actions;
pub mod coalesce;
pub mod escalate;
pub mod inventory;
pub mod journal;
pub mod pidwatch;
pub mod runtime;
pub mod stats;
pub mod stream;
pub mod sys;
pub mod threshold;
pub mod watch;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};

/// Event schema version (bump only on breaking changes; adding fields is not breaking).
pub const SCHEMA_VERSION: u32 = 1;
/// Ring buffer size behind `GET /evt` and stream catch-up.
pub const RING: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "info" | "debug" | "notice" | "event" => Self::Info,
            "low" => Self::Low,
            "medium" | "warn" | "warning" => Self::Medium,
            "high" | "error" | "err" => Self::High,
            "critical" | "crit" | "fatal" | "alert" | "emerg" => Self::Critical,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// One nerve event (wire format: one JSON object per NDJSON line).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NerveEvent {
    pub v: u32,
    /// Monotonic per nerve process, starts at 1; 0 before publication.
    pub seq: u64,
    /// Europe/Zagreb RFC 3339 with offset.
    pub ts: String,
    /// Unix seconds.
    pub t: u64,
    /// journal | inotify | pidfd | threshold | http | inventory | engine | nerve | escalation | actions
    pub source: String,
    /// Dotted kind, see the contract (unit.failed, oom.kill, task.done, ...).
    pub kind: String,
    pub severity: Severity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Inventory group: nerve, cortex, gaia, hermes, console, sandbox,
    /// galion-os, swarm, infra, transient, user, other.
    pub group: String,
    /// Dedupe key: identical keys inside the dedupe window are one event.
    pub key: String,
    pub msg: String,
    #[serde(default)]
    pub data: Value,
    /// Duplicates folded into this event so far (1 = itself).
    #[serde(default = "one")]
    pub count: u32,
}

fn one() -> u32 {
    1
}

impl NerveEvent {
    pub fn new(source: &str, kind: &str, severity: Severity, msg: impl Into<String>) -> Self {
        let t = crate::util::now_secs();
        NerveEvent {
            v: SCHEMA_VERSION,
            seq: 0,
            ts: crate::util::ts(t),
            t,
            source: source.into(),
            kind: kind.into(),
            severity,
            unit: None,
            group: "other".into(),
            key: kind.into(),
            msg: msg.into(),
            data: Value::Null,
            count: 1,
        }
    }
    pub fn at(mut self, t: u64) -> Self {
        self.t = t;
        self.ts = crate::util::ts(t);
        self
    }
    pub fn unit(mut self, u: &str) -> Self {
        self.group = classify_unit(u).into();
        self.key = format!("{}:{u}", self.kind);
        self.unit = Some(u.into());
        self
    }
    pub fn group(mut self, g: &str) -> Self {
        self.group = g.into();
        self
    }
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = k.into();
        self
    }
    pub fn data(mut self, d: Value) -> Self {
        self.data = d;
        self
    }
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Unit name -> inventory group.
pub fn classify_unit(u: &str) -> &'static str {
    let b = u
        .trim_end_matches(".service")
        .trim_end_matches(".scope")
        .trim_end_matches(".slice");
    let has = |s: &str| b.contains(s);
    if b.starts_with("run-u")
        || b.starts_with("session-")
        || b.starts_with("user@")
        || b.starts_with("user-")
        || b == "-"
        || b.is_empty()
    {
        "transient"
    } else if b.starts_with("sbx") || has("-sbx") || has("sandbox") {
        "sandbox"
    } else if has("cortex") {
        "cortex"
    } else if has("nerve") {
        "nerve"
    } else if b.starts_with("hermes") {
        "hermes"
    } else if b == "console" || b == "galion-desk" || b.starts_with("console-ide") {
        "console"
    } else if b.starts_with("galion-os") {
        "galion-os"
    } else if b.starts_with("swarm") || b.starts_with("slot-") {
        "swarm"
    } else if [
        "cloudflared",
        "tuner-quick",
        "ssh",
        "sshd",
        "cron",
        "dbus",
        "rsyslog",
        "postfix@-",
        "postfix",
        "fail2ban",
        "networkd-dispatcher",
        "console-getty",
        "docker",
        "containerd",
    ]
    .contains(&b)
        || b.starts_with("systemd-")
        || b.starts_with("container-getty")
        || b.starts_with("getty")
    {
        "infra"
    } else if gaia_function(u).is_some() || b.starts_with("gaia") {
        "gaia"
    } else {
        "other"
    }
}

/// Gaia function owning a unit (ws/gaia section 4.2 names), if known.
pub fn gaia_function(u: &str) -> Option<&'static str> {
    let b = u.trim_end_matches(".service");
    Some(match b {
        "openmind-memory" | "link-watch-rs" => "APOLLO",
        "openmind-queue" => "POSEIDON",
        "galion-one" => "ELEUTHIA",
        "galion-health" | "tuner" | "galion-backup" => "DEMETER",
        "tanga-proto" => "ELEUTHIA",
        "galion-desk" | "console" => "HEPHAESTUS",
        "hermes-rs" => "ARTEMIS",
        "cloudflared" | "tuner-quick" | "galion-nerve" | "galion-nerve-shadow" => "AETHER",
        _ if b.starts_with("gaia-") => "GAIA",
        _ => return None,
    })
}

/// Groups whose unit lifecycle (started/stopped/finished) is an event.
/// Failures are events for every unit regardless.
pub const LIFECYCLE_GROUPS: &[&str] = &[
    "nerve",
    "cortex",
    "gaia",
    "hermes",
    "console",
    "galion-os",
    "swarm",
];
/// Infra units whose lifecycle is also tracked.
pub const LIFECYCLE_INFRA: &[&str] = &["cloudflared.service", "tuner-quick.service"];

pub fn lifecycle_tracked(unit: &str) -> bool {
    LIFECYCLE_GROUPS.contains(&classify_unit(unit)) || LIFECYCLE_INFRA.contains(&unit)
}

/// Event-mode configuration (env, read once at start).
#[derive(Debug, Clone)]
pub struct EvtConfig {
    /// Liveness heartbeat (seconds): one engine cycle + threshold check +
    /// inventory refresh, never an LLM call. Clamped to 60..=3600.
    pub heartbeat_s: u64,
    /// Engine cycle after an event: wait this long for the burst to settle.
    pub cycle_debounce_s: u64,
    /// Never more than one event-triggered engine cycle per this many seconds.
    pub cycle_min_gap_s: u64,
    pub dedupe_s: u64,
    pub debounce_s: u64,
    pub max_wait_s: u64,
    /// Lowest severity that may escalate.
    pub escalate_min: Severity,
    pub journal: bool,
    pub journal_bin: String,
    pub pidfd: bool,
    /// Paths watched with inotify (files or dirs).
    pub watch: Vec<PathBuf>,
    pub extra_units: Vec<String>,
    /// Extra PIDs to watch with pidfd (`NERVE_EVT_PIDS=pid,pid`).
    pub extra_pids: Vec<u32>,
    pub disk_paths: Vec<String>,
    pub disk_pct: Vec<u8>,
    pub certs: Vec<PathBuf>,
    pub cert_days: Vec<i64>,
    /// `live` lets POST /actions run (still needs reflex mode live); else dry-run.
    pub actions_live: bool,
    pub escalate: escalate::EscalateConfig,
    pub data_dir: PathBuf,
    /// Extra port -> group hints (`port=group,...`).
    pub port_groups: Vec<(u16, String)>,
}

fn list(v: Option<String>) -> Vec<String> {
    v.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

impl EvtConfig {
    pub fn from_env(
        env: &dyn Fn(&str) -> Option<String>,
        data_dir: PathBuf,
    ) -> Result<Self, String> {
        let num = |k: &str, d: u64| -> Result<u64, String> {
            match env(k) {
                None => Ok(d),
                Some(v) => v
                    .trim()
                    .parse()
                    .map_err(|_| format!("{k}: not a number: {v:?}")),
            }
        };
        let on = |k: &str, d: bool| {
            env(k)
                .map(|v| !matches!(v.trim(), "0" | "off" | "false" | "no"))
                .unwrap_or(d)
        };
        let mut disk_pct: Vec<u8> = list(env("NERVE_EVT_DISK_PCT"))
            .iter()
            .filter_map(|x| x.parse().ok())
            .filter(|p| *p > 0 && *p <= 100)
            .collect();
        if disk_pct.is_empty() {
            disk_pct = vec![85, 90, 95];
        }
        disk_pct.sort_unstable();
        let mut cert_days: Vec<i64> = list(env("NERVE_EVT_CERT_DAYS"))
            .iter()
            .filter_map(|x| x.parse().ok())
            .collect();
        if cert_days.is_empty() {
            cert_days = vec![30, 14, 3, 0];
        }
        cert_days.sort_unstable_by(|a, b| b.cmp(a));
        let escalate_min = match env("NERVE_EVT_ESCALATE_MIN") {
            None => Severity::High,
            Some(s) => Severity::parse(&s)
                .ok_or_else(|| format!("NERVE_EVT_ESCALATE_MIN: bad severity {s:?}"))?,
        };
        if escalate_min < Severity::High {
            return Err(
                "NERVE_EVT_ESCALATE_MIN must be high or critical (only severity >= high escalates)"
                    .into(),
            );
        }
        let port_groups = list(env("NERVE_EVT_PORT_GROUPS"))
            .iter()
            .filter_map(|kv| {
                let (p, g) = kv.split_once('=')?;
                Some((p.trim().parse().ok()?, g.trim().to_string()))
            })
            .collect();
        Ok(EvtConfig {
            heartbeat_s: num("NERVE_EVT_HEARTBEAT", 900)?.clamp(60, 3600),
            cycle_debounce_s: num("NERVE_EVT_CYCLE_DEBOUNCE", 5)?.clamp(1, 300),
            cycle_min_gap_s: num("NERVE_EVT_CYCLE_MIN_GAP", 60)?.clamp(5, 3600),
            dedupe_s: num("NERVE_EVT_DEDUPE", 600)?.clamp(1, 86_400),
            debounce_s: num("NERVE_EVT_DEBOUNCE", 30)?.clamp(1, 3600),
            max_wait_s: num("NERVE_EVT_MAX_WAIT", 120)?.clamp(1, 7200),
            escalate_min,
            journal: on("NERVE_EVT_JOURNAL", true),
            journal_bin: env("NERVE_EVT_JOURNALCTL").unwrap_or_else(|| "journalctl".into()),
            pidfd: on("NERVE_EVT_PIDFD", true),
            watch: list(env("NERVE_EVT_WATCH"))
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            extra_units: list(env("NERVE_EVT_UNITS")),
            extra_pids: list(env("NERVE_EVT_PIDS"))
                .iter()
                .filter_map(|p| p.parse().ok())
                .collect(),
            disk_paths: {
                let d = list(env("NERVE_EVT_DISK"));
                if d.is_empty() {
                    vec!["/".into()]
                } else {
                    d
                }
            },
            disk_pct,
            certs: list(env("NERVE_EVT_CERTS"))
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            cert_days,
            actions_live: env("NERVE_EVT_ACTIONS")
                .map(|v| v.trim() == "live")
                .unwrap_or(false),
            escalate: escalate::EscalateConfig::from_env(env)?,
            data_dir,
            port_groups,
        })
    }
}

/// `POST /event` record (gaia-live body) as a nerve event. `data.unit` names the unit.
pub fn from_http_event(e: &crate::Event) -> NerveEvent {
    let data: Value = serde_json::from_str(&e.data_json()).unwrap_or(Value::Null);
    let sev = Severity::parse(e.severity()).unwrap_or(Severity::Info);
    let kind: String = e.kind().chars().take(64).collect();
    let mut ev = NerveEvent::new("http", &kind, sev, format!("{} {}", e.source(), e.kind()));
    if let Some(u) = data.get("unit").and_then(Value::as_str) {
        ev = ev.unit(u);
    } else {
        ev.group = match e.source() {
            s if s.contains("swarm") || s.contains("slot") => "swarm".into(),
            s => classify_unit(&format!("{s}.service")).into(),
        };
    }
    let id = data
        .get("id")
        .or_else(|| data.get("slot"))
        .map(|v| v.to_string())
        .unwrap_or_default();
    ev.key = format!("http:{}:{}:{id}", e.source(), kind);
    ev.data = data;
    ev
}

/// An `error` signal (POST /signal, nerve-client) as a nerve event (medium:
/// the engine correlates repeated errors into an incident, which escalates).
pub fn from_signal(s: &crate::hub::Signal) -> NerveEvent {
    NerveEvent::new(
        "http",
        "signal.error",
        Severity::Medium,
        s.msg.chars().take(200).collect::<String>(),
    )
    .group(classify_unit(&format!("{}.service", s.source)))
    .key(format!("signal.error:{}:{}", s.source, s.name))
    .data(json!({"source": s.source, "name": s.name}))
    .at(s.t)
}

/// Shared event hub: ring buffer, dedupe table, stream wake-up, stats,
/// inventory, escalation and action records.
pub struct Hub {
    pub inner: Mutex<HubInner>,
    pub cv: Condvar,
    /// Into the runtime (HTTP-born events).
    pub tx: Mutex<Option<std::sync::mpsc::Sender<runtime::Input>>>,
    /// Reflex dry-run state, refreshed after every engine cycle.
    pub reflex_dry: std::sync::atomic::AtomicBool,
    pub actions: Mutex<actions::Actions>,
}

pub struct HubInner {
    pub seq: u64,
    pub ring: VecDeque<NerveEvent>,
    /// key -> (seq of the stored event, last seen t)
    dedupe: HashMap<String, (u64, u64)>,
    pub dedupe_s: u64,
    pub stats: stats::Stats,
    pub inventory: Value,
    pub escalations: VecDeque<Value>,
    pub action_log: VecDeque<Value>,
    pub streams: usize,
    pub config: Value,
}

impl Hub {
    pub fn new(dedupe_s: u64) -> Self {
        Hub {
            inner: Mutex::new(HubInner {
                seq: 0,
                ring: VecDeque::new(),
                dedupe: HashMap::new(),
                dedupe_s,
                stats: stats::Stats::new(crate::util::now_secs()),
                inventory: Value::Null,
                escalations: VecDeque::new(),
                action_log: VecDeque::new(),
                streams: 0,
                config: Value::Null,
            }),
            cv: Condvar::new(),
            tx: Mutex::new(None),
            reflex_dry: std::sync::atomic::AtomicBool::new(true),
            actions: Mutex::new(actions::Actions::new(false)),
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, HubInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Publish: `Some(event with seq)` if new, `None` if folded into a
    /// duplicate inside the dedupe window (its `count` is bumped).
    pub fn publish(&self, mut ev: NerveEvent) -> Option<NerveEvent> {
        let mut g = self.lock();
        let window = g.dedupe_s;
        if let Some(&(seq, last)) = g.dedupe.get(&ev.key) {
            if ev.t.saturating_sub(last) < window {
                g.dedupe.insert(ev.key.clone(), (seq, ev.t));
                if let Some(e) = g.ring.iter_mut().rev().find(|e| e.seq == seq) {
                    e.count = e.count.saturating_add(1);
                }
                g.stats.deduped += 1;
                return None;
            }
        }
        g.seq += 1;
        ev.seq = g.seq;
        let (k, s, t) = (ev.key.clone(), ev.seq, ev.t);
        g.dedupe.insert(k, (s, t));
        if g.dedupe.len() > 4096 {
            let cut = t.saturating_sub(window);
            g.dedupe.retain(|_, v| v.1 >= cut);
        }
        g.ring.push_back(ev.clone());
        while g.ring.len() > RING {
            g.ring.pop_front();
        }
        g.stats.published(ev.severity, t);
        drop(g);
        self.cv.notify_all();
        Some(ev)
    }

    /// Events with seq > since (oldest first), at most n.
    pub fn since(&self, since: u64, n: usize) -> Vec<NerveEvent> {
        let g = self.lock();
        g.ring
            .iter()
            .filter(|e| e.seq > since)
            .take(n)
            .cloned()
            .collect()
    }

    /// Hand an HTTP-born event to the runtime (falls back to direct publish).
    pub fn submit(&self, ev: NerveEvent) {
        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match tx {
            Some(t) => {
                if let Err(e) = t.send(runtime::Input::Http(ev)) {
                    if let runtime::Input::Http(ev) = e.0 {
                        self.publish(ev);
                    }
                }
            }
            None => {
                self.publish(ev);
            }
        }
    }

    /// POST /actions.
    pub fn action(&self, body: &str) -> (u16, Value) {
        let r = match actions::parse(body) {
            Ok(r) => r,
            Err(e) => return (400, json!({"ok": false, "error": e})),
        };
        let dry = self.reflex_dry.load(std::sync::atomic::Ordering::SeqCst);
        let (code, v, ev) = self
            .actions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .handle(&r, dry, crate::util::now_secs());
        {
            let mut g = self.lock();
            g.action_log.push_back(v.clone());
            while g.action_log.len() > 100 {
                g.action_log.pop_front();
            }
        }
        self.submit(ev);
        (code, v)
    }

    pub fn stats_json(&self, now: u64) -> Value {
        let mut g = self.lock();
        let mut s = g.stats.to_json(now);
        s["seq"] = json!(g.seq);
        s["ring"] = json!(g.ring.len());
        s["streams"] = json!(g.streams);
        s["config"] = g.config.clone();
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        assert_eq!(classify_unit("galion-nerve.service"), "nerve");
        assert_eq!(classify_unit("galion-nerve-shadow.service"), "nerve");
        assert_eq!(classify_unit("openmind-queue.service"), "gaia");
        assert_eq!(classify_unit("hermes-rs.service"), "hermes");
        assert_eq!(classify_unit("console.service"), "console");
        assert_eq!(
            classify_unit("sbx176-wdq-20261021-095340.service"),
            "sandbox"
        );
        assert_eq!(classify_unit("run-u21491.service"), "transient");
        assert_eq!(classify_unit("session-71580.scope"), "transient");
        assert_eq!(classify_unit("cloudflared.service"), "infra");
        assert_eq!(classify_unit("galion-os-image.service"), "galion-os");
        assert_eq!(classify_unit("cortex-advisor.service"), "cortex");
        // another worker's sandbox crash unit seen live on the VPS 2026-10-21 18:39 +02:00
        assert_eq!(
            classify_unit("cortex-sbx-crash-20261021-183907.service"),
            "sandbox"
        );
        assert!(!lifecycle_tracked(
            "cortex-sbx-crash-20261021-183907.service"
        ));
        assert_eq!(classify_unit("motd-news.service"), "other");
        assert!(lifecycle_tracked("tuner-quick.service"));
        assert!(!lifecycle_tracked("sysstat-collect.service"));
    }

    #[test]
    fn severity_order_and_parse() {
        assert!(Severity::Critical > Severity::High && Severity::High > Severity::Medium);
        assert_eq!(Severity::parse("error"), Some(Severity::High));
        assert_eq!(Severity::parse("fatal"), Some(Severity::Critical));
        assert_eq!(Severity::parse("x"), None);
    }

    #[test]
    fn publish_dedupes_inside_window() {
        let h = Hub::new(60);
        let e = NerveEvent::new("journal", "unit.failed", Severity::High, "x")
            .unit("hermes-rs.service")
            .at(1000);
        assert_eq!(h.publish(e.clone()).unwrap().seq, 1);
        assert!(h.publish(e.clone().at(1030)).is_none());
        assert_eq!(h.since(0, 10)[0].count, 2);
        // outside the window (60 s after the last duplicate): new event
        assert_eq!(h.publish(e.at(1091)).unwrap().seq, 2);
        assert_eq!(h.lock().stats.deduped, 1);
    }

    #[test]
    fn wire_roundtrip() {
        let e = NerveEvent::new("journal", "oom.kill", Severity::Critical, "m")
            .unit("openmind-memory.service");
        let l = e.to_line();
        assert!(!l.contains('\n'));
        let back: NerveEvent = serde_json::from_str(&l).unwrap();
        assert_eq!(back, e);
        assert!(l.contains("\"severity\":\"critical\""));
    }

    #[test]
    fn config_defaults_and_guards() {
        let env = |_: &str| None;
        let c = EvtConfig::from_env(&env, "/tmp".into()).unwrap();
        assert_eq!(c.heartbeat_s, 900);
        assert_eq!(c.escalate_min, Severity::High);
        assert!(!c.actions_live);
        assert!(!c.escalate.enabled());
        let low = |k: &str| (k == "NERVE_EVT_ESCALATE_MIN").then(|| "medium".to_string());
        assert!(EvtConfig::from_env(&low, "/tmp".into()).is_err());
        let hb = |k: &str| (k == "NERVE_EVT_HEARTBEAT").then(|| "1".to_string());
        assert_eq!(
            EvtConfig::from_env(&hb, "/tmp".into()).unwrap().heartbeat_s,
            60
        );
    }
}
