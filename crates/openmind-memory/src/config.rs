// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Configuration from the environment only, loopback defaults.
//! `OPENMIND_MEMORY_*` wins; the gateway's own `TDAI_*` names are read as a
//! fallback so the existing `/etc/memory-gateway.env` works unchanged.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub instance_id: String,
    /// Optional shared Bearer secret. Never logged.
    pub api_key: Option<String>,
    pub read_only: bool,
    pub max_body: usize,
    pub max_conns: usize,
    pub recall_max_results: usize,
    pub recall_threshold: f64,
    pub recall_strategy: String,
    /// Opt-in: also put the L1 keyword hits into `/recall` `context`
    /// (the Node gateway computes them but drops them; default keeps parity).
    pub recall_include_l1: bool,
    /// Opt-in replay/import mode for `/capture` (`"import": true` in the body
    /// is ignored unless this is set): no cursor filter, so old timestamps are
    /// kept, plus an idempotent dedupe. Default off = gateway behaviour.
    pub allow_import: bool,
}

impl Config {
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Config {
        let pick = |names: &[&str]| {
            names.iter().find_map(|n| {
                get(n)
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
        };
        let truthy = |names: &[&str]| {
            pick(names).is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
        };
        let home = get("HOME").unwrap_or_else(|| ".".into());
        let data_dir = pick(&["OPENMIND_MEMORY_DATA_DIR", "TDAI_DATA_DIR"])
            .map(|d| match d.strip_prefix("~/") {
                Some(rest) => PathBuf::from(&home).join(rest),
                None => PathBuf::from(d),
            })
            .unwrap_or_else(|| PathBuf::from(&home).join(".openmind").join("memory"));
        let strategy = pick(&["OPENMIND_MEMORY_RECALL_STRATEGY"])
            .filter(|s| matches!(s.as_str(), "keyword" | "embedding" | "hybrid"))
            .unwrap_or_else(|| "hybrid".into());
        Config {
            host: pick(&["OPENMIND_MEMORY_HOST", "TDAI_GATEWAY_HOST"])
                .unwrap_or_else(|| "127.0.0.1".into()),
            port: pick(&["OPENMIND_MEMORY_PORT", "TDAI_GATEWAY_PORT"])
                .and_then(|p| p.parse().ok())
                .unwrap_or(8420),
            data_dir,
            instance_id: pick(&["OPENMIND_MEMORY_INSTANCE_ID", "TDAI_INSTANCE_ID"])
                .unwrap_or_else(|| "default".into()),
            api_key: pick(&["OPENMIND_MEMORY_API_KEY", "TDAI_GATEWAY_API_KEY"]),
            read_only: truthy(&["OPENMIND_MEMORY_READ_ONLY"]),
            max_body: pick(&["MEMORY_MAX_BODY_BYTES"])
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(1024 * 1024),
            max_conns: pick(&["OPENMIND_MEMORY_MAX_CONNS"])
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(32),
            recall_max_results: pick(&["OPENMIND_MEMORY_RECALL_MAX_RESULTS"])
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            recall_threshold: pick(&["OPENMIND_MEMORY_RECALL_THRESHOLD"])
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.3),
            recall_strategy: strategy,
            recall_include_l1: truthy(&["OPENMIND_MEMORY_RECALL_INCLUDE_L1"]),
            allow_import: truthy(&["OPENMIND_MEMORY_ALLOW_IMPORT"]),
        }
    }

    /// Same rule as the gateway's store factory.
    pub fn db_path(&self) -> PathBuf {
        if self.instance_id.is_empty() || self.instance_id == "default" {
            self.data_dir.join("vectors.db")
        } else {
            self.data_dir
                .join("instances")
                .join(&self.instance_id)
                .join("vectors.db")
        }
    }

    pub fn is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "::1" | "localhost")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_fallbacks() {
        let env = |k: &str| match k {
            "HOME" => Some("/h".to_string()),
            "TDAI_GATEWAY_PORT" => Some("8420".into()),
            "OPENMIND_MEMORY_PORT" => Some("8421".into()),
            "TDAI_DATA_DIR" => Some("~/d".into()),
            "TDAI_INSTANCE_ID" => Some("galion".into()),
            _ => None,
        };
        let c = Config::from_env(&env);
        assert_eq!(c.host, "127.0.0.1");
        assert_eq!(c.port, 8421);
        assert_eq!(
            c.db_path(),
            PathBuf::from("/h/d/instances/galion/vectors.db")
        );
        assert!(c.api_key.is_none() && !c.read_only && c.is_loopback());
        assert!(!c.allow_import, "import mode must be opt-in");
    }
}
