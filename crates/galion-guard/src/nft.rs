// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Firewall backends.
//!
//! `Nft` drives the system `nft` binary (argv only, script on stdin, never a
//! shell; every address in a script is a parsed `IpAddr`, so nothing from a
//! log line reaches nft as text). See docs/guard-rust.md "nftables backend"
//! for why this beats the Rust netlink crates today.
//!
//! Layout (own table, independent of fail2ban's `f2b-table`):
//! ```text
//! table inet <table> {
//!   set addr4 { type ipv4_addr; flags timeout; }
//!   set addr6 { type ipv6_addr; flags timeout; }
//!   chain input { type filter hook input priority filter - 1; policy accept;
//!     ip saddr 127.0.0.0/8 accept; ip6 saddr ::1 accept
//!     ip saddr { <allowlist v4> } accept; ip6 saddr { <allowlist v6> } accept
//!     tcp dport { 22 } ip saddr @addr4 reject
//!     tcp dport { 22 } ip6 saddr @addr6 reject } }
//! ```
//! Every element carries a kernel timeout, so bans lift themselves even if
//! the daemon is dead.

use std::collections::BTreeMap;
use std::io::Write;
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::ipnet::Net;

pub trait Backend: Send {
    /// Create or refresh table, sets and rules (idempotent, keeps elements).
    fn setup(&mut self) -> Result<(), String>;
    /// Ban for `timeout` seconds (replaces an existing element's timeout).
    fn ban(&mut self, ip: IpAddr, timeout: u64) -> Result<(), String>;
    /// Remove a ban; removing an absent element is not an error.
    fn unban(&mut self, ip: IpAddr) -> Result<(), String>;
    /// Current elements with remaining seconds (if the kernel reports it).
    fn list(&mut self) -> Result<Vec<(IpAddr, Option<u64>)>, String>;
    /// Remove our table entirely (operator `teardown`).
    fn teardown(&mut self) -> Result<(), String>;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct NftConfig {
    pub bin: PathBuf,
    pub table: String,
    pub ports: Vec<u16>,
    pub allow: Vec<Net>,
    /// `reject` (like fail2ban) or `drop`.
    pub blocktype: String,
}

/// Table names are interpolated into scripts, so they are restricted.
pub fn valid_table(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn setup_script(c: &NftConfig) -> String {
    let t = &c.table;
    let ports = c
        .ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let block = if c.blocktype == "drop" {
        "drop"
    } else {
        "reject"
    };
    let v4: Vec<String> = c
        .allow
        .iter()
        .filter(|n| n.is_v4())
        .map(|n| n.to_string())
        .collect();
    let v6: Vec<String> = c
        .allow
        .iter()
        .filter(|n| !n.is_v4())
        .map(|n| n.to_string())
        .collect();
    let mut s = String::new();
    s += &format!("add table inet {t}\n");
    s += &format!("add set inet {t} addr4 {{ type ipv4_addr; flags timeout; }}\n");
    s += &format!("add set inet {t} addr6 {{ type ipv6_addr; flags timeout; }}\n");
    s += &format!(
        "add chain inet {t} input {{ type filter hook input priority filter - 1; policy accept; }}\n"
    );
    s += &format!("flush chain inet {t} input\n");
    s += &format!("add rule inet {t} input ip saddr 127.0.0.0/8 accept\n");
    s += &format!("add rule inet {t} input ip6 saddr ::1 accept\n");
    if !v4.is_empty() {
        s += &format!(
            "add rule inet {t} input ip saddr {{ {} }} accept\n",
            v4.join(", ")
        );
    }
    if !v6.is_empty() {
        s += &format!(
            "add rule inet {t} input ip6 saddr {{ {} }} accept\n",
            v6.join(", ")
        );
    }
    s += &format!("add rule inet {t} input tcp dport {{ {ports} }} ip saddr @addr4 {block}\n");
    s += &format!("add rule inet {t} input tcp dport {{ {ports} }} ip6 saddr @addr6 {block}\n");
    s
}

fn set_of(ip: IpAddr) -> &'static str {
    if ip.is_ipv4() {
        "addr4"
    } else {
        "addr6"
    }
}

pub fn ban_script(table: &str, ip: IpAddr, timeout: u64) -> String {
    let set = set_of(ip);
    // add + delete + add in one transaction = "upsert" with a fresh timeout.
    format!(
        "add element inet {table} {set} {{ {ip} timeout {t}s }}\n\
         delete element inet {table} {set} {{ {ip} }}\n\
         add element inet {table} {set} {{ {ip} timeout {t}s }}\n",
        t = timeout.max(1)
    )
}

pub fn unban_script(table: &str, ip: IpAddr) -> String {
    let set = set_of(ip);
    // add first so the delete never fails on an absent element.
    format!(
        "add element inet {table} {set} {{ {ip} timeout 1s }}\n\
         delete element inet {table} {set} {{ {ip} }}\n"
    )
}

pub struct Nft {
    pub cfg: NftConfig,
}

impl Nft {
    pub fn new(cfg: NftConfig) -> Result<Self, String> {
        if !valid_table(&cfg.table) {
            return Err(format!("invalid nft table name {:?}", cfg.table));
        }
        if cfg.ports.is_empty() {
            return Err("no ports configured".into());
        }
        Ok(Nft { cfg })
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new(&self.cfg.bin);
        cmd.args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", self.cfg.bin.display()))?;
        if let Some(s) = stdin {
            if let Some(mut w) = child.stdin.take() {
                w.write_all(s.as_bytes())
                    .map_err(|e| format!("nft stdin: {e}"))?;
            }
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("nft wait: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(format!(
                "nft failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    fn script(&self, s: &str) -> Result<(), String> {
        self.run(&["-f", "-"], Some(s)).map(|_| ())
    }
}

impl Backend for Nft {
    fn setup(&mut self) -> Result<(), String> {
        self.script(&setup_script(&self.cfg))
    }
    fn ban(&mut self, ip: IpAddr, timeout: u64) -> Result<(), String> {
        self.script(&ban_script(&self.cfg.table, ip, timeout))
    }
    fn unban(&mut self, ip: IpAddr) -> Result<(), String> {
        self.script(&unban_script(&self.cfg.table, ip))
    }
    fn list(&mut self) -> Result<Vec<(IpAddr, Option<u64>)>, String> {
        let mut out = Vec::new();
        for set in ["addr4", "addr6"] {
            let j = self.run(&["-j", "list", "set", "inet", &self.cfg.table, set], None)?;
            out.extend(parse_set_json(&j)?);
        }
        Ok(out)
    }
    fn teardown(&mut self) -> Result<(), String> {
        let t = &self.cfg.table;
        // "add" first so deleting a missing table is not an error.
        self.script(&format!("add table inet {t}\ndelete table inet {t}\n"))
    }
    fn name(&self) -> &'static str {
        "nft"
    }
}

/// Parse `nft -j list set` output: elements are plain strings or
/// `{"elem": {"val": "...", "timeout": n, "expires": n}}`.
pub fn parse_set_json(j: &str) -> Result<Vec<(IpAddr, Option<u64>)>, String> {
    let v: serde_json::Value = serde_json::from_str(j).map_err(|e| format!("nft json: {e}"))?;
    let mut out = Vec::new();
    let items = v
        .get("nftables")
        .and_then(|n| n.as_array())
        .ok_or("nft json: no nftables array")?;
    for it in items {
        let Some(elems) = it
            .get("set")
            .and_then(|s| s.get("elem"))
            .and_then(|e| e.as_array())
        else {
            continue;
        };
        for e in elems {
            let (val, exp) = match e {
                serde_json::Value::String(s) => (s.as_str(), None),
                serde_json::Value::Object(o) => {
                    let inner = o.get("elem").unwrap_or(e);
                    (
                        inner.get("val").and_then(|v| v.as_str()).unwrap_or(""),
                        inner.get("expires").and_then(|x| x.as_u64()),
                    )
                }
                _ => continue,
            };
            if let Ok(ip) = val.parse::<IpAddr>() {
                out.push((ip, exp));
            }
        }
    }
    Ok(out)
}

/// In-memory firewall that writes the nft scripts it would run to a log.
/// Used for tests and for `backend = "dry-run"` shadow runs next to fail2ban.
pub struct DryRun {
    pub table: String,
    pub cfg: Option<NftConfig>,
    pub log: Option<PathBuf>,
    /// ip -> absolute expiry (unix seconds).
    pub elems: BTreeMap<IpAddr, i64>,
    pub calls: Vec<String>,
}

impl DryRun {
    pub fn new(cfg: Option<NftConfig>, log: Option<PathBuf>) -> Self {
        DryRun {
            table: cfg
                .as_ref()
                .map_or_else(|| "galion-guard".into(), |c| c.table.clone()),
            cfg,
            log,
            elems: BTreeMap::new(),
            calls: Vec::new(),
        }
    }

