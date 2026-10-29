// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The seven static security checks on a proposal, before anything runs.
//! Every check is recorded (pass or fail) in the pass record; one failure
//! makes the verdict `fail` and the sandbox deploy is never started.
//!
//! 1. `applies_to_base`     the base commit exists and the patch applies
//!    cleanly to it (`git apply --cached --check` on a throwaway index).
//! 2. `allowed_tree`        every path is relative, has no `..`/`.git`, no
//!    control characters, and lies under the allowed tree (default
//!    `crates/`, `docs/`).
//! 3. `protected_targets`   no CI/workflow, deploy script, systemd unit,
//!    sandbox/security crate, secrets or key file; no added line that
//!    reaches for a live path, service control, push or a remote shell.
//! 4. `no_secrets`          no secret-like string (keys, tokens, private key
//!    blocks, key variable assignments) and no public IPv4 literal.
//! 5. `no_new_capabilities` no new network, process-spawn or `unsafe` code
//!    unless the gate operator passed the matching `--allow` flag.
//! 6. `size_limits`         patch bytes, file count, added lines.
//! 7. `text_only`           no binary patch, no executable/symlink/submodule
//!    mode, no NUL, no minified or base64-like blob lines.
//!
//! Nothing here executes the patch; check 1 only runs local git plumbing in
//! the (non-live) source repository.

use crate::proposal::Proposal;
use serde_json::{json, Value};
use std::path::Path;

pub const MAX_PATCH_BYTES: usize = 256 * 1024;
pub const MAX_FILES: usize = 32;
pub const MAX_ADDED_LINES: usize = 2000;
pub const MAX_LINE_CHARS: usize = 400;

pub const NAMES: [&str; 7] = [
    "applies_to_base",
    "allowed_tree",
    "protected_targets",
    "no_secrets",
    "no_new_capabilities",
    "size_limits",
    "text_only",
];

/// Paths Hermes may never change through a proposal: VCS and CI, deploy and
/// unit files, secrets, and the security/enforcement code (so Hermes cannot
/// weaken the gate it has to pass).
pub const DENY_PREFIXES: &[&str] = &[
    ".git/",
    ".github/",
    ".cargo/",
    "deploy/",
    "scripts/",
    "release/",
    "secrets/",
    "crates/hermes-gate/",
    "crates/openmind-sandbox/",
    "crates/hades-gate/",
    "crates/galion-guard/",
    "crates/galion-deploy/",
    "crates/one-defence/",
    "crates/repo-check/",
    "crates/hermes-tools/src/guard.rs",
    "crates/hermes-tools/src/config.rs",
    "crates/hermes-tools/src/exec.rs",
    "deny.toml",
    "REUSE.toml",
    "Cargo.lock",
    "rust-toolchain",
    "build.rs",
];
pub const DENY_SUFFIXES: &[&str] = &[
    ".service",
    ".path",
    ".timer",
    ".socket",
    ".mount",
    ".conf",
    ".env",
    ".pem",
    ".key",
    ".p12",
    ".token",
    "/build.rs",
    "/Cargo.toml",
];

/// Added-line markers that reach for live systems.
pub const DENY_LIVE: &[(&str, &str)] = &[
    ("/opt/galion", "live install root"),
    ("/etc/systemd", "unit directory"),
    ("/var/lib/hermes-rs", "live state dir"),
    ("/var/lib/galion-gate", "gate home"),
    ("systemctl ", "service control"),
    ("git push", "push"),
    ("ssh ", "remote shell"),
    ("scp ", "remote copy"),
    ("HERMES_RS_CODING_ALLOW_NO_LANDLOCK", "sandbox override"),
    ("HERMES_GATE_", "gate override"),
];

