// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gate config: a flat TOML subset, one `key = value` per line.
//!
//! ```toml
//! admin_pubkey = "/etc/galion/hades/admin.pub"   # required
//! ledger       = "/var/lib/galion/hades/ledger.jsonl"  # required
//! disable_file = "/etc/galion/hades/DISABLED"    # required; present = HADES off
//! max_ttl_secs = 900                             # optional, capped at 3600
//! ```
//!
//! Unknown keys are an error, so a typo cannot silently drop a setting.

use std::path::{Path, PathBuf};

/// Default token lifetime ceiling.
pub const DEFAULT_MAX_TTL_SECS: u64 = 900;

/// Parsed config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Admin Ed25519 public key file.
    pub admin_pubkey: PathBuf,
    /// Replay ledger file.
    pub ledger: PathBuf,
    /// If this file exists, HADES refuses everything.
    pub disable_file: PathBuf,
    /// Longest token lifetime accepted.
    pub max_ttl_secs: u64,
}

impl Config {
    /// Parse config text.
    ///
    /// # Errors
    /// Unknown key, bad value, or a missing required key.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (mut pubkey, mut ledger, mut disable, mut ttl) = (None, None, None, None);
        for (i, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: expected key = value", i + 1))?;
            let (key, value) = (key.trim(), value.trim());
            match key {
                "admin_pubkey" => pubkey = Some(PathBuf::from(string(value, i)?)),
                "ledger" => ledger = Some(PathBuf::from(string(value, i)?)),
                "disable_file" => disable = Some(PathBuf::from(string(value, i)?)),
                "max_ttl_secs" => {
                    ttl = Some(
                        value
                            .parse::<u64>()
                            .ok()
                            .filter(|t| *t > 0)
                            .ok_or_else(|| format!("line {}: max_ttl_secs", i + 1))?,
                    );
                }
                other => return Err(format!("line {}: unknown key {other}", i + 1)),
            }
        }
        Ok(Self {
            admin_pubkey: pubkey.ok_or("missing admin_pubkey")?,
            ledger: ledger.ok_or("missing ledger")?,
            disable_file: disable.ok_or("missing disable_file")?,
            max_ttl_secs: ttl
                .unwrap_or(DEFAULT_MAX_TTL_SECS)
                .min(crate::token::HARD_MAX_TTL_SECS),
        })
    }

    /// Read and parse a config file.
    ///
    /// # Errors
    /// I/O or parse errors.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {}", path.display(), e.kind()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn strip_comment(line: &str) -> &str {
    // Only strip `#` outside a quoted string.
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn string(value: &str, i: usize) -> Result<String, String> {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .filter(|v| !v.is_empty() && !v.contains('"') && !v.contains('\\'))
        .map(str::to_owned)
        .ok_or_else(|| format!("line {}: expected a quoted path", i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_caps_ttl() {
        let c = Config::parse(
            "# hades\nadmin_pubkey = \"/k/admin.pub\"\nledger = \"/l.jsonl\" # x\ndisable_file = \"/d\"\nmax_ttl_secs = 99999\n",
        )
        .unwrap();
        assert_eq!(c.admin_pubkey, PathBuf::from("/k/admin.pub"));
        assert_eq!(c.max_ttl_secs, crate::token::HARD_MAX_TTL_SECS);
    }

    #[test]
    fn rejects_unknown_and_missing() {
        assert!(Config::parse("admin_pubky = \"/k\"").is_err());
        assert!(Config::parse("admin_pubkey = \"/k\"\nledger = \"/l\"").is_err());
        assert!(
            Config::parse("admin_pubkey = /k\nledger = \"/l\"\ndisable_file = \"/d\"").is_err()
        );
    }
}