    fn record(&mut self, s: String) {
        if let Some(p) = &self.log {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                let _ = writeln!(f, "# {}\n{}", zagreb_time::now_rfc3339(), s.trim_end());
            }
        }
        self.calls.push(s);
    }
}

impl Backend for DryRun {
    fn setup(&mut self) -> Result<(), String> {
        let s = match &self.cfg {
            Some(c) => setup_script(c),
            None => format!("add table inet {}\n", self.table),
        };
        self.record(s);
        Ok(())
    }
    fn ban(&mut self, ip: IpAddr, timeout: u64) -> Result<(), String> {
        self.elems
            .insert(ip, crate::state::now_secs() + timeout as i64);
        let s = ban_script(&self.table, ip, timeout);
        self.record(s);
        Ok(())
    }
    fn unban(&mut self, ip: IpAddr) -> Result<(), String> {
        self.elems.remove(&ip);
        let s = unban_script(&self.table, ip);
        self.record(s);
        Ok(())
    }
    fn list(&mut self) -> Result<Vec<(IpAddr, Option<u64>)>, String> {
        let now = crate::state::now_secs();
        self.elems.retain(|_, e| *e > now);
        Ok(self
            .elems
            .iter()
            .map(|(ip, e)| (*ip, Some((*e - now).max(0) as u64)))
            .collect())
    }
    fn teardown(&mut self) -> Result<(), String> {
        self.elems.clear();
        let s = format!("delete table inet {}\n", self.table);
        self.record(s);
        Ok(())
    }
    fn name(&self) -> &'static str {
        "dry-run"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts() {
        let c = NftConfig {
            bin: "nft".into(),
            table: "galion-guard".into(),
            ports: vec![22],
            allow: vec![
                Net::parse("198.51.100.0/24").unwrap(),
                Net::parse("2001:db8::/32").unwrap(),
            ],
            blocktype: "reject".into(),
        };
        let s = setup_script(&c);
        assert!(s.contains("flags timeout"));
        assert!(s.contains("ip saddr 127.0.0.0/8 accept"));
        assert!(s.contains("ip saddr { 198.51.100.0/24 } accept"));
        assert!(s.contains("ip6 saddr { 2001:db8::/32 } accept"));
        assert!(s.contains("tcp dport { 22 } ip saddr @addr4 reject"));
        // Accept rules come before reject rules.
        assert!(s.find("127.0.0.0/8 accept").unwrap() < s.find("@addr4 reject").unwrap());
        let b = ban_script("galion-guard", "203.0.113.1".parse().unwrap(), 3600);
        assert_eq!(b.matches("timeout 3600s").count(), 2);
        assert!(ban_script("t", "2001:db8::1".parse().unwrap(), 0)
            .contains("addr6 { 2001:db8::1 timeout 1s }"));
        assert!(unban_script("t", "203.0.113.1".parse().unwrap())
            .contains("delete element inet t addr4 { 203.0.113.1 }"));
    }

    #[test]
    fn table_names() {
        assert!(valid_table("galion-guard"));
        assert!(!valid_table("x; flush ruleset"));
        assert!(!valid_table(""));
    }

    #[test]
    fn parses_set_json() {
        let j = r#"{"nftables": [{"metainfo": {"version": "1.1.3"}}, {"set": {"family": "inet", "name": "addr4", "table": "t", "type": "ipv4_addr", "handle": 1, "flags": ["timeout"], "elem": [{"elem": {"val": "1.2.3.4", "timeout": 5, "expires": 4}}, "5.6.7.8"]}}]}"#; // repo-check: allow (test fixture address, never dialled)
        let v = parse_set_json(j).unwrap();
        assert_eq!(
            v,
            vec![
                ("1.2.3.4".parse().unwrap(), Some(4)), // repo-check: allow (test fixture address, never dialled)
                ("5.6.7.8".parse().unwrap(), None) // repo-check: allow (test fixture address, never dialled)
            ]
        );
        let empty = r#"{"nftables": [{"metainfo": {}}, {"set": {"name": "addr6"}}]}"#;
        assert!(parse_set_json(empty).unwrap().is_empty());
        assert!(parse_set_json("nope").is_err());
    }
}
