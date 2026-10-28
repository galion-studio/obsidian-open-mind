// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! repo-check: the repo self-evaluation lint.
//!
//! Turns the TEACHER/CHECKS.md one-liners (C6 Zagreb time, C7 CJK, C8 no
//! Python) plus the repo rules (MIT LICENSE, SPDX headers, no secrets, no
//! hard-coded hosts, universal CPU) into one std-only, read-only scanner.
//!
//! Every check works on `(relative path, text)` so it is unit-testable without
//! touching the disk. A line containing the marker `repo-check: allow` is
//! skipped by every check except `secrets` and `license`.
//!
//! Patterns that would match this file itself are assembled with `concat!`, so
//! the tool can scan its own source without allow markers.

use std::fmt;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

/// Inline marker that silences non-secret findings on one line.
pub const ALLOW_MARKER: &str = concat!("repo-check", ": allow");

/// Files larger than this are skipped (binaries, dumps).
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Directories never descended into.
pub const SKIP_DIRS: &[&str] = &[".git", "target", "contrib", "stage", "node_modules"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Warn,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Level::Warn => "warn",
            Level::Error => "error",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    License,
    Spdx,
    Cjk,
    Python,
    Utc,
    Secrets,
    Hosts,
    Portable,
    Deps,
    Openmind,
}

impl Check {
    pub const ALL: [Check; 10] = [
        Check::License,
        Check::Spdx,
        Check::Cjk,
        Check::Python,
        Check::Utc,
        Check::Secrets,
        Check::Hosts,
        Check::Portable,
        Check::Deps,
        Check::Openmind,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Check::License => "license",
            Check::Spdx => "spdx",
            Check::Cjk => "cjk",
            Check::Python => "python",
            Check::Utc => "utc",
            Check::Secrets => "secrets",
            Check::Hosts => "hosts",
            Check::Portable => "portable",
            Check::Deps => "deps",
            Check::Openmind => "openmind",
        }
    }

    pub fn parse(s: &str) -> Option<Check> {
        Check::ALL.into_iter().find(|c| c.name() == s)
    }

    /// Default severity. `utc` is a review list (CHECKS.md C6 allows calls
    /// that feed zagreb-time), so it only warns.
    pub fn default_level(self) -> Level {
        match self {
            Check::Utc => Level::Warn,
            _ => Level::Error,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Check::License => "root LICENSE is MIT with a Copyright line; crate LICENSE copies match it",
            Check::Spdx => "every .rs/.sh/Cargo.toml under crates/ and deploy/ has an SPDX MIT line in its first 5 lines",
            Check::Cjk => "no Han/Hiragana/Katakana/Hangul characters (CHECKS C7)",
            Check::Python => "no .py/.pyc files and no python shebangs (CHECKS C8)",
            Check::Utc => "UTC clock reads or Z-suffixed timestamps in crates/ outside zagreb-time (CHECKS C6, review)",
            Check::Secrets => "no private-key blocks or token-shaped strings (matches are never printed)",
            Check::Hosts => "no public IPv4 literals in crates/, deploy/, .github/",
            Check::Portable => "no inline asm, arch intrinsics, target-cpu/target-feature, openssl-sys (UNIVERSAL-CPU.md)",
            Check::Deps => "serde_json features are pinned once in the root [workspace.dependencies] (preserve_order); member crates use { workspace = true } with no features",
            Check::Openmind => "no cd / git -C / GIT_DIR into the live VPS tree /opt/galion/openmind (D-391-2): git write subcommands are errors, cd and read-only git are warnings",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub check: Check,
    pub level: Level,
    pub file: String,
    /// 1-based; 0 means "whole file".
    pub line: usize,
    pub msg: String,
}

fn finding(check: Check, file: &str, line: usize, msg: impl Into<String>) -> Finding {
    Finding {
        check,
        level: check.default_level(),
        file: file.to_string(),
        line,
        msg: msg.into(),
    }
}

fn ext(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
}

fn file_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|e| e.to_str())
        .unwrap_or("")
}

