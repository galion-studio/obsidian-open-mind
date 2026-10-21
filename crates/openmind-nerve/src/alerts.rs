// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Alerts: journald (stderr with a syslog priority prefix, which systemd maps
//! to PRIORITY), `<data_dir>/alerts.json` (last 200), and optionally an
//! existing Telegram bot configured on the host (token read in place from an
//! env file at send time, passed to curl on stdin, never logged; alert text
//! only; capped per hour).

use crate::util::{now_secs, ts, write_atomic};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const KEEP: usize = 200;
const DEDUP_S: u64 = 1800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Crit,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Crit => "crit",
        }
    }
    fn syslog(self) -> u8 {
        match self {
            Severity::Info => 6,
            Severity::Warn => 4,
            Severity::Crit => 2,
        }
    }
}

/// Read `alerts.json`: a JSON array (current layout) or an object holding
/// it under `recent` or `alerts`. Non-object entries are dropped, the newest
/// KEEP kept. A file that exists but cannot be read is copied aside first
/// (`alerts.json.corrupt-<now>`) so the next write does not erase it.
fn load_recent(p: &std::path::Path, now: u64) -> (VecDeque<Value>, Option<String>) {
    let s = match std::fs::read(p) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (VecDeque::new(), None),
        Err(e) => {
            return (
                VecDeque::new(),
                Some(format!("{}: unreadable: {e}", p.display())),
            )
        }
    };
    let arr = match serde_json::from_str::<Value>(&s) {
        Ok(Value::Array(a)) => Some(a),
        Ok(Value::Object(mut m)) => match m.remove("recent").or_else(|| m.remove("alerts")) {
            Some(Value::Array(a)) => Some(a),
            _ => None,
        },
        _ => None,
    };
    match arr {
        Some(a) => {
            let mut q: VecDeque<Value> = a.into_iter().filter(Value::is_object).collect();
            while q.len() > KEEP {
                q.pop_front();
            }
            (q, None)
        }
        None => {
            let copy = crate::util::quarantine(p, now);
            let note = format!(
                "{} is not an alert list; copy at {}",
                p.display(),
                copy.map(|c| c.display().to_string())
                    .unwrap_or_else(|| "(copy failed)".into())
            );
            eprintln!("<4>galion-nerve alerts: {note}");
            (VecDeque::new(), Some(note))
        }
    }
}

pub struct Alerts {
    pub path: Option<PathBuf>,
    pub recent: VecDeque<Value>,
    last_key: HashMap<String, u64>,
    pub telegram_env: Option<PathBuf>,
    pub ext_per_hour: u32,
    ext_sent: VecDeque<u64>,
    pub ext_status: String,
    /// What happened loading `alerts.json` (None when clean).
    pub load_note: Option<String>,
    /// When false nothing is printed (tests).
    pub to_journal: bool,
}

impl Alerts {
    pub fn new(path: Option<PathBuf>, telegram_env: Option<PathBuf>, ext_per_hour: u32) -> Self {
        let now = now_secs();
        let (recent, load_note) = match &path {
            Some(p) => load_recent(p, now),
            None => (VecDeque::new(), None),
        };
        // Restore the 30-min dedupe window from the history, so a restart
        // does not raise (and page) every recent alert again.
        let mut last_key: HashMap<String, u64> = HashMap::new();
        for a in &recent {
            if let (Some(k), Some(t)) = (
                a["key"].as_str(),
                a["ts"].as_str().and_then(crate::util::parse_rfc3339),
            ) {
                // A stamp from the future (clock step) must not mute a key for long.
                let t = t.min(now);
                let e = last_key.entry(k.to_string()).or_insert(t);
                *e = (*e).max(t);
            }
        }
        let ext_status = match &telegram_env {
            None => {
                "none configured (no Telegram/Hermes alert channel found on the host)".to_string()
            }
            Some(p) if p.exists() => format!("telegram via {}", p.display()),
            Some(p) => format!("telegram env {} missing; disabled", p.display()),
        };
        Alerts {
            path,
            recent,
            last_key,
            load_note,
            telegram_env,
            ext_per_hour,
            ext_sent: VecDeque::new(),
            ext_status,
            to_journal: true,
        }
    }

    /// Raise an alert. Same `key` within 30 min is suppressed. Returns true if raised.
    pub fn raise(&mut self, key: &str, sev: Severity, component: &str, msg: &str) -> bool {
        let now = now_secs();
        if self.last_key.get(key).is_some_and(|t| now < t + DEDUP_S) {
            return false;
        }
        self.last_key.insert(key.to_string(), now);
        let a = json!({"ts": ts(now), "severity": sev.as_str(), "component": component, "key": key, "msg": msg});
        if self.to_journal {
            eprintln!(
                "<{}>galion-nerve ALERT {} {component}: {msg}",
                sev.syslog(),
                sev.as_str()
            );
        }
        if self.recent.len() >= KEEP {
            self.recent.pop_front();
        }
        self.recent.push_back(a);
        if let Some(p) = &self.path {
            let v: Vec<&Value> = self.recent.iter().collect();
            if let Ok(s) = serde_json::to_string_pretty(&v) {
                let _ = write_atomic(p, s.as_bytes());
            }
        }
        if sev != Severity::Info {
            self.external(
                now,
                &format!("[galion-nerve] {} {component}: {msg}", sev.as_str()),
            );
        }
        true
    }

