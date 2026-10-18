// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! boot.toml: the service list both backends use (systemd unit renderer and
//! the plain supervisor for Termux / minimal Linux without systemd).

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    #[serde(default)]
    pub boot: Boot,
    #[serde(default)]
    pub service: Vec<Service>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Boot {
    /// Supervisor logs (`<log_dir>/<name>.log`).
    pub log_dir: PathBuf,
    /// Supervisor state: pids, restart requests from galion-healer.
    pub supervisor_dir: PathBuf,
}

impl Default for Boot {
    fn default() -> Self {
        Boot {
            log_dir: PathBuf::from("target/test-out/boot/logs"),
            supervisor_dir: PathBuf::from("target/test-out/supervisor"),
        }
    }
}

fn d_restart() -> String {
    "always".into()
}
fn d_sec() -> u64 {
    5
}
fn d_type() -> String {
    "simple".into()
}
fn d_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Health {
    #[serde(default = "d_host")]
    pub host: String,
    pub port: u16,
    pub path: String,
    #[serde(default)]
    pub ok_codes: Vec<u16>,
}

fn d_host() -> String {
    "127.0.0.1".into()
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    pub description: String,
    /// Program and arguments. No shell, ever.
    pub argv: Vec<String>,
    #[serde(default)]
    pub workdir: Option<String>,
    /// `KEY=VALUE` pairs. Never secrets: use `env_files`.
    #[serde(default)]
    pub env: Vec<String>,
    /// Env files (secrets live here; never printed, never on argv).
    #[serde(default)]
    pub env_files: Vec<String>,
    /// `always`, `on-failure` or `no`.
    #[serde(default = "d_restart")]
    pub restart: String,
    #[serde(default = "d_sec")]
    pub restart_sec: u64,
    /// `simple` or `oneshot`.
    #[serde(default = "d_type", rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub user: Option<String>,
    /// Hardening: paths writable under ProtectSystem=strict.
    #[serde(default)]
    pub read_write: Vec<String>,
    #[serde(default)]
    pub memory_max: Option<String>,
    #[serde(default)]
    pub nice: Option<i8>,
    /// Render a `.timer` that starts this (oneshot) service every N seconds.
    #[serde(default)]
    pub timer_every_s: Option<u64>,
    #[serde(default)]
    pub health: Option<Health>,
    /// Include in `supervise` (non-systemd backend).
    #[serde(default = "d_true")]
    pub supervise: bool,
    /// Hardening on (NoNewPrivileges, ProtectSystem=strict, PrivateTmp...).
    #[serde(default = "d_true")]
    pub harden: bool,
}

pub fn parse(text: &str) -> Result<File, String> {
    let f: File = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut seen = std::collections::BTreeSet::new();
    for s in &f.service {
        if s.name.is_empty()
            || !s
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        {
            return Err(format!("bad service name {:?}", s.name));
        }
        if !seen.insert(s.name.clone()) {
            return Err(format!("duplicate service {}", s.name));
        }
        if s.argv.is_empty() || !s.argv[0].starts_with('/') {
            return Err(format!("{}: argv[0] must be an absolute path", s.name));
        }
        if !matches!(s.restart.as_str(), "always" | "on-failure" | "no") {
            return Err(format!("{}: bad restart {}", s.name, s.restart));
        }
        if !matches!(s.kind.as_str(), "simple" | "oneshot") {
            return Err(format!("{}: bad type {}", s.name, s.kind));
        }
        if s.env.iter().any(|e| !e.contains('=')) {
            return Err(format!("{}: env entries are KEY=VALUE", s.name));
        }
        let joined = s.argv.join(" ").to_lowercase();
        if joined.contains("--token") || joined.contains("lock-secrets") {
            return Err(format!(
                "{}: secrets on argv / lock-secrets refused",
                s.name
            ));
        }
    }
    Ok(f)
}
