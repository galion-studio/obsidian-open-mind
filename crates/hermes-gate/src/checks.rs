// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Static security checks on a proposal, before anything is applied.
//! Every check is recorded (pass or fail) in the pass record; one failure
//! makes the verdict `fail`. Nothing here runs the patch.

use crate::proposal::Proposal;
use serde_json::{json, Value};

pub const MAX_PATCH_BYTES: usize = 512 * 1024;
pub const MAX_FILES: usize = 64;

/// Paths Hermes may never change through a proposal: VCS and CI, deploy and
/// unit files, secrets, and the gate's own enforcement code (so Hermes
/// cannot weaken the gate it has to pass).
pub const DENY_PREFIXES: &[&str] = &[
    ".git/",
    ".github/",
    ".cargo/",
    "deploy/",
    "secrets/",
    "crates/hermes-gate/",
    "crates/openmind-sandbox/",
    "crates/hermes-tools/src/guard.rs",
    "crates/hermes-tools/src/config.rs",
    "crates/hermes-tools/src/exec.rs",
    "deny.toml",
    "REUSE.toml",
    "rust-toolchain",
    "build.rs",
];
pub const DENY_SUFFIXES: &[&str] = &[
    ".service",
    ".path",
    ".timer",
    ".socket",
    ".env",
    ".pem",
    ".key",
    "/build.rs",
];

/// Added-line markers that mean "this change reaches for live or secrets".
pub const DENY_ADDED: &[(&str, &str)] = &[
    ("/opt/galion", "live install root"),
    ("/etc/systemd", "unit directory"),
    ("systemctl ", "service control"),
    ("git push", "push"),
    ("ssh ", "remote shell"),
    ("scp ", "remote copy"),
    ("-----BEGIN", "key material"),
    ("sk-or-", "OpenRouter key"),
    ("ghp_", "GitHub token"),
    ("github_pat_", "GitHub token"),
    ("API_SERVER_KEY=", "gateway key"),
    ("OPENROUTER_API_KEY=", "OpenRouter key"),
    ("RIVER_API_KEY=", "River key"),
    ("HERMES_RS_CODING_ALLOW_NO_LANDLOCK", "sandbox override"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

impl Check {
    pub fn to_json(&self) -> Value {
        json!({"name": self.name, "ok": self.ok, "detail": self.detail})
    }
}

fn path_bad(p: &str) -> Option<String> {
    if p.starts_with('/') || p.starts_with('~') {
        return Some(format!("{p}: absolute path"));
    }
    if p.split('/').any(|c| c == ".." || c == ".git") {
        return Some(format!("{p}: escapes or touches .git"));
    }
    if p.bytes().any(|c| c < 0x20 || c == b'\\' || c == b'"') {
        return Some(format!("{p}: control or quoted characters"));
    }
    if let Some(d) = DENY_PREFIXES
        .iter()
        .find(|d| p == d.trim_end_matches('/') || p.starts_with(*d))
    {
        return Some(format!("{p}: protected path ({d})"));
    }
    if let Some(d) = DENY_SUFFIXES
        .iter()
        .find(|d| p.ends_with(*d) || p == d.trim_start_matches('/'))
    {
        return Some(format!("{p}: protected file type ({d})"));
    }
    None
}

/// A public IPv4 literal in added lines (loopback, private, link-local,
/// documentation and 0.0.0.0 are fine). Host addresses do not belong in code.
pub fn public_ipv4(line: &str) -> Option<String> {
    for tok in line.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        let parts: Vec<&str> = tok.split('.').collect();
        if parts.len() != 4 {
            continue;
        }
        let Ok(o) = parts
            .iter()
            .map(|p| {
                if p.is_empty() || p.len() > 3 {
                    Err(())
                } else {
                    p.parse::<u8>().map_err(|_| ())
                }
            })
            .collect::<Result<Vec<u8>, ()>>()
        else {
            continue;
        };
        let private = o[0] == 0
            || o[0] == 10
            || o[0] == 127
            || (o[0] == 169 && o[1] == 254)
            || (o[0] == 172 && (16..=31).contains(&o[1]))
            || (o[0] == 192 && o[1] == 168)
            || (o[0] == 192 && o[1] == 0 && o[2] == 2)
            || (o[0] == 198 && o[1] == 51 && o[2] == 100)
            || (o[0] == 203 && o[1] == 0 && o[2] == 113)
            || o[0] >= 224
            || (o[0] == 100 && (64..=127).contains(&o[1]));
        if !private {
            return Some(tok.to_string());
        }
    }
    None
}

pub fn run(p: &Proposal) -> Vec<Check> {
    let mut out = vec![];
    let mut push = |name, bad: Vec<String>| {
        out.push(Check {
            name,
            ok: bad.is_empty(),
            detail: if bad.is_empty() {
                "ok".into()
            } else {
                bad.join("; ")
            },
        })
    };
    let files = p.files();
    push(
        "size",
        [
            (p.patch.len() > MAX_PATCH_BYTES)
                .then(|| format!("patch {} B > {MAX_PATCH_BYTES} B", p.patch.len())),
            (files.len() > MAX_FILES).then(|| format!("{} files > {MAX_FILES}", files.len())),
            files
                .is_empty()
                .then(|| "patch touches no file".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
    push(
        "text_only",
        if p.patch.contains("GIT binary patch") || p.patch.contains("Binary files ") {
            vec!["binary patch".into()]
        } else {
            vec![]
        },
    );
    push(
        "mode_changes",
        p.patch
            .lines()
            .filter(|l| {
                (l.starts_with("new mode ") || l.starts_with("new file mode "))
                    && (l.ends_with("755") || l.ends_with("120000") || l.ends_with("160000"))
            })
            .map(|l| format!("{l} (executable, symlink or submodule)"))
            .collect(),
    );
    push("paths", files.iter().filter_map(|f| path_bad(f)).collect());
    let mut live = vec![];
    let mut ips = vec![];
    let mut unsafe_lines = 0;
    for l in p.added_lines() {
        for (m, why) in DENY_ADDED {
            if l.contains(m) {
                live.push(format!("added line has {why}"));
            }
        }
        if let Some(ip) = public_ipv4(l) {
            let _ = ip; // never echo the address itself into the record
            ips.push("added line has a public IPv4 literal".to_string());
        }
        let t = l.trim_start();
        if t.starts_with("unsafe ") || t.contains(" unsafe {") || t.contains("unsafe fn ") {
            unsafe_lines += 1;
        }
    }
    live.dedup();
    ips.dedup();
    push("no_live_or_secrets", live);
    push("no_host_addresses", ips);
    push(
        "no_new_unsafe",
        if unsafe_lines > 0 {
            vec![format!("{unsafe_lines} added line(s) with unsafe")]
        } else {
            vec![]
        },
    );
    out
}

pub fn all_ok(c: &[Check]) -> bool {
    !c.is_empty() && c.iter().all(|x| x.ok)
}