fn under(path: &str, dirs: &[&str]) -> bool {
    dirs.iter()
        .any(|d| path.starts_with(d) && path[d.len()..].starts_with('/'))
}

// ---------------------------------------------------------------- license

/// Root LICENSE: MIT header plus a `Copyright (c) ` line. `expect` optionally
/// pins the holder text (for example "2026 Slavik / Galion Studio").
pub fn check_license(text: Option<&str>, expect: Option<&str>) -> Vec<Finding> {
    let c = Check::License;
    let Some(text) = text else {
        return vec![finding(c, "LICENSE", 0, "root LICENSE file missing")];
    };
    let mut out = Vec::new();
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.trim() != "MIT License" {
        out.push(finding(
            c,
            "LICENSE",
            1,
            "first line is not \"MIT License\"",
        ));
    }
    let copyright = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim_start().starts_with("Copyright (c) "));
    match (copyright, expect) {
        (None, _) => out.push(finding(c, "LICENSE", 0, "no \"Copyright (c) \" line")),
        (Some((i, l)), Some(want)) if !l.contains(want) => out.push(finding(
            c,
            "LICENSE",
            i + 1,
            format!("copyright line does not contain {want:?}"),
        )),
        _ => {}
    }
    if !text.contains("Permission is hereby granted, free of charge") {
        out.push(finding(
            c,
            "LICENSE",
            0,
            "MIT permission grant text missing",
        ));
    }
    out
}

/// A crate-level LICENSE must be a byte-for-byte copy of the root one.
pub fn check_crate_license(path: &str, text: &str, root: Option<&str>) -> Vec<Finding> {
    match root {
        Some(r) if r != text => vec![finding(
            Check::License,
            path,
            0,
            "differs from root LICENSE",
        )],
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------- per file

/// Run every line/file check over one text file. `path` is repo-relative with
/// `/` separators.
pub fn scan_text(path: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    spdx(path, text, &mut out);
    python_shebang(path, text, &mut out);
    let in_code = under(path, &["crates"]);
    let rs = ext(path) == "rs";
    let utc_scope = rs && in_code && !under(path, &["crates/zagreb-time"]);
    let hosts_scope =
        under(path, &["crates", "deploy", ".github"]) && file_name(path) != "Cargo.lock";
    let portable_rs = rs;
    let portable_cfg = ext(path) == "toml" || under(path, &[".cargo"]) || under(path, &[".github"]);

    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        secrets_line(path, n, line, &mut out);
        if line.contains(ALLOW_MARKER) {
            continue;
        }
        if let Some(ch) = line.chars().find(|&ch| is_cjk(ch)) {
            out.push(finding(
                Check::Cjk,
                path,
                n,
                format!("CJK character U+{:04X}", ch as u32),
            ));
        }
        if utc_scope {
            utc_line(path, n, line, &mut out);
        }
        if hosts_scope {
            for ip in public_ipv4s(line) {
                out.push(finding(
                    Check::Hosts,
                    path,
                    n,
                    format!("hard-coded public IPv4 {ip}; read it from config/env"),
                ));
            }
        }
        if portable_rs {
            for pat in PORTABLE_RS {
                if line.contains(pat) {
                    out.push(finding(
                        Check::Portable,
                        path,
                        n,
                        format!("arch-specific Rust `{pat}`; use portable code or cfg-gate with a fallback and mark it"),
                    ));
                    break;
                }
            }
        }
        if portable_cfg {
            for pat in PORTABLE_CFG {
                if line.contains(pat) {
                    out.push(finding(
                        Check::Portable,
                        path,
                        n,
                        format!("`{pat}` pins a CPU; binaries must run on any consumer CPU"),
                    ));
                }
            }
        }
    }
    if file_name(path) == "Cargo.lock" {
        for bad in NON_PORTABLE_CRATES {
            let needle = format!("name = \"{bad}\"");
            if let Some(i) = text.lines().position(|l| l.trim() == needle) {
                out.push(finding(
                    Check::Portable,
                    path,
                    i + 1,
                    format!("{bad} needs a C toolchain per target; use rustls or no TLS"),
                ));
            }
        }
    }
    if file_name(path) == "Cargo.toml" {
        dep_pins(path, text, &mut out);
    }
    openmind_writes(path, text, &mut out);
    out
}

/// Path-only checks (files whose content is not text).
pub fn scan_path(path: &str) -> Vec<Finding> {
    match ext(path) {
        "py" | "pyc" | "pyw" => vec![finding(
            Check::Python,
            path,
            0,
            "Python file; port it to Rust or move it out of the repo",
        )],
        _ => Vec::new(),
    }
}

fn spdx(path: &str, text: &str, out: &mut Vec<Finding>) {
    let scoped = under(path, &["crates", "deploy"])
        && (matches!(ext(path), "rs" | "sh") || file_name(path) == "Cargo.toml");
    if !scoped {
        return;
    }
    let tag = concat!("SPDX-License-", "Identifier: MIT");
    if !text.lines().take(5).any(|l| l.contains(tag)) {
        out.push(finding(
            Check::Spdx,
            path,
            1,
            "missing SPDX MIT line in the first 5 lines",
        ));
    }
}

fn python_shebang(path: &str, text: &str, out: &mut Vec<Finding>) {
    if let Some(first) = text.lines().next() {
        if first.starts_with("#!") && first.contains(concat!("pyth", "on")) {
            out.push(finding(Check::Python, path, 1, "python shebang"));
        }
    }
}

// ---------------------------------------------------------------- cjk

pub fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF      // Hangul Jamo
        | 0x3040..=0x309F    // Hiragana
        | 0x30A0..=0x30FF    // Katakana
        | 0x3130..=0x318F    // Hangul compatibility Jamo
        | 0x31F0..=0x31FF    // Katakana phonetic extensions
        | 0x3400..=0x4DBF    // Han extension A
        | 0x4E00..=0x9FFF    // Han unified
        | 0xA960..=0xA97F    // Hangul Jamo extended A
        | 0xAC00..=0xD7AF    // Hangul syllables
        | 0xD7B0..=0xD7FF    // Hangul Jamo extended B
        | 0xF900..=0xFAFF    // Han compatibility
        | 0xFF66..=0xFF9F    // half-width Katakana
        | 0x20000..=0x2FA1F  // Han extensions B..F + compat supplement
        | 0x30000..=0x323AF  // Han extensions G, H
    )
}

