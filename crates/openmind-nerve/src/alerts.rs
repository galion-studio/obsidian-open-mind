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

pub struct Alerts {
    pub path: Option<PathBuf>,
    pub recent: VecDeque<Value>,
    last_key: HashMap<String, u64>,
    pub telegram_env: Option<PathBuf>,
    pub ext_per_hour: u32,
    ext_sent: VecDeque<u64>,
    pub ext_status: String,
    /// When false nothing is printed (tests).
    pub to_journal: bool,
}

impl Alerts {
    pub fn new(path: Option<PathBuf>, telegram_env: Option<PathBuf>, ext_per_hour: u32) -> Self {
        let recent = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Vec<Value>>(&s).ok())
            .map(VecDeque::from)
            .unwrap_or_default();
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
            last_key: HashMap::new(),
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
}
