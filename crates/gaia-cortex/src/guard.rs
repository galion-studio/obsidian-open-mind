// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Never-act guard (Mate's safety constraints, 2026-10-21).
//!
//! The cortex may alert about anything but never asks for an action on:
//! - the VPS itself, DNS, the mail server, the backup server (hostnames from
//!   `openmind_nerve::protected_hosts`, D-T42-4, plus unit names below);
//! - sshd, cloudflared, networking (a restart is a stop + start);
//! - anything under `/opt/galion/openmind`;
//! - the nerve's never-touch list (`openmind_nerve::reflex::DENY`);
//! - the cortex and the nerve themselves.
//!
//! The only action type is restart (no stop, delete or arbitrary command),
//! and HADES security mode is not something the cortex can address at all.

use openmind_nerve::protected_hosts::ProtectedHosts;
use openmind_nerve::reflex;

/// Protected unit name fragments (substring, case-insensitive), on top of
/// [`reflex::DENY`].
pub const PROTECTED_UNITS: &[&str] = &[
    // remote access + edge
    "ssh",
    "cloudflared",
    // networking
    "network",
    "networking",
    "netplan",
    "wpa_supplicant",
    "dhcp",
    // DNS
    "named",
    "bind9",
    "unbound",
    "dnsmasq",
    "resolved",
    "pdns",
    // mail
    "postfix",
    "exim",
    "dovecot",
    "opendkim",
    "rspamd",
    // backup
    "galion-backup",
    "backup",
    "restic",
    "borg",
    // the control plane itself
    "gaia-cortex",
    "galion-nerve",
    "openmind-nerve",
    "hades",
    // host lifecycle
    "reboot",
    "poweroff",
    "shutdown",
    "halt",
    "kexec",
];

/// No action may ever reference this path.
pub const PROTECTED_PATH: &str = "/opt/galion/openmind";

/// Why something is protected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protected(pub String);

pub struct Guard {
    hosts: ProtectedHosts,
    extra_units: Vec<String>,
}

impl Guard {
    pub fn new(extra_units: &[String]) -> Self {
        Guard {
            hosts: ProtectedHosts::from_env(&|k| std::env::var(k).ok()),
            extra_units: extra_units.iter().map(|s| s.to_ascii_lowercase()).collect(),
        }
    }

    pub fn with_hosts(hosts: ProtectedHosts, extra_units: &[String]) -> Self {
        Guard {
            hosts,
            extra_units: extra_units.iter().map(|s| s.to_ascii_lowercase()).collect(),
        }
    }

    pub fn hosts_json(&self) -> serde_json::Value {
        self.hosts.to_json()
    }

    /// Is this unit off limits for any action?
    pub fn unit(&self, unit: &str) -> Option<Protected> {
        let u = unit.to_ascii_lowercase();
        if u.contains(PROTECTED_PATH) {
            return Some(Protected(format!("{unit}: references {PROTECTED_PATH}")));
        }
        if let Some(p) = PROTECTED_UNITS.iter().find(|p| u.contains(*p)) {
            return Some(Protected(format!("{unit}: protected unit ({p})")));
        }
        if let Some(p) = reflex::DENY.iter().find(|p| u.contains(*p)) {
            return Some(Protected(format!("{unit}: nerve never-touch list ({p})")));
        }
        if let Some(p) = self.extra_units.iter().find(|p| u.contains(p.as_str())) {
            return Some(Protected(format!(
                "{unit}: configured protected unit ({p})"
            )));
        }
        None
    }

    /// Is this host one of the VPS / DNS / mail / backup names?
    pub fn host(&self, host: &str) -> Option<Protected> {
        self.hosts
            .category(host)
            .map(|c| Protected(format!("{host}: protected host ({c})")))
    }

    /// Any string field of an event that names the protected path.
    pub fn text(&self, s: &str) -> Option<Protected> {
        s.contains(PROTECTED_PATH)
            .then(|| Protected(format!("event references {PROTECTED_PATH}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mates_list_is_protected() {
        let g = Guard::with_hosts(ProtectedHosts::builtin(), &[]);
        for u in [
            "ssh.service",
            "sshd.service",
            "cloudflared.service",
            "systemd-networkd.service",
            "networking.service",
            "NetworkManager.service",
            "systemd-resolved.service",
            "postfix.service",
            "galion-backup.service",
            "galion-backup.timer",
            "galion-nerve.service",
            "gaia-cortex.service",
        ] {
            assert!(g.unit(u).is_some(), "{u} must be protected");
        }
        for u in ["tuner.service", "hermes-rs.service", "run-u42.service"] {
            assert!(g.unit(u).is_none(), "{u} must not be protected");
        }
        assert!(g.host("ip50.vp3.titanaxe.com").is_some());
        assert!(g.host("mx.zoho.eu").is_some());
        assert!(g.host("galion.app").is_none());
        assert!(g.text("/opt/galion/openmind/bin/x").is_some());
    }

    #[test]
    fn configured_extra() {
        let g = Guard::with_hosts(ProtectedHosts::builtin(), &["tuner".into()]);
        assert!(g.unit("tuner.service").is_some());
    }
}