// ---------------------------------------------------------------- utc

const UTC_CALLS: &[&str] = &[concat!("Utc::", "now()"), concat!("SystemTime::", "now()")];

fn utc_line(path: &str, n: usize, line: &str, out: &mut Vec<Finding>) {
    for pat in UTC_CALLS {
        if line.contains(pat) {
            out.push(finding(
                Check::Utc,
                path,
                n,
                format!("`{pat}` outside zagreb-time; fine only if it feeds zagreb-time"),
            ));
        }
    }
    if has_z_timestamp(line) {
        out.push(finding(
            Check::Utc,
            path,
            n,
            "RFC 3339 timestamp ending in Z; output must carry the Zagreb offset",
        ));
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.frac]Z` anywhere in the line.
pub fn has_z_timestamp(line: &str) -> bool {
    let b = line.as_bytes();
    const SHAPE: &[u8] = b"dddd-dd-ddTdd:dd:dd";
    if b.len() < SHAPE.len() + 1 {
        return false;
    }
    for start in 0..=b.len() - SHAPE.len() {
        let ok = SHAPE.iter().enumerate().all(|(k, &s)| {
            let c = b[start + k];
            if s == b'd' {
                c.is_ascii_digit()
            } else {
                c == s
            }
        });
        if !ok {
            continue;
        }
        let mut j = start + SHAPE.len();
        if j < b.len() && b[j] == b'.' {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        if j < b.len() && b[j] == b'Z' {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------- secrets

/// (prefix, minimum token length after the prefix, extra allowed chars, label)
const TOKEN_SHAPES: &[(&str, usize, &str, &str)] = &[
    (concat!("gh", "p_"), 36, "", "GitHub token"),
    (concat!("gh", "o_"), 36, "", "GitHub OAuth token"),
    (concat!("gh", "s_"), 36, "", "GitHub app token"),
    (concat!("gh", "u_"), 36, "", "GitHub user token"),
    (concat!("gh", "r_"), 36, "", "GitHub refresh token"),
    (
        concat!("github", "_pat_"),
        22,
        "_",
        "GitHub fine-grained token",
    ),
    (concat!("s", "k-"), 20, "_-", "API secret key"),
    (concat!("xo", "xb-"), 10, "-", "Slack bot token"),
    (concat!("xo", "xp-"), 10, "-", "Slack user token"),
    (concat!("AK", "IA"), 16, "", "AWS access key id"),
    (concat!("AI", "za"), 35, "_-", "Google API key"),
    (concat!("gl", "pat-"), 20, "_-", "GitLab token"),
];

fn secrets_line(path: &str, n: usize, line: &str, out: &mut Vec<Finding>) {
    let c = Check::Secrets;
    if line.contains(concat!("-----BEGIN ", "")) && line.contains(concat!("PRIVATE ", "KEY-----")) {
        out.push(finding(c, path, n, "private key block"));
    }
    for (prefix, min, extra, label) in TOKEN_SHAPES {
        if token_shaped(line, prefix, *min, extra) {
            out.push(finding(
                c,
                path,
                n,
                format!("looks like a {label} (value not shown); move it to the secret store"),
            ));
        }
    }
}

/// True when `prefix` appears at a word boundary followed by at least `min`
/// characters from `[A-Za-z0-9]` plus `extra`.
pub fn token_shaped(line: &str, prefix: &str, min: usize, extra: &str) -> bool {
    let mut from = 0;
    while let Some(off) = line[from..].find(prefix) {
        let at = from + off;
        let boundary = line[..at]
            .chars()
            .next_back()
            .is_none_or(|p| !(p.is_ascii_alphanumeric() || p == '_' || p == '-'));
        let tail = line[at + prefix.len()..]
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || extra.contains(*ch))
            .count();
        if boundary && tail >= min {
            return true;
        }
        from = at + prefix.len();
    }
    false
}

// ---------------------------------------------------------------- hosts

/// Public (routable) IPv4 literals in a line. Loopback, private, link-local,
/// unspecified, broadcast, documentation and CGNAT/shared ranges are allowed.
pub fn public_ipv4s(line: &str) -> Vec<Ipv4Addr> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit()
            || (i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'.'))
        {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
            j += 1;
        }
        let mut cand = &line[i..j];
        // A sentence-ending dot is not part of the address.
        if let Some(stripped) = cand.strip_suffix('.') {
            cand = stripped;
        }
        let next_alpha = j < b.len() && (b[j].is_ascii_alphabetic() || b[j] == b'_');
        if !next_alpha && cand.split('.').count() == 4 {
            if let Ok(ip) = cand.parse::<Ipv4Addr>() {
                if is_public(ip) {
                    out.push(ip);
                }
            }
        }
        i = j.max(i + 1);
    }
    out
}

fn is_public(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let shared = o[0] == 100 && (64..128).contains(&o[1]);
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || shared
        || o[0] == 0)
}

// ---------------------------------------------------------------- portable

const PORTABLE_RS: &[&str] = &[
    concat!("as", "m!("),
    concat!("global_as", "m!("),
    concat!("core::", "arch::"),
    concat!("std::", "arch::"),
    concat!("#[target_", "feature"),
    concat!("is_x86_feature_", "detected!"),
    concat!("is_aarch64_feature_", "detected!"),
];

const PORTABLE_CFG: &[&str] = &[concat!("target-", "cpu"), concat!("target-", "feature")];

const NON_PORTABLE_CRATES: &[&str] = &["openssl-sys"];

// ---------------------------------------------------------------- walking

/// Every regular file under `root` (relative, `/`-separated, sorted), skipping
/// [`SKIP_DIRS`] and symlinks.
pub fn walk(root: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for entry in std::fs::read_dir(root.join(&rel))? {
            let entry = entry?;
            let ft = entry.file_type()?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let child = rel.join(name);
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name) {
                    stack.push(child);
                }
            } else if ft.is_file() {
                out.push(
                    child
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Read a file as text; `None` for binaries (NUL byte, invalid UTF-8) and
/// files over [`MAX_FILE_BYTES`].
pub fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub struct Report {
    pub files_scanned: usize,
    pub findings: Vec<Finding>,
}

/// Scan the whole tree under `root`.
pub fn scan_tree(root: &Path, expect_copyright: Option<&str>) -> std::io::Result<Report> {
    let files = walk(root)?;
    let root_license = read_text(&root.join("LICENSE"));
    let mut findings = check_license(root_license.as_deref(), expect_copyright);
    let mut files_scanned = 0;
    for rel in &files {
        findings.extend(scan_path(rel));
        let Some(text) = read_text(&root.join(rel)) else {
            continue;
        };
        files_scanned += 1;
        // Only our own crate copies must equal the root LICENSE (same scope as the
        // CI license job). Vendored third-party licences, e.g. under stack/, keep
        // their own copyright line.
        let ours = rel.starts_with("crates/") || rel.starts_with("edge/");
        if ours && rel != "LICENSE" && file_name(rel) == "LICENSE" {
            findings.extend(check_crate_license(rel, &text, root_license.as_deref()));
        }
        findings.extend(scan_text(rel, &text));
    }
    Ok(Report {
        files_scanned,
        findings,
    })
}

// ---------------------------------------------------------------- deps

/// The dependency whose features must be decided once for the workspace.
pub const PINNED_DEP: &str = "serde_json";
/// The feature the root pins for it.
pub const PINNED_FEATURE: &str = "preserve_order";

/// `serde_json = ...` or `serde_json.x = ...` (not `serde-json`, not a
/// feature list that merely mentions `dep:serde_json`).
fn is_pinned_dep_key(line: &str) -> bool {
    let t = line.trim_start();
    t.strip_prefix(PINNED_DEP)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c == ' ' || c == '=' || c == '.' || c == '\t')
}

/// Cargo unifies features over the packages in one build, so a member crate
/// that adds `features = ["preserve_order"]` changes the JSON key order of
/// every other crate in a `--workspace` build but not in a `-p` build. The
/// feature set is therefore pinned once in the root `[workspace.dependencies]`
/// and member crates may only say `{ workspace = true }`.
/// Scope: the root `Cargo.toml` and `crates/*/Cargo.toml`.
pub fn dep_pins(path: &str, text: &str, out: &mut Vec<Finding>) {
    let root = path == "Cargo.toml";
    let member = path.starts_with("crates/") && path.matches('/').count() == 2;
    if !root && !member {
        return;
    }
    let mut section = String::new();
    let mut root_pin_seen = false;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let t = line.trim();
        if line.contains(ALLOW_MARKER) || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            section = t.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            let tail = format!(".{PINNED_DEP}");
            if member && section.ends_with(&tail) {
                out.push(finding(
                    Check::Deps,
                    path,
                    n,
                    format!("[{section}] table; write `{PINNED_DEP} = {{ workspace = true }}` (features are pinned in the root Cargo.toml)"),
                ));
            }
            continue;
        }
        if !is_pinned_dep_key(line) {
            continue;
        }
        let code = t.split('#').next().unwrap_or("");
        if root && section == "workspace.dependencies" {
            root_pin_seen = true;
            if !code.contains(PINNED_FEATURE) {
                out.push(finding(
                    Check::Deps,
                    path,
                    n,
                    format!("root [workspace.dependencies] {PINNED_DEP} must enable `{PINNED_FEATURE}` so `-p` and `--workspace` builds emit the same JSON key order"),
                ));
            }
        } else if member {
            let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
            let uses_ws = compact.contains("workspace=true");
            if compact.contains("features") || !uses_ws {
                out.push(finding(
                    Check::Deps,
                    path,
                    n,
                    format!("{PINNED_DEP} in a member crate must be `{{ workspace = true }}` with no features; its feature set is pinned in the root Cargo.toml"),
                ));
            }
        }
    }
    if root && text.contains("[workspace.dependencies]") && !root_pin_seen {
        out.push(finding(
            Check::Deps,
            path,
            0,
            format!(
                "root [workspace.dependencies] has no {PINNED_DEP} entry with `{PINNED_FEATURE}`"
            ),
        ));
    }
}

// ---------------------------------------------------------------- json

pub fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

// ---------------------------------------------------------------- openmind

/// The live VPS working copy (hard rule 3, D-391-2). Built with `concat!` so
/// this file does not flag itself.
pub const LIVE_TREE: &str = concat!("/opt/galion/", "openmind");

/// git subcommands that only read (still warned: `status` refreshes the
/// index unless `GIT_OPTIONAL_LOCKS=0`, and the tree should not be anyone's
/// working directory). Anything not listed here counts as a write.
pub const GIT_READ_ONLY: &[&str] = &[
    "status",
    "log",
    "show",
    "diff",
    "rev-parse",
    "ls-files",
    "ls-tree",
    "cat-file",
    "describe",
    "blame",
    "grep",
    "for-each-ref",
    "rev-list",
    "shortlog",
    "show-ref",
    "name-rev",
    "merge-base",
    "reflog",
    "count-objects",
    "fsck",
    "verify-commit",
    "var",
    "help",
    "version",
];

/// Read-only use of a subcommand that can also write, judged by its first
/// argument (`git remote -v`, `git branch --show-current`, `git config --get`,
/// `git stash list`, `git worktree list`).
pub fn git_read_only(sub: &str, rest: &[&str]) -> bool {
    if GIT_READ_ONLY.contains(&sub) {
        return true;
    }
    let first = rest.first().copied().unwrap_or("");
    match sub {
        "remote" => matches!(first, "" | "-v" | "--verbose" | "show" | "get-url"),
        "branch" => matches!(
            first,
            "" | "-a"
                | "-r"
                | "-v"
                | "-vv"
                | "--list"
                | "--show-current"
                | "--contains"
                | "--merged"
        ),
        "tag" => matches!(first, "" | "-l" | "--list" | "--contains"),
        "config" => matches!(
            first,
            "--get" | "--get-all" | "--get-regexp" | "-l" | "--list"
        ),
        "stash" | "worktree" | "notes" => first == "list",
        _ => false,
    }
}

/// `p` is the live tree or below it (component-wise; `openmind-old` is not).
fn in_live_tree(p: &str) -> bool {
    p.strip_prefix(LIVE_TREE)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Strip shell/markdown punctuation around a token.
fn bare(tok: &str) -> &str {
    let junk = |c: char| "\"'`()${};&|<>[],*".contains(c) || c == '\\';
    tok.trim_matches(junk)
        .trim_end_matches(['.', ':'])
        .trim_matches(junk)
}

fn openmind_finding(path: &str, n: usize, level: Level, msg: String, out: &mut Vec<Finding>) {
    out.push(Finding {
        check: Check::Openmind,
        level,
        file: path.to_string(),
        line: n,
        msg,
    });
}

/// Flag scripts, units, code and docs that point git (or a shell) at the
/// live tree. Errors: a git write subcommand aimed at it (`git -C`,
/// `--git-dir`, `--work-tree`, `git clone ... <tree>`), `GIT_DIR=` /
/// `GIT_WORK_TREE=` set to it, or `cd <tree> && git <write>` on one line.
/// Warnings: `cd` / `pushd` / `WorkingDirectory=` / `.current_dir(` into it
/// and read-only git there. Use a separate clone (galion-report-push).
pub fn openmind_writes(path: &str, text: &str, out: &mut Vec<Finding>) {
    if !text.contains(LIVE_TREE) {
        return;
    }
    // Shell scripts: a `cd <tree>` stays in force for later lines until the
    // next cd/popd (the 2026-10-27 overnight push pattern).
    let shell = ext(path) == "sh";
    let mut sticky = false;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.contains(ALLOW_MARKER) {
            continue;
        }
        if !line.contains(LIVE_TREE) && !(sticky && line.contains("git")) {
            if shell
                && line
                    .split_whitespace()
                    .any(|t| matches!(bare(t), "cd" | "pushd" | "popd"))
            {
                sticky = false;
            }
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().map(bare).collect();
        let mut cd_into = sticky;
        let mut flagged = false;
        let mut k = 0;
        while k < toks.len() {
            let t = toks[k];
            // cd / pushd into the tree.
            if t == "cd" || t == "pushd" || t == "popd" {
                cd_into = t != "popd" && toks.get(k + 1).is_some_and(|v| in_live_tree(v));
                if shell {
                    sticky = cd_into;
                }
                k += if t == "popd" { 1 } else { 2 };
                continue;
            }
            // GIT_DIR=/GIT_WORK_TREE= redirects.
            for var in ["GIT_DIR=", "GIT_WORK_TREE=", "GIT_COMMON_DIR="] {
                if let Some(v) = t.strip_prefix(var) {
                    if in_live_tree(bare(v)) {
                        openmind_finding(
                            path,
                            n,
                            Level::Error,
                            format!("{var} points git at the live tree; use your own clone"),
                            out,
                        );
                        flagged = true;
                    }
                }
            }
            if t == "git" || t.ends_with("/git") {
                let mut j = k + 1;
                let mut into = false;
                while j < toks.len() {
                    let o = toks[j];
                    let (opt, inline) = match o.split_once('=') {
                        Some((a, b)) if a.starts_with("--") => (a, Some(b)),
                        _ => (o, None),
                    };
                    match opt {
                        "-C" | "--git-dir" | "--work-tree" => {
                            let v = match inline {
                                Some(v) => v,
                                None => {
                                    j += 1;
                                    toks.get(j).copied().unwrap_or("")
                                }
                            };
                            if in_live_tree(bare(v)) {
                                into = true;
                            }
                        }
                        "-c" => j += 1,
                        _ if o.starts_with('-') => {}
                        _ => break,
                    }
                    j += 1;
                }
                let sub = toks.get(j).copied().unwrap_or("");
                let clone_into = sub == "clone" && toks[j + 1..].iter().any(|v| in_live_tree(v));
                let target = into || cd_into;
                if clone_into || (target && !sub.is_empty()) {
                    let rest = &toks[(j + 1).min(toks.len())..];
                    let write = clone_into || !git_read_only(sub, rest);
                    let how = if clone_into {
                        "git clone into the live tree"
                    } else if into {
                        "git -C/--git-dir/--work-tree into the live tree"
                    } else {
                        "cd into the live tree, then git"
                    };
                    if write {
                        openmind_finding(
                            path,
                            n,
                            Level::Error,
                            format!(
                                "{how} with write subcommand `{sub}` (D-391-2); use your own clone"
                            ),
                            out,
                        );
                    } else {
                        openmind_finding(
                            path,
                            n,
                            Level::Warn,
                            format!("{how}, read-only `{sub}`; set GIT_OPTIONAL_LOCKS=0"),
                            out,
                        );
                    }
                    flagged = true;
                }
                k = j + 1;
                continue;
            }
            k += 1;
        }
        let workdir = line
            .split_whitespace()
            .filter_map(|t| t.strip_prefix("WorkingDirectory="))
            .any(|v| in_live_tree(bare(v)));
        let rust_cd = line.contains(".current_dir(") && {
            let lit = format!("\"{LIVE_TREE}");
            line.match_indices(&lit)
                .any(|(at, _)| in_live_tree(line[at + 1..].split('"').next().unwrap_or("")))
        };
        let cd_here = line.contains(LIVE_TREE) && cd_into;
        if !flagged && (cd_here || workdir || rust_cd) {
            openmind_finding(
                path,
                n,
                Level::Warn,
                "working directory set to the live tree; no git writes there (D-391-2)".into(),
                out,
            );
        }
    }
}