    fn external(&mut self, now: u64, text: &str) {
        let Some(envf) = self.telegram_env.clone() else {
            return;
        };
        while self.ext_sent.front().is_some_and(|t| *t + 3600 < now) {
            self.ext_sent.pop_front();
        }
        if self.ext_sent.len() as u32 >= self.ext_per_hour {
            return;
        }
        let Ok(env) = std::fs::read_to_string(&envf) else {
            return;
        };
        let get = |k: &str| {
            env.lines()
                .filter_map(|l| l.trim().trim_start_matches("export ").split_once('='))
                .find(|(a, _)| a.trim() == k)
                .map(|(_, v)| v.trim().trim_matches('"').trim_matches('\'').to_string())
        };
        let (Some(tok), Some(chat)) = (
            get("TELEGRAM_BOT_TOKEN"),
            get("TELEGRAM_CHAT_ID").or_else(|| get("TELEGRAM_HOME_CHANNEL")),
        ) else {
            self.ext_status =
                "telegram env lacks TELEGRAM_BOT_TOKEN/TELEGRAM_CHAT_ID; disabled".into();
            return;
        };
        let safe = |s: &str| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || ":-_".contains(c))
        };
        if !safe(&tok) || !safe(&chat) {
            return;
        }
        // curl config on stdin keeps the token out of argv and logs.
        let text: String = text
            .chars()
            .filter(|c| *c != '"' && *c != '\\' && !c.is_control())
            .take(500)
            .collect();
        let cfg = format!(
            "url = \"https://api.telegram.org/bot{tok}/sendMessage\"\ndata-urlencode = \"chat_id={chat}\"\ndata-urlencode = \"text={text}\"\nsilent\nmax-time = 10\noutput = /dev/null\n"
        );
        let child = Command::new("curl")
            .args(["-K", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut c) = child {
            if let Some(mut si) = c.stdin.take() {
                let _ = si.write_all(cfg.as_bytes());
            }
            let ok = c.wait().map(|s| s.success()).unwrap_or(false);
            self.ext_sent.push_back(now);
            self.ext_status = format!("telegram: last send {}", if ok { "ok" } else { "failed" });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_and_file() {
        let dir = std::env::temp_dir().join(format!("nerve-alerts-{}", std::process::id()));
        let p = dir.join("alerts.json");
        let mut a = Alerts::new(Some(p.clone()), None, 4);
        a.to_journal = false;
        assert!(a.raise("k1", Severity::Warn, "tuner", "down"));
        assert!(!a.raise("k1", Severity::Warn, "tuner", "down again"));
        assert!(a.raise("k2", Severity::Crit, "tuner", "worse"));
        let v: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[1]["severity"], "crit");
        assert!(v[0]["ts"].as_str().unwrap().contains("+0"));
        let b = Alerts::new(Some(p), None, 4);
        assert_eq!(b.recent.len(), 2);
        assert!(b.ext_status.starts_with("none configured"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn old_alert_history_restores_dedupe_and_tolerates_layouts() {
        let dir = std::env::temp_dir().join(format!("nerve-alerts-t41-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("alerts.json");
        let now = now_secs();
        let old = json!([
            {"ts": ts(now - 60), "severity": "warn", "component": "tuner", "key": "recent", "msg": "m", "legacy_ack": true},
            {"ts": ts(now - 7200), "severity": "warn", "component": "tuner", "key": "old", "msg": "m"},
            {"ts": ts(now + 86400), "severity": "warn", "component": "tuner", "key": "future", "msg": "m"},
            "not an object",
        ]);
        std::fs::write(&p, old.to_string()).unwrap();
        let mut a = Alerts::new(Some(p.clone()), None, 0);
        a.to_journal = false;
        assert!(a.load_note.is_none());
        assert_eq!(a.recent.len(), 3, "non-object entries dropped");
        assert!(
            !a.raise("recent", Severity::Warn, "tuner", "again"),
            "still deduped after restart"
        );
        assert!(a.raise("old", Severity::Warn, "tuner", "again"));
        // A future stamp mutes for at most the normal window from now.
        assert!(!a.raise("future", Severity::Warn, "tuner", "again"));
        let v: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(v[0]["legacy_ack"], true, "unknown field kept");
        // Wrapped layout {"recent": [...]}.
        std::fs::write(
            &p,
            json!({"recent": [{"key": "w", "ts": ts(now)}]}).to_string(),
        )
        .unwrap();
        assert_eq!(Alerts::new(Some(p.clone()), None, 0).recent.len(), 1);
        // Over-long history is trimmed to the newest KEEP.
        let many: Vec<Value> = (0..KEEP + 5)
            .map(|i| json!({"key": format!("k{i}")}))
            .collect();
        std::fs::write(&p, serde_json::to_string(&many).unwrap()).unwrap();
        let b = Alerts::new(Some(p.clone()), None, 0);
        assert_eq!(b.recent.len(), KEEP);
        assert_eq!(b.recent[0]["key"], "k5");
        // Garbage: copied aside, reported, starts empty.
        std::fs::write(&p, "garbage").unwrap();
        let c = Alerts::new(Some(p.clone()), None, 0);
        assert!(c.recent.is_empty());
        assert!(c
            .load_note
            .as_deref()
            .unwrap_or("")
            .contains("not an alert list"));
        let copies = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("alerts.json.corrupt-")
            })
            .count();
        assert_eq!(copies, 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
