// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! omnode configuration (TOML). No hosts are hard-coded: every URL comes from
//! here or from the command line. Missing file = defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub node: NodeCfg,
    pub network: NetworkCfg,
    pub discover: DiscoverCfg,
    pub announce: AnnounceCfg,
    pub sandbox: SandboxCfg,
    pub pair: PairCfg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodeCfg {
    /// Human label shown to Hermes at enrollment (optional).
    pub name: Option<String>,
    /// Advertised capabilities.
    pub capabilities: Vec<String>,
}

impl Default for NodeCfg {
    fn default() -> Self {
        Self {
            name: None,
            capabilities: vec!["cpu".into(), "sandbox".into(), "voice-client".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkCfg {
    /// Optional stack-wide salt (hex, >= 16 bytes) handed out at enrollment so
    /// Hermes can tell two devices are on the same trusted network. When unset,
    /// a per-device random salt (kept in node-state.toml) is used.
    pub salt_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscoverCfg {
    /// Passive listening window in seconds.
    pub seconds: u64,
    /// Send one SSDP M-SEARCH (one multicast UDP packet) at start.
    pub ssdp_msearch: bool,
    /// Send one active mDNS query packet (off by default).
    pub mdns_query: bool,
    /// Ports for the opt-in TCP probe (max 16 used).
    pub probe_ports: Vec<u16>,
    /// Max TCP connection attempts per second during a probe.
    pub probe_rate_per_sec: u32,
    pub probe_timeout_ms: u64,
}

impl Default for DiscoverCfg {
    fn default() -> Self {
        Self {
            seconds: 5,
            ssdp_msearch: true,
            mdns_query: false,
            // ssh, http, https, alt-http, ollama, llama.cpp rpc-server, exo
            probe_ports: vec![22, 80, 443, 8080, 11434, 50052, 52415],
            probe_rate_per_sec: 20,
            probe_timeout_ms: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnnounceCfg {
    pub service_type: String,
    /// Port written into the SRV record. Nothing listens there yet (v0.1).
    pub port: u16,
}

impl Default for AnnounceCfg {
    fn default() -> Self {
        Self {
            service_type: "_openmind._tcp.local.".into(),
            port: 7787,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SandboxCfg {
    pub wall_secs: u64,
    pub cpu_secs: u64,
    pub mem_mb: u64,
    pub fsize_mb: u64,
    pub nofile: u64,
    pub max_output_kb: usize,
    pub allow_net: bool,
    /// Fail instead of falling back when no network isolation is available.
    pub require_isolation: bool,
    pub landlock: bool,
    /// Install the seccomp deny-list filter (dangerous syscalls -> EPERM).
    pub seccomp: bool,
    /// Use user/net/mount/pid namespaces when the kernel allows them.
    pub namespaces: bool,
    /// Extra read-only paths granted under Landlock.
    pub extra_ro_paths: Vec<PathBuf>,
}

impl Default for SandboxCfg {
    fn default() -> Self {
        Self {
            wall_secs: 30,
            cpu_secs: 20,
            mem_mb: 512,
            fsize_mb: 64,
            nofile: 256,
            max_output_kb: 256,
            allow_net: false,
            require_isolation: false,
            landlock: true,
            seccomp: true,
            namespaces: true,
            extra_ro_paths: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PairCfg {
    /// Default enrollment URL if `omnode pair` is called without one.
    pub enroll_url: Option<String>,
}

/// ~/.config/openmind (or $XDG_CONFIG_HOME/openmind).
pub fn config_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(x).join("openmind");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("openmind")
}

pub fn default_config_path() -> PathBuf {
    std::env::var_os("OMNODE_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir().join("node.toml"))
}

pub fn load(path: Option<&std::path::Path>) -> Result<Config, String> {
    let explicit = path.is_some();
    let p = path.map(PathBuf::from).unwrap_or_else(default_config_path);
    match std::fs::read_to_string(&p) {
        Ok(s) => toml::from_str(&s).map_err(|e| format!("config {}: {e}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => Ok(Config::default()),
        Err(e) => Err(format!("config {}: {e}", p.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_polite() {
        let c = Config::default();
        assert!(!c.discover.mdns_query, "active mDNS query must be opt-in");
        assert!(!c.sandbox.allow_net, "sandbox must default to no network");
        assert!(c.discover.probe_ports.len() <= 16);
        assert!(c.discover.probe_rate_per_sec <= 50);
    }

    #[test]
    fn parses_partial_toml() {
        let c: Config = toml::from_str(
            "[sandbox]\nmem_mb = 128\n[pair]\nenroll_url = \"https://example.invalid/enroll\"\n",
        )
        .unwrap();
        assert_eq!(c.sandbox.mem_mb, 128);
        assert_eq!(c.sandbox.wall_secs, 30);
        assert_eq!(
            c.pair.enroll_url.as_deref(),
            Some("https://example.invalid/enroll")
        );
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("[sandbox]\nmem = 1\n").is_err());
    }

    #[test]
    fn example_config_parses() {
        let s = include_str!("../omnode.example.toml");
        let c: Config = toml::from_str(s).unwrap();
        assert_eq!(c.announce.service_type, "_openmind._tcp.local.");
    }
}