/// Added-line markers that look like secret material.
pub const DENY_SECRET: &[(&str, &str)] = &[
    ("-----BEGIN", "key material"),
    ("sk-or-", "OpenRouter key"),
    ("sk-ant-", "API key"),
    ("sk-proj-", "API key"),
    ("ghp_", "GitHub token"),
    ("gho_", "GitHub token"),
    ("ghs_", "GitHub token"),
    ("github_pat_", "GitHub token"),
    ("xoxb-", "Slack token"),
    ("xoxp-", "Slack token"),
    ("AKIA", "AWS key id"),
    ("API_SERVER_KEY=", "gateway key"),
    ("OPENROUTER_API_KEY=", "OpenRouter key"),
    ("RIVER_API_KEY=", "River key"),
    ("TDAI_GATEWAY_API_KEY=", "gateway key"),
    ("Authorization: Bearer ", "bearer token"),
];

/// Capability markers: (flag, marker). A marker in an added line needs
/// `--allow <flag>` from the gate operator (never from the proposal).
pub const CAPS: &[(&str, &str)] = &[
    ("net", "TcpStream"),
    ("net", "TcpListener"),
    ("net", "UdpSocket"),
    ("net", "std::net"),
    ("net", "reqwest"),
    ("net", "ureq"),
    ("net", "hyper"),
    ("net", "to_socket_addrs"),
    ("net", "libc::socket"),
    ("net", "libc::connect"),
    ("spawn", "Command::new"),
    ("spawn", "std::process"),
    ("spawn", "process::Command"),
    ("spawn", "libc::fork"),
    ("spawn", "libc::exec"),
    ("spawn", "posix_spawn"),
    ("spawn", "/bin/sh"),
    ("unsafe", "unsafe "),
    ("unsafe", "unsafe{"),
    ("unsafe", "#[no_mangle]"),
    ("unsafe", "extern \"C\""),
];

/// Who runs the gate decides these, never the proposal.
#[derive(Clone, Debug)]
pub struct CheckPolicy {
    pub allow_tree: Vec<String>,
    /// Subset of `net`, `spawn`, `unsafe`.
    pub allow_caps: Vec<String>,
}

impl Default for CheckPolicy {
    fn default() -> Self {
        CheckPolicy {
            allow_tree: vec!["crates/".into(), "docs/".into()],
            allow_caps: vec![],
        }
    }
}

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

fn path_shape_bad(p: &str) -> Option<String> {
    if p.starts_with('/') || p.starts_with('~') {
        return Some(format!("{p}: absolute path"));
    }
    if p.split('/')
        .any(|c| c == ".." || c == ".git" || c.is_empty())
    {
        return Some(format!("{p}: escapes, empty component or .git"));
    }
    if p.bytes()
        .any(|c| c < 0x20 || c == b'\\' || c == b'"' || c == 0x7f)
    {
        return Some(format!("{p}: control or quoted characters"));
    }
    None
}

fn protected(p: &str) -> Option<String> {
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
    let low = p.to_ascii_lowercase();
    if [
        "secret",
        "credential",
        "password",
        "private_key",
        "id_ed25519",
        "id_rsa",
    ]
    .iter()
    .any(|w| low.contains(w))
    {
        return Some(format!("{p}: secrets-like file name"));
    }
    None
}

/// A public IPv4 literal (loopback, private, link-local, CGNAT,
/// documentation, multicast and 0.0.0.0 are fine). Never echoed.
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

