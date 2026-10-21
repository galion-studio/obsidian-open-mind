// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! TOML configuration. Defaults reproduce the VPS fail2ban sshd jail.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::duration;
use crate::ipnet::Net;
use crate::jail::JailParams;

pub const DEFAULT_PATH: &str = "/etc/galion-guard/guard.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Guard {
    /// "nft" or "dry-run".
    pub backend: String,
    pub nft: PathBuf,
    pub table: String,
    /// Dry-run backend: append the nft scripts it would run here.
    pub dry_run_log: Option<PathBuf>,
    pub state: PathBuf,
    pub socket: PathBuf,
    /// fail2ban `ignoreself`: never ban this host's own addresses.
    pub ignoreself: bool,
    /// Seconds between state saves when only counters changed.
    pub save_every: u64,
}

impl Default for Guard {
    fn default() -> Self {
        Guard {
            backend: "nft".into(),
            nft: "/usr/sbin/nft".into(),
            table: "galion-guard".into(),
            dry_run_log: None,
            state: "/var/lib/galion-guard/state.json".into(),
            socket: "/run/galion-guard/control.sock".into(),
            ignoreself: true,
            save_every: 30,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sshd {
    /// "journal" (like the VPS `backend = systemd`) or "file".
    pub source: String,
    pub journalctl: PathBuf,
    /// journalctl match arguments; `+` separates OR groups.
    pub journal_match: Vec<String>,
    pub logpath: PathBuf,
    /// File source: poll interval in milliseconds.
    pub poll_ms: u64,
    /// File source: zone of BSD-format (`Oct 20 18:49:14`) stamps, minutes
    /// east of UTC. RFC 3339 stamps carry their own offset.
    pub bsd_utc_offset_minutes: i64,
    pub maxretry: u32,
    pub findtime: String,
    pub bantime: String,
    pub bantime_increment: bool,
    pub bantime_factor: f64,
    /// Empty = no cap (VPS parity).
    pub bantime_maxtime: String,
    pub purge_age: String,
    /// "0" disables. Addresses with a successful login this recently are
    /// never banned and are unbanned when they log in.
    pub protect_accepted: String,
    pub ignoreip: Vec<String>,
    pub ports: Vec<u16>,
    /// "reject" (VPS parity) or "drop".
    pub blocktype: String,
}

impl Default for Sshd {
    fn default() -> Self {
        Sshd {
            source: "journal".into(),
            journalctl: "/usr/bin/journalctl".into(),
            journal_match: vec![
                "_SYSTEMD_UNIT=sshd.service".into(),
                "+".into(),
                "_SYSTEMD_UNIT=ssh.service".into(),
                "+".into(),
                "_COMM=sshd".into(),
                "+".into(),
                "_COMM=sshd-session".into(),
            ],
            logpath: "/var/log/auth.log".into(),
            poll_ms: 1000,
            bsd_utc_offset_minutes: 0,
            maxretry: 5,
            findtime: "10m".into(),
            bantime: "1h".into(),
            bantime_increment: true,
            bantime_factor: 1.0,
            bantime_maxtime: String::new(),
            purge_age: "1d".into(),
            protect_accepted: "24h".into(),
            ignoreip: vec!["127.0.0.1/8".into(), "::1".into()],
            ports: vec![22],
            blocktype: "reject".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub guard: Guard,
    pub sshd: Sshd,
}

/// Validated, ready to use.
#[derive(Debug, Clone)]
pub struct Effective {
    pub cfg: Config,
    pub params: JailParams,
    pub allow: Vec<Net>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Effective, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Effective, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        cfg.validate()
    }

    pub fn validate(self) -> Result<Effective, String> {
        let s = &self.sshd;
        if !matches!(self.guard.backend.as_str(), "nft" | "dry-run") {
            return Err(format!(
                "guard.backend must be nft or dry-run, not {:?}",
                self.guard.backend
            ));
        }
        if !crate::nft::valid_table(&self.guard.table) {
            return Err(format!(
                "guard.table {:?} is not a valid name",
                self.guard.table
            ));
        }
        if !matches!(s.source.as_str(), "journal" | "file") {
            return Err(format!(
                "sshd.source must be journal or file, not {:?}",
                s.source
            ));
        }
        if !matches!(s.blocktype.as_str(), "reject" | "drop") {
            return Err("sshd.blocktype must be reject or drop".into());
        }
        if s.maxretry == 0 {
            return Err("sshd.maxretry must be >= 1".into());
        }
        if s.ports.is_empty() || s.ports.contains(&0) {
            return Err("sshd.ports must list real ports".into());
        }
        if !(s.bantime_factor.is_finite() && s.bantime_factor > 0.0) {
            return Err("sshd.bantime_factor must be > 0".into());
        }
        let d = |name: &str, v: &str| duration::parse(v).map_err(|e| format!("sshd.{name}: {e}"));
        let findtime = d("findtime", &s.findtime)?;
        let bantime = d("bantime", &s.bantime)?;
        if findtime == 0 || bantime == 0 {
            return Err("sshd.findtime and sshd.bantime must be > 0".into());
        }
        let maxtime = if s.bantime_maxtime.trim().is_empty() {
            None
        } else {
            Some(d("bantime_maxtime", &s.bantime_maxtime)?)
        };
        let purge_age = d("purge_age", &s.purge_age)?;
        let protect_accepted = d("protect_accepted", &s.protect_accepted)?;
        let mut allow = Vec::new();
        for e in &s.ignoreip {
            for tok in e.split([' ', ',']).filter(|t| !t.is_empty()) {
                let n = Net::parse(tok).map_err(|e| format!("sshd.ignoreip: {e}"))?;
                if n.prefix == 0 {
                    return Err("sshd.ignoreip: a /0 entry would disable the jail".into());
                }
                allow.push(n);
            }
        }
        for n in ["127.0.0.0/8", "::1/128"] {
            let n = Net::parse(n).expect("static");
            if !allow.contains(&n) {
                allow.push(n);
            }
        }
        let params = JailParams {
            maxretry: s.maxretry,
            findtime,
            bantime,
            increment: s.bantime_increment,
            factor: s.bantime_factor,
            maxtime,
            purge_age,
            protect_accepted,
        };
        Ok(Effective {
            cfg: self,
            params,
            allow,
        })
    }
}

/// This host's own addresses (fail2ban `ignoreself`), from /proc.
pub fn self_addresses() -> Vec<std::net::IpAddr> {
    let mut out = Vec::new();
    if let Ok(t) = std::fs::read_to_string("/proc/net/fib_trie") {
        out.extend(parse_fib_trie(&t));
    }
    if let Ok(t) = std::fs::read_to_string("/proc/net/if_inet6") {
        out.extend(parse_if_inet6(&t));
    }
    out.sort();
    out.dedup();
    out
}

/// `|-- 10.0.0.5` followed by `/32 host LOCAL` marks a local address.
pub fn parse_fib_trie(t: &str) -> Vec<std::net::IpAddr> {
    let mut out = Vec::new();
    let mut last: Option<std::net::IpAddr> = None;
    for line in t.lines() {
        let l = line.trim();
        if let Some(a) = l.strip_prefix("|-- ").or_else(|| l.strip_prefix("+-- ")) {
            last = a.split_whitespace().next().and_then(|a| a.parse().ok());
        } else if l.contains("host LOCAL") {
            if let Some(a) = last {
                out.push(a);
            }
        }
    }
    out
}

pub fn parse_if_inet6(t: &str) -> Vec<std::net::IpAddr> {
    t.lines()
        .filter_map(|l| {
            let hex = l.split_whitespace().next()?;
            if hex.len() != 32 {
                return None;
            }
            let v = u128::from_str_radix(hex, 16).ok()?;
            Some(std::net::IpAddr::V6(std::net::Ipv6Addr::from(v)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_vps() {
        let e = Config::parse("").unwrap();
        assert_eq!(e.params, JailParams::default());
        assert_eq!(e.cfg.sshd.ports, vec![22]);
        assert_eq!(e.allow.len(), 2);
    }

    #[test]
    fn parses_and_rejects() {
        let e = Config::parse(
            "[guard]\nbackend = \"dry-run\"\n[sshd]\nsource = \"file\"\nmaxretry = 3\nfindtime = \"5m\"\nbantime = \"30s\"\nbantime_maxtime = \"1w\"\nignoreip = [\"198.51.100.0/24 2001:db8::/32\"]\n",
        )
        .unwrap();
        assert_eq!(e.params.maxretry, 3);
        assert_eq!(e.params.maxtime, Some(604_800));
        assert_eq!(e.allow.len(), 4);
        assert!(Config::parse("[sshd]\nignoreip = [\"0.0.0.0/0\"]").is_err());
        assert!(Config::parse("[sshd]\nmaxretry = 0").is_err());
        assert!(Config::parse("[sshd]\nbantime = \"-1\"").is_err());
        assert!(Config::parse("[guard]\ntable = \"x y\"").is_err());
        assert!(Config::parse("[guard]\nbogus = 1").is_err());
        assert!(Config::parse("[sshd]\nports = []").is_err());
    }

    #[test]
    fn proc_parsers() {
        let fib = "Main:\n  +-- 0.0.0.0/0 3 0 5\n     |-- 10.1.2.3\n        /32 host LOCAL\n     |-- 10.1.2.255\n        /32 link BROADCAST\n";
        assert_eq!(
            parse_fib_trie(fib),
            vec!["10.1.2.3".parse::<std::net::IpAddr>().unwrap()]
        );
        let v6 = "00000000000000000000000000000001 01 80 10 80       lo\n20010db8000000000000000000000005 02 40 00 80 eth0\n";
        let v = parse_if_inet6(v6);
        assert_eq!(v[1], "2001:db8::5".parse::<std::net::IpAddr>().unwrap());
    }
}
