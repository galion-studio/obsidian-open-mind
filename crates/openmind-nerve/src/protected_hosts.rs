// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Never-unroute hostnames (owner decision D-T42-4).
//!
//! The route-removal plan (`ingress-rule-removal`) must never target the VPS
//! itself, DNS, the mail server or the backup server. The names live in
//! config, not in code: `deploy/nerve/plans-protected-hosts.conf` is built in
//! as the default, `NERVE_PLANS_PROTECTED_HOSTS_FILE` replaces it at run time
//! and `NERVE_PLANS_PROTECTED_HOSTS` (comma list) adds names under `extra`.
//! A file that is missing or invalid never weakens the list: the built-in
//! list is kept and the problem is reported in `warning`.

use serde_json::{json, Value};

/// The committed default list.
pub const BUILTIN: &str = include_str!("../../../deploy/nerve/plans-protected-hosts.conf");

/// Categories every list must fill (D-T42-4).
pub const REQUIRED: [&str; 4] = ["vps", "dns", "mail", "backup"];

/// Optional category for run-time additions.
pub const EXTRA: &str = "extra";

/// One never-unroute list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedHosts {
    /// `(category, lower-case host or *.suffix)`, in file order.
    entries: Vec<(String, String)>,
    /// Where the list came from (`builtin` or the file path).
    pub source: String,
    /// Set when a configured file could not be used.
    pub warning: Option<String>,
}

impl Default for ProtectedHosts {
    fn default() -> Self {
        Self::builtin()
    }
}