/// A long run of base64/hex-alphabet characters (an embedded blob or key).
fn blob_like(l: &str) -> bool {
    let mut run = 0usize;
    for c in l.chars() {
        if c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' {
            run += 1;
            if run >= 120 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Check 1 on its own: does the patch apply to `base` in `repo`?
pub fn applies_to_base(repo: &Path, p: &Proposal) -> Result<(), String> {
    let base = format!("{}^{{commit}}", p.base_commit);
    crate::git::git(repo, &["rev-parse", "--verify", "--quiet", &base], None)
        .map_err(|_| "base commit not found in the source repository".to_string())?;
    let mut r = [0u8; 6];
    getrandom::getrandom(&mut r).map_err(|e| e.to_string())?;
    let idx = std::env::temp_dir().join(format!("hermes-gate-idx-{}", crate::hex(&r)));
    let idxs = idx.display().to_string();
    let env = [("GIT_INDEX_FILE", idxs.as_str())];
    let res = crate::git::git_env(repo, &["read-tree", &p.base_commit], None, &env)
        .and_then(|_| {
            crate::git::git_env(
                repo,
                &["apply", "--cached", "--check", "-"],
                Some(p.patch.as_bytes()),
                &env,
            )
        })
        .map(|_| ());
    let _ = std::fs::remove_file(&idx);
    res.map_err(|e| format!("patch does not apply cleanly to base: {e}"))
}

pub fn run(p: &Proposal, repo: &Path, pol: &CheckPolicy) -> Vec<Check> {
    let mut out = vec![];
    let mut push = |name, bad: Vec<String>| {
        let mut bad = bad;
        bad.dedup();
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
    let added: Vec<&str> = p.added_lines().collect();

    // 1
    push(
        NAMES[0],
        match applies_to_base(repo, p) {
            Ok(()) => vec![],
            Err(e) => vec![e],
        },
    );
    // 2
    push(
        NAMES[1],
        files
            .iter()
            .filter_map(|f| {
                path_shape_bad(f).or_else(|| {
                    (!pol.allow_tree.iter().any(|t| f.starts_with(t.as_str())))
                        .then(|| format!("{f}: outside the allowed tree {:?}", pol.allow_tree))
                })
            })
            .collect(),
    );
    // 3
    let mut prot: Vec<String> = files.iter().filter_map(|f| protected(f)).collect();
    for l in &added {
        for (m, why) in DENY_LIVE {
            if l.contains(m) {
                prot.push(format!("added line has {why}"));
            }
        }
    }
    push(NAMES[2], prot);
    // 4
    let mut sec = vec![];
    for l in &added {
        for (m, why) in DENY_SECRET {
            if l.contains(m) {
                sec.push(format!("added line has {why}"));
            }
        }
        if public_ipv4(l).is_some() {
            sec.push("added line has a public IPv4 literal".to_string());
        }
    }
    push(NAMES[3], sec);
    // 5
    let mut caps = vec![];
    for l in &added {
        for (flag, m) in CAPS {
            if l.contains(m) && !pol.allow_caps.iter().any(|a| a == flag) {
                caps.push(format!(
                    "new {flag} capability ({m}) without --allow {flag}"
                ));
            }
        }
    }
    push(NAMES[4], caps);
    // 6
    push(
        NAMES[5],
        [
            (p.patch.len() > MAX_PATCH_BYTES)
                .then(|| format!("patch {} B > {MAX_PATCH_BYTES} B", p.patch.len())),
            (files.len() > MAX_FILES).then(|| format!("{} files > {MAX_FILES}", files.len())),
            (added.len() > MAX_ADDED_LINES)
                .then(|| format!("{} added lines > {MAX_ADDED_LINES}", added.len())),
            files
                .is_empty()
                .then(|| "patch touches no file".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
    // 7
    let mut bin = vec![];
    if p.patch.contains("GIT binary patch") || p.patch.contains("Binary files ") {
        bin.push("binary patch".to_string());
    }
    if p.patch.contains('\0') {
        bin.push("NUL byte".to_string());
    }
    for l in p.patch.lines() {
        if (l.starts_with("new mode ")
            || l.starts_with("new file mode ")
            || l.starts_with("old mode "))
            && (l.ends_with("755") || l.ends_with("120000") || l.ends_with("160000"))
        {
            bin.push(format!("{l} (executable, symlink or submodule)"));
        }
    }
    for l in &added {
        if l.chars().count() > MAX_LINE_CHARS {
            bin.push(format!("added line longer than {MAX_LINE_CHARS} chars"));
        } else if blob_like(l) {
            bin.push("added line looks like an encoded blob".to_string());
        }
    }
    push(NAMES[6], bin);
    out
}

pub fn all_ok(c: &[Check]) -> bool {
    c.len() == NAMES.len() && c.iter().all(|x| x.ok)
}
