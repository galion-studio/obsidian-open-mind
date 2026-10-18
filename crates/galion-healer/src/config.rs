// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Healer config. Safe defaults: state under target/test-out, dry-run,
//! no restart allow-list, probes on 127.0.0.1.

use serde::Deserialize;
use std::path::PathBuf;

/// Names the healer must never act on (heal-apply.py/heal-dry.py PROTECTED
/// plus the services B17 lists as out of scope).
pub const PROTECTED_UNITS: &[&str] = &[
    "gateway",
    "hermes",
    "whisper",
    "vision",
    "keep.sh",
    "cloudflared",
    "ssh",
    "memory",
];

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct File {
    #[serde(default)]
    pub healer: Settings,
    #[serde(default)]
    pub probe: Vec<Probe>,
    #[serde(default)]
    pub restart: Vec<RestartRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    pub interval_s: u64,
    /// desk.jsonl, nerve.jsonl, open.json, pulse.json, inbox/, actions.jsonl.
    pub state_dir: PathBuf,
    /// Seconds before the follow-up check (desk_heal.py FOLLOW_S).
    pub follow_s: u64,
    /// false = every action is logged as dry-run and nothing is changed.
    pub apply: bool,
    /// Hermes home for the stale gateway lock repair. Empty = action off.
    pub hermes_home: String,
    /// Probe name that means "the gateway" (lock repair trigger).
    pub gateway_probe: String,
    /// heal-apply.py fixture root; json repairs are confined to it.
    pub fixture_root: PathBuf,
    /// "systemd" (systemctl restart), "supervisor" (request file for
    /// galion-boot supervise) or "none".
    pub restart_backend: String,
    /// Request directory for the supervisor backend.
    pub supervisor_dir: PathBuf,
    /// Minimum seconds between two restarts of one unit.
    pub restart_min_gap_s: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            interval_s: 30,
            state_dir: PathBuf::from("target/test-out/healer"),
            follow_s: 60,
            apply: false,
            hermes_home: String::new(),
            gateway_probe: "hermes".into(),
            fixture_root: PathBuf::from("target/test-out/healer/heal-fixture"),
            restart_backend: "none".into(),
            supervisor_dir: PathBuf::from("target/test-out/supervisor"),
            restart_min_gap_s: 600,
        }
    }
}

fn d_host() -> String {
    "127.0.0.1".into()
}
fn d_codes() -> Vec<u16> {
    vec![200]
}
fn d_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub name: String,
    #[serde(default = "d_host")]
    pub host: String,
    pub port: u16,
    pub path: String,
    #[serde(default = "d_codes")]
    pub ok_codes: Vec<u16>,
    #[serde(default = "d_true")]
    pub loading_ok: bool,
}

/// Allow-list entry: restart `unit` after `after_cycles` failed cycles of probe.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RestartRule {
    pub probe: String,
    pub unit: String,
    #[serde(default = "d_after")]
    pub after_cycles: u32,
}

fn d_after() -> u32 {
    4
}

pub fn is_protected(name: &str) -> bool {
    let low = name.to_lowercase();
    PROTECTED_UNITS.iter().any(|p| low.contains(p))
}

pub fn parse(text: &str) -> Result<File, String> {
    let f: File = toml::from_str(text).map_err(|e| e.to_string())?;
    if f.probe.is_empty() {
        return Err("no [[probe]] entries".into());
    }
    if f.healer.interval_s < 5 {
        return Err("interval_s must be >= 5".into());
    }
    match f.healer.restart_backend.as_str() {
        "systemd" | "supervisor" | "none" => {}
        o => return Err(format!("unknown restart_backend {o}")),
    }
    for r in &f.restart {
        if is_protected(&r.unit) || is_protected(&r.probe) {
            return Err(format!("restart of {} refused: protected", r.unit));
        }
        if !r
            .unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.@".contains(c))
        {
            return Err(format!("bad unit name {}", r.unit));
        }
        if !f.probe.iter().any(|p| p.name == r.probe) {
            return Err(format!("restart rule for unknown probe {}", r.probe));
        }
    }
    Ok(f)
}