fn norm(h: &str) -> String {
    h.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// A hostname (or `*.suffix`) we accept in the list: printable ASCII
/// letters, digits, `-`, `.`, and not an address.
fn valid_name(h: &str) -> bool {
    let bare = h.strip_prefix("*.").unwrap_or(h);
    !bare.is_empty()
        && bare.len() <= 253
        && bare
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        && !bare.starts_with('.')
        && !bare.ends_with('.')
        && bare.parse::<std::net::IpAddr>().is_err()
        && !bare.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

impl ProtectedHosts {
    /// The committed list. It is checked by the tests, so it always parses.
    pub fn builtin() -> Self {
        let mut p = Self::parse(BUILTIN).unwrap_or_else(|e| ProtectedHosts {
            entries: vec![],
            source: String::new(),
            warning: Some(format!("built-in list invalid: {e}")),
        });
        p.source = "builtin".into();
        p
    }

    /// Parse `<category> = host[, host...]` lines (`#` comments).
    ///
    /// # Errors
    /// A malformed line, an unknown category, an address or a bad name, or a
    /// required category left empty.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut entries: Vec<(String, String)> = vec![];
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (cat, hosts) = line
                .split_once('=')
                .ok_or(format!("line {}: expected <category> = host, ...", n + 1))?;
            let cat = cat.trim().to_ascii_lowercase();
            if !REQUIRED.contains(&cat.as_str()) && cat != EXTRA {
                return Err(format!("line {}: unknown category {cat:?}", n + 1));
            }
            for h in hosts.split(',').map(norm).filter(|h| !h.is_empty()) {
                if !valid_name(&h) {
                    return Err(format!(
                        "line {}: {h:?} is not a hostname (addresses are not allowed)",
                        n + 1
                    ));
                }
                if !entries.iter().any(|(c, e)| *c == cat && *e == h) {
                    entries.push((cat.clone(), h));
                }
            }
        }
        for r in REQUIRED {
            if !entries.iter().any(|(c, _)| c == r) {
                return Err(format!("category {r:?} is empty"));
            }
        }
        Ok(ProtectedHosts {
            entries,
            source: "inline".into(),
            warning: None,
        })
    }

    /// From the environment: `NERVE_PLANS_PROTECTED_HOSTS_FILE` (replaces the
    /// built-in list when it reads and parses), then
    /// `NERVE_PLANS_PROTECTED_HOSTS` (adds names under `extra`).
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut p = match get("NERVE_PLANS_PROTECTED_HOSTS_FILE").filter(|s| !s.is_empty()) {
            None => Self::builtin(),
            Some(path) => match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| Self::parse(&t))
            {
                Ok(mut p) => {
                    p.source = path;
                    p
                }
                Err(e) => {
                    let mut b = Self::builtin();
                    b.warning = Some(format!(
                        "NERVE_PLANS_PROTECTED_HOSTS_FILE {path}: {e}; built-in list kept"
                    ));
                    b
                }
            },
        };
        if let Some(extra) = get("NERVE_PLANS_PROTECTED_HOSTS") {
            for h in extra.split(',').map(norm).filter(|h| !h.is_empty()) {
                if valid_name(&h) {
                    if !p.entries.iter().any(|(_, e)| *e == h) {
                        p.entries.push((EXTRA.into(), h));
                    }
                } else {
                    let w = format!("NERVE_PLANS_PROTECTED_HOSTS: {h:?} ignored (not a hostname)");
                    p.warning = Some(match p.warning.take() {
                        Some(o) => format!("{o}; {w}"),
                        None => w,
                    });
                }
            }
        }
        p
    }

    /// The category that protects `host`, if any.
    pub fn category(&self, host: &str) -> Option<&str> {
        let h = norm(host);
        self.entries
            .iter()
            .find(|(_, e)| match e.strip_prefix("*.") {
                Some(suffix) => h.len() > suffix.len() + 1 && h.ends_with(&format!(".{suffix}")),
                None => *e == h,
            })
            .map(|(c, _)| c.as_str())
    }

    pub fn is_protected(&self, host: &str) -> bool {
        self.category(host).is_some()
    }

    /// Every protected name, sorted and de-duplicated (for plan text).
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.entries.iter().map(|(_, e)| e.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Names in one category.
    pub fn in_category(&self, cat: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(c, _)| c == cat)
            .map(|(_, e)| e.as_str())
            .collect()
    }

    pub fn to_json(&self) -> Value {
        let mut by = serde_json::Map::new();
        for c in REQUIRED.iter().copied().chain([EXTRA]) {
            let v = self.in_category(c);
            if !v.is_empty() || c != EXTRA {
                by.insert(c.into(), json!(v));
            }
        }
        json!({"source": self.source, "categories": by, "warning": self.warning})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_is_complete_and_has_no_addresses() {
        let p = ProtectedHosts::builtin();
        assert_eq!(p.warning, None);
        for c in REQUIRED {
            assert!(!p.in_category(c).is_empty(), "{c} empty");
        }
        assert!(p.is_protected("ip50.vp3.titanaxe.com"));
        assert!(p.is_protected("MX.Zoho.EU."));
        assert_eq!(p.category("peyton.ns.cloudflare.com"), Some("dns"));
        assert!(!p.is_protected("galion.app"));
        assert!(!BUILTIN
            .split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .any(|t| t.split('.').filter(|x| !x.is_empty()).count() == 4));
    }

    #[test]
    fn parse_rejects_bad_input() {
        let ok = "vps = a.example\ndns = ns.example\nmail = mx.example\nbackup = b.example\n";
        assert!(ProtectedHosts::parse(ok).is_ok());
        for bad in [
            "vps = a.example\ndns = ns.example\nmail = mx.example\n",
            &format!("{ok}nope = x.example"),
            &format!("{ok}extra = 192.0.2.7"),
            &format!("{ok}extra = 2001:db8::1"),
            &format!("{ok}extra = bad_name.example"),
            &format!("{ok}just text"),
        ] {
            assert!(ProtectedHosts::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn wildcard_protects_children_not_apex() {
        let p = ProtectedHosts::parse(
            "vps = v.example\ndns = *.ns.example\nmail = m.example\nbackup = b.example",
        )
        .unwrap();
        assert!(p.is_protected("a.ns.example"));
        assert!(!p.is_protected("ns.example"));
        assert!(!p.is_protected("xns.example"));
    }

    #[test]
    fn env_file_replaces_bad_file_keeps_builtin_extra_adds() {
        let d = std::env::temp_dir().join(format!("nerve-ph-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let good = d.join("good.conf");
        std::fs::write(
            &good,
            "vps = v.example\ndns = n.example\nmail = m.example\nbackup = b.example\n",
        )
        .unwrap();
        let gp = good.display().to_string();
        let p = ProtectedHosts::from_env(&|k| match k {
            "NERVE_PLANS_PROTECTED_HOSTS_FILE" => Some(gp.clone()),
            "NERVE_PLANS_PROTECTED_HOSTS" => Some("keep.example, 10.0.0.1".into()),
            _ => None,
        });
        assert_eq!(p.source, gp);
        assert!(p.is_protected("v.example") && p.is_protected("keep.example"));
        assert!(!p.is_protected("ip50.vp3.titanaxe.com"));
        assert!(p.warning.as_deref().unwrap().contains("10.0.0.1"));
        let missing = d.join("missing.conf").display().to_string();
        let p = ProtectedHosts::from_env(&|k| {
            (k == "NERVE_PLANS_PROTECTED_HOSTS_FILE").then(|| missing.clone())
        });
        assert_eq!(p.source, "builtin");
        assert!(p.is_protected("ip50.vp3.titanaxe.com"));
        assert!(p.warning.unwrap().contains("built-in list kept"));
        let _ = std::fs::remove_dir_all(d);
    }
}
