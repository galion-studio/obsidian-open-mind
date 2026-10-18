// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Guardian config (TOML). Every path and address has a safe default:
//! state under `target/test-out/guardian`, probes on 127.0.0.1.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct File {
    #[serde(default)]
    pub guardian: Settings,
    #[serde(default)]
    pub probe: Vec<Spec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    /// Seconds between beats (guardian-tick.py ran every 60 s).
    pub interval_s: u64,
    /// Own state: beat, report.json, health.jsonl, incidents.json, streak.json.
    pub state_dir: PathBuf,
    /// Store backend: "jsonl" (default, portable) or "sqlite" (guardian.db
    /// parity, needs the `sqlite` build feature).
    pub store: String,
    /// guardian.db path for the sqlite store. Empty = `<state_dir>/guardian.db`.
    pub db: String,
    /// Extra beat file (keep.sh wrote `last-guardian.ts`). Empty = none.
    pub beat: String,
    /// Keep at most this many health rows (guardian-tick.py pruned to ~4000).
    pub keep_rows: usize,
    /// Hermes state.db to read tool-loop warnings from (read-only). Empty = off.
    pub hermes_state_db: String,
    /// Where the tool-loop warning line goes. Empty = `<state_dir>/tool-warn.txt`.
    pub tool_warn_out: String,
    /// health-tick.py parity: append `health fail ...` rows here. Empty = off.
    pub activity: String,
    /// Minimum seconds between two activity rows (health-tick ran every 900 s).
    pub activity_every_s: u64,
    /// B25: nerve JSONL path; a line per probe whose state changed. Empty = off.
    pub nerve: String,
    /// keep.sh parity: one `digest` activity row per Zagreb day. Empty = off.
    /// `{day}` is replaced with the date.
    pub digest_text: String,
    /// Escalation policy for suggestions.
    pub warn_after: u32,
    pub restart_after: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            interval_s: 60,
            state_dir: PathBuf::from("target/test-out/guardian"),
            store: "jsonl".into(),
            db: String::new(),
            beat: String::new(),
            keep_rows: 4000,
            hermes_state_db: String::new(),
            tool_warn_out: String::new(),
            activity: String::new(),
            activity_every_s: 900,
            nerve: String::new(),
            digest_text: String::new(),
            warn_after: 2,
            restart_after: 4,
        }
    }
}

fn d_host() -> String {
    "127.0.0.1".into()
}
fn d_timeout() -> u64 {
    2500
}
fn d_warn() -> f64 {
    85.0
}
fn d_crit() -> f64 {
    95.0
}
fn d_tunnel() -> String {
    "cloudflared".into()
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Spec {
    Http {
        name: String,
        #[serde(default = "d_host")]
        host: String,
        port: u16,
        path: String,
        #[serde(default)]
        ok_codes: Vec<u16>,
        /// desk_heal.py: a 503 with `loading` in the body counts as up.
        #[serde(default)]
        loading_ok: bool,
        #[serde(default = "d_timeout")]
        timeout_ms: u64,
    },
    /// systemctl is-active; reported as skipped where systemd is absent.
    Unit { name: Option<String>, unit: String },
    Disk {
        path: String,
        #[serde(default = "d_warn")]
        warn: f64,
        #[serde(default = "d_crit")]
        crit: f64,
    },
    Mem,
    Load,
    Fresh { path: String, max_age_s: u64 },
    /// Process present (scan of /proc/*/comm, no pgrep). Named `tunnel` by
    /// default like guardian-tick.py.
    Process {
        #[serde(default)]
        name: Option<String>,
        #[serde(default = "d_tunnel")]
        comm: String,
    },
}

pub fn parse(text: &str) -> Result<File, String> {
    let f: File = toml::from_str(text).map_err(|e| e.to_string())?;
    if f.probe.is_empty() {
        return Err("no [[probe]] entries".into());
    }
    if f.guardian.interval_s < 5 {
        return Err("interval_s must be >= 5".into());
    }
    match f.guardian.store.as_str() {
        "jsonl" | "sqlite" => {}
        o => return Err(format!("unknown store {o}")),
    }
    Ok(f)
}
