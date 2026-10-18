// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! One small config loader for OpenMind binaries.
//!
//! Order: TOML file (`$OPENMIND_CONFIG`, default `/etc/openmind/openmind.toml`;
//! a missing file means defaults), then env overrides
//! `OPENMIND_<SECTION>_<KEY>`. Defaults are loopback only.

use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

/// Default config file location.
pub const DEFAULT_PATH: &str = "/etc/openmind/openmind.toml";

/// Hostnames or addresses of services.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Hosts {
    pub ollama: String,
    pub hermes: String,
    /// Optional remote probe host. `None` unless configured.
    pub remote_probe: Option<String>,
}

impl Default for Hosts {
    fn default() -> Self {
        Hosts {
            ollama: "127.0.0.1".into(),
            hermes: "127.0.0.1".into(),
            remote_probe: None,
        }
    }
}

/// Service ports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Ports {
    pub ollama: u16,
    pub hermes: u16,
    pub whisper: u16,
    pub docs: u16,
    pub one: u16,
    pub desk: u16,
    pub console: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Ports {
            ollama: 11434,
            hermes: 8642,
            whisper: 8643,
            docs: 8645,
            one: 8091,
            desk: 8646,
            console: 8080,
        }
    }
}

/// Filesystem locations. State files live under `base`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Paths {
    pub base: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Paths {
            base: PathBuf::from("/var/lib/openmind"),
        }
    }
}

impl Paths {
    /// A state file path under the configured base.
    pub fn state_file(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }
}

/// Resource limits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub max_body_bytes: u64,
    pub request_timeout_secs: u64,
    pub max_connections: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_body_bytes: 1_048_576,
            request_timeout_secs: 30,
            max_connections: 64,
        }
    }
}

/// The whole config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hosts: Hosts,
    pub ports: Ports,
    pub paths: Paths,
    pub limits: Limits,
}

/// Load errors.
#[derive(Debug)]
pub enum ConfigError {
    /// File exists but could not be read.
    Read { file: PathBuf, message: String },
    /// File is not valid config TOML (message includes line and key info).
    Parse { file: PathBuf, message: String },
    /// An env override has an unusable value.
    Env {
        var: String,
        value: String,
        reason: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read { file, message } => {
                write!(f, "cannot read config file {}: {}", file.display(), message)
            }
            ConfigError::Parse { file, message } => {
                write!(f, "bad config file {}: {}", file.display(), message)
            }
            ConfigError::Env { var, value, reason } => {
                write!(f, "bad env value {var}={value:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

fn num<T: std::str::FromStr>(var: &str, value: &str, what: &str) -> Result<T, ConfigError> {
    value.trim().parse::<T>().map_err(|_| ConfigError::Env {
        var: var.into(),
        value: value.into(),
        reason: format!("expected {what}"),
    })
}

fn host(var: &str, value: &str) -> Result<String, ConfigError> {
    let v = value.trim();
    if v.is_empty() || v.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(ConfigError::Env {
            var: var.into(),
            value: value.into(),
            reason: "expected a host name or address".into(),
        });
    }
    Ok(v.to_string())
}

impl Config {
    /// Parse TOML text; `file` is used only for error messages.
    pub fn from_toml(text: &str, file: &Path) -> Result<Config, ConfigError> {
        toml::from_str(text).map_err(|e| ConfigError::Parse {
            file: file.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Apply `OPENMIND_<SECTION>_<KEY>` overrides from `env`.
    pub fn apply_env(&mut self, env: &dyn Fn(&str) -> Option<String>) -> Result<(), ConfigError> {
        macro_rules! s {
            ($var:literal, $slot:expr) => {
                if let Some(v) = env($var) {
                    $slot = host($var, &v)?;
                }
            };
        }
        macro_rules! n {
            ($var:literal, $slot:expr, $what:literal) => {
                if let Some(v) = env($var) {
                    $slot = num($var, &v, $what)?;
                }
            };
        }
        s!("OPENMIND_HOSTS_OLLAMA", self.hosts.ollama);
        s!("OPENMIND_HOSTS_HERMES", self.hosts.hermes);
        if let Some(v) = env("OPENMIND_HOSTS_REMOTE_PROBE") {
            self.hosts.remote_probe = if v.trim().is_empty() {
                None
            } else {
                Some(host("OPENMIND_HOSTS_REMOTE_PROBE", &v)?)
            };
        }
        n!("OPENMIND_PORTS_OLLAMA", self.ports.ollama, "a port 0-65535");
        n!("OPENMIND_PORTS_HERMES", self.ports.hermes, "a port 0-65535");
        n!(
            "OPENMIND_PORTS_WHISPER",
            self.ports.whisper,
            "a port 0-65535"
        );
        n!("OPENMIND_PORTS_DOCS", self.ports.docs, "a port 0-65535");
        n!("OPENMIND_PORTS_ONE", self.ports.one, "a port 0-65535");
        n!("OPENMIND_PORTS_DESK", self.ports.desk, "a port 0-65535");
        n!(
            "OPENMIND_PORTS_CONSOLE",
            self.ports.console,
            "a port 0-65535"
        );
        if let Some(v) = env("OPENMIND_PATHS_BASE") {
            if v.trim().is_empty() {
                return Err(ConfigError::Env {
                    var: "OPENMIND_PATHS_BASE".into(),
                    value: v,
                    reason: "expected a non-empty path".into(),
                });
            }
            self.paths.base = PathBuf::from(v);
        }
        n!(
            "OPENMIND_LIMITS_MAX_BODY_BYTES",
            self.limits.max_body_bytes,
            "a non-negative integer"
        );
        n!(
            "OPENMIND_LIMITS_REQUEST_TIMEOUT_SECS",
            self.limits.request_timeout_secs,
            "a non-negative integer"
        );
        n!(
            "OPENMIND_LIMITS_MAX_CONNECTIONS",
            self.limits.max_connections,
            "a non-negative integer"
        );
        Ok(())
    }
}

/// Load from `path` (None or missing file = defaults), then apply `env`.
pub fn load_from(
    path: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Config, ConfigError> {
    let mut cfg = match path {
        None => Config::default(),
        Some(p) => match std::fs::read_to_string(p) {
            Ok(text) => Config::from_toml(&text, p)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => {
                return Err(ConfigError::Read {
                    file: p.to_path_buf(),
                    message: e.to_string(),
                })
            }
        },
    };
    cfg.apply_env(env)?;
    Ok(cfg)
}

/// Load using the process env: file at `$OPENMIND_CONFIG` (default
/// `/etc/openmind/openmind.toml`), then env overrides.
pub fn load() -> Result<Config, ConfigError> {
    let env = |k: &str| std::env::var(k).ok();
    let path = env("OPENMIND_CONFIG").unwrap_or_else(|| DEFAULT_PATH.to_string());
    load_from(Some(Path::new(&path)), &env)
}
