// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The Hermes deny list (hardened 2026-10-29).
//!
//! Every tool call of every agent (the boss, delegated children, cron, MCP
//! tools, coding tools) passes [`check_tool_call`] through the guarded
//! wrapper that [`crate::tools::Registry::add`] puts around each tool. A
//! refusal never reaches the tool.
//!
//! Layers (docs/hermes-denylist.md has the full table):
//! * P  — the prior list (55 + 2 rules) and the 12-rule floor, verbatim, run
//!   on the raw text, the prior normalisation, and the de-quoted command.
//! * S1 — chaining, subshells, substitution, here-docs, compound commands.
//! * S2 — quoting, unicode look-alikes, globs in command names and paths.
//! * S3 — encoded payloads: decoders, escape printers, embedded base64/hex.
//! * S4 — interpreters, wrappers, running files by path, find/sed/tar/git/cargo.
//! * S5 — environment tricks: assignments, expansions, env dumps, tool env.
//! * S6 — paths: canonical resolution (`..`, `~`, symlinks), secrets, write roots.
//! * S7 — network clients, loopback / private hosts, non-http URL schemes.
//! * S8 — services, signals, packages, git push and other remote/system ops.
//! * S9 — tool level: code tools, shell tools without an inspectable command,
//!   delegated writes, vault writes, MCP tools, size limits. Fail closed.

pub mod ere;
pub mod rules;
pub mod shell;

use rules::*;
use serde_json::Value;
use shell::{RedirKind, Segment, Word};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// A refusal: the rule id and a plain reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Deny {
    pub rule: String,
    pub reason: String,
}

impl std::fmt::Display for Deny {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.rule, self.reason)
    }
}

fn deny(rule: &str, why: impl Into<String>) -> Deny {
    Deny {
        rule: rule.to_string(),
        reason: why.into(),
    }
}

type R = Result<(), Deny>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Hardened deny list (default).
    Denylist,
    /// Default-deny: only [`RESEARCH_ALLOW`] command words may run.
    Allowlist,
}

/// Where the guard thinks the agent works and what it may write.
#[derive(Debug, Clone)]
pub struct Policy {
    pub mode: Mode,
    /// Directory relative paths and commands resolve against.
    pub cwd: PathBuf,
    /// The only roots writes may land in (empty = no writes at all).
    pub write_roots: Vec<PathBuf>,
    pub home: PathBuf,
    /// Hosts `curl`/`wget` may reach (empty = no network clients).
    pub net_allow: Vec<String>,
    /// Extra read-only vault roots on top of [`VAULT_ROOTS`].
    pub extra_vaults: Vec<PathBuf>,
}

/// Tool-call size bounds (over = refused, never partially checked).
pub const MAX_STRING_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_STRINGS: usize = 4096;
pub const MAX_DEPTH: usize = 32;

fn under(p: &Path, root: &Path) -> bool {
    p == root || p.starts_with(root)
}

impl Policy {
    /// A policy with one writable workspace (tests, coding worktrees).
    pub fn workspace(dir: impl Into<PathBuf>) -> Policy {
        let d = dir.into();
        let d = std::fs::canonicalize(&d).unwrap_or(d);
        let mut p = Policy {
            mode: Mode::Denylist,
            cwd: d.clone(),
            write_roots: vec![d],
            home: PathBuf::from("/root"),
            net_allow: Vec::new(),
            extra_vaults: Vec::new(),
        };
        p.sanitise();
        p
    }

    /// This policy with `dir` as the cwd and the only writable root (the
    /// coding tools' worktree). Mode, home, network and vault settings stay.
    pub fn with_workspace(&self, dir: &Path) -> Policy {
        let w = Policy::workspace(dir);
        let mut p = self.clone();
        p.cwd = w.cwd;
        p.write_roots = w.write_roots;
        p.sanitise();
        p
    }

    /// From the environment:
    /// `HERMES_RS_GUARD_WORKSPACE` or else `HERMES_RS_CODING_WORKTREE` = the
    /// writable root and cwd (none set = nothing writable, cwd `/nonexistent`);
    /// `HERMES_RS_GUARD_MODE=allowlist` = default-deny terminal;
    /// `HERMES_RS_GUARD_NET_ALLOW=host,host`; `HERMES_RS_GUARD_VAULTS=/a:/b`.
    pub fn from_env() -> Policy {
        let ws = std::env::var("HERMES_RS_GUARD_WORKSPACE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("HERMES_RS_CODING_WORKTREE").ok())
            .filter(|s| !s.trim().is_empty());
        let mut p = match ws {
            Some(w) => Policy::workspace(w.trim()),
            None => Policy {
                mode: Mode::Denylist,
                cwd: PathBuf::from("/nonexistent-hermes-workspace"),
                write_roots: Vec::new(),
                home: PathBuf::from("/root"),
                net_allow: Vec::new(),
                extra_vaults: Vec::new(),
            },
        };
        if let Ok(h) = std::env::var("HOME") {
            if h.starts_with('/') {
                p.home = PathBuf::from(h);
            }
        }
        if std::env::var("HERMES_RS_GUARD_MODE")
            .map(|m| m.trim().eq_ignore_ascii_case("allowlist"))
            .unwrap_or(false)
        {
            p.mode = Mode::Allowlist;
        }
        if let Ok(n) = std::env::var("HERMES_RS_GUARD_NET_ALLOW") {
            p.net_allow = n
                .split(',')
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("HERMES_RS_GUARD_VAULTS") {
            p.extra_vaults = v
                .split(':')
                .filter(|s| s.starts_with('/'))
                .map(PathBuf::from)
                .collect();
        }
        p.sanitise();
        p
    }

    /// The process-wide policy (read once).
    pub fn global() -> Arc<Policy> {
        static G: OnceLock<Arc<Policy>> = OnceLock::new();
        G.get_or_init(|| Arc::new(Policy::from_env())).clone()
    }

    /// Drop write roots that are `/`, secret, a vault, or contain one.
    fn sanitise(&mut self) {
        let vaults = self.vaults();
        self.write_roots.retain(|w| {
            let bad_root = w.components().count() < 3; // "/" and "/x"
            let secret = SECRET_ROOTS.iter().any(|s| {
                let s = Path::new(s);
                under(w, s) || under(s, w)
            });
            let vault = vaults.iter().any(|v| under(w, v) || under(v, w));
            !(bad_root || secret || vault)
        });
    }

    pub fn vaults(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = VAULT_ROOTS.iter().map(PathBuf::from).collect();
        v.extend(self.extra_vaults.iter().cloned());
        v
    }

    fn in_write_roots(&self, p: &Path) -> bool {
        self.write_roots.iter().any(|w| under(p, w))
    }
}

// ------------------------------------------------------------------ paths

/// `~`, `..`, `.`, `//` resolved lexically against `cwd`.
fn lexical(raw: &str, cwd: &Path, home: &Path) -> Result<PathBuf, Deny> {
    if raw.contains('\0') {
        return Err(deny("S6-nul", "NUL byte in a path"));
    }
    let joined: PathBuf = if raw == "~" {
        home.to_path_buf()
    } else if let Some(r) = raw.strip_prefix("~/") {
        home.join(r)
    } else if raw.starts_with('~') {
        return Err(deny("S6-tilde-user", "~user paths point into other homes"));
    } else if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        cwd.join(raw)
    };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(n) => out.push(n),
        }
    }
    Ok(out)
}

/// Resolve symlinks on the longest existing prefix, keep the rest lexical.
fn physical(p: &Path) -> PathBuf {
    let mut existing = p.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = std::fs::canonicalize(&existing) {
            let mut out = c;
            for r in rest.iter().rev() {
                out.push(r);
            }
            return out;
        }
        match (
            existing.file_name().map(|f| f.to_os_string()),
            existing.parent(),
        ) {
            (Some(f), Some(par)) => {
                rest.push(f);
                existing = par.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

fn is_alias(p: &Path) -> bool {
    let s = p.to_string_lossy();
    if ALIAS_PREFIXES.iter().any(|a| under(p, Path::new(a))) {
        return true;
    }
    // /proc/<pid>/{root,cwd,fd,environ,mem,map_files,exe}
    let parts: Vec<&str> = s.split('/').filter(|x| !x.is_empty()).collect();
    parts.len() >= 3
        && parts[0] == "proc"
        && (parts[1].chars().all(|c| c.is_ascii_digit())
            || parts[1] == "self"
            || parts[1] == "thread-self")
        && matches!(
            parts[2],
            "root" | "cwd" | "fd" | "environ" | "mem" | "map_files" | "exe" | "task"
        )
}

fn secret_reason(p: &Path) -> Option<String> {
    for r in SECRET_ROOTS {
        if under(p, Path::new(r)) {
            return Some(format!("{r} holds secrets"));
        }
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let lname = name.to_ascii_lowercase();
    if SECRET_NAMES.iter().any(|n| lname == *n) || lname.starts_with(".env.") {
        return Some(format!("{name} is a secret file name"));
    }
    if SECRET_SUFFIXES.iter().any(|s| lname.ends_with(s)) {
        return Some(format!("{name} looks like key material"));
    }
    // the prior floor's secret words, now on the canonical path too
    let full = p.to_string_lossy().to_ascii_lowercase();
    for w in [
        "galion-secrets",
        "galion-keys",
        "github.token",
        "galion-app.token",
        "hermes.key",
        "keys.db",
    ] {
        if full.contains(w) {
            return Some(format!("{w} is a secret store"));
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
}

/// Check one path for a read or a write. `strict` adds the prior floor's
/// `/etc/` and `/boot/` refusal on the canonical path (shell and file tools).
pub fn check_path(raw: &str, access: Access, cwd: &Path, pol: &Policy) -> R {
    check_path_x(raw, access, cwd, pol, true)
}

fn check_path_x(raw: &str, access: Access, cwd: &Path, pol: &Policy, strict: bool) -> R {
    if matches!(
        raw,
        "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/stdin" | "-"
    ) {
        return Ok(());
    }
    if let Some(rest) = raw.strip_prefix("file://") {
        return check_path_x(rest, access, cwd, pol, strict);
    }
    let lex = lexical(raw, cwd, &pol.home)?;
    let phys = physical(&lex);
    for p in [&lex, &phys] {
        if is_alias(p) {
            return Err(deny(
                "S6-alias",
                format!("{} is an alias of / or of a process view", p.display()),
            ));
        }
        if let Some(why) = secret_reason(p) {
            return Err(deny("S6-secret", format!("{}: {why}", p.display())));
        }
        if strict && (under(p, Path::new("/etc")) || under(p, Path::new("/boot"))) {
            return Err(deny(
                "S6-etc-boot",
                format!(
                    "{} is under /etc or /boot (prior floor rule, now on the resolved path)",
                    p.display()
                ),
            ));
        }
        if p.to_string_lossy().contains("docker.sock") {
            return Err(deny("S6-etc-boot", "docker.sock"));
        }
    }
    if access == Access::Write {
        for p in [&lex, &phys] {
            if let Some(v) = pol.vaults().iter().find(|v| under(p, v)) {
                return Err(deny(
                    "S9-vault-write",
                    format!(
                        "{} is inside the read-only vault {}",
                        p.display(),
                        v.display()
                    ),
                ));
            }
            if !pol.in_write_roots(p) {
                let root = PROTECTED_WRITE_ROOTS
                    .iter()
                    .find(|r| under(p, Path::new(r)))
                    .copied()
                    .unwrap_or("/");
                return Err(deny(
                    "S6-write-root",
                    format!(
                        "{} is outside the writable workspace (protected: {root})",
                        p.display()
                    ),
                ));
            }
            for c in p.components() {
                if let Component::Normal(n) = c {
                    let n = n.to_string_lossy();
                    if NO_WRITE_DIRS.iter().any(|d| *d == n) {
                        return Err(deny(
                            "S6-no-write",
                            format!("{} is inside a {n} directory", p.display()),
                        ));
                    }
                }
            }
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if NO_WRITE_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                return Err(deny(
                    "S6-no-write",
                    format!("{name} steers the agent or runs code on its own"),
                ));
            }
        }
    }
    Ok(())
}

// --------------------------------------------------------------- the prior

fn py_normalize(text: &str) -> String {
    let t = text.replace('\n', " ").replace("''", "");
    let c: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\\'
            && i > 0
            && c[i - 1].is_ascii_alphabetic()
            && c.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic())
        {
            i += 1;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Prior floor (all 12, or only the all-tools ones) and, when `list`, the
/// prior dangerous-patterns list.
fn prior_check(text: &str, floor_all: bool, list: bool) -> R {
    let p = rules::prior();
    let folded = py_normalize(text);
    for r in &p.floor {
        if !floor_all && !FLOOR_ALL_TOOLS.contains(&r.id.as_str()) {
            continue;
        }
        if r.re.is_match(text) || r.re.is_match(&folded) {
            return Err(deny(&r.id, format!("prior floor rule /{}/", r.re.source)));
        }
    }
    if list {
        for r in &p.list {
            if r.re.is_match(text) || r.re.is_match(&folded) {
                return Err(deny(
                    &r.id,
                    format!("prior deny-list rule /{}/", r.re.source),
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- decoding

fn b64val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

fn b64decode(s: &str) -> Option<Vec<u8>> {
    let t: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    if t.len() < 8 {
        return None;
    }
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for b in t {
        acc = (acc << 6) | u32::from(b64val(b)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn hexdecode(s: &str) -> Option<Vec<u8>> {
    let t: String = s.chars().filter(|c| !matches!(c, ':' | ' ')).collect();
    if t.len() < 8 || !t.len().is_multiple_of(2) || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&t[i..i + 2], 16).ok())
        .collect()
}

/// `\xNN`, `\NNN`, `\uNNNN` escapes as printf / echo -e would expand them.
fn unescape(s: &str) -> Option<String> {
    if !s.contains('\\') {
        return None;
    }
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut any = false;
    while i < c.len() {
        if c[i] == '\\' && i + 1 < c.len() {
            let n = c[i + 1];
            let (radix, max, skip) = match n {
                'x' => (16, 2, 2),
                'u' => (16, 4, 2),
                'U' => (16, 8, 2),
                '0'..='7' => (8, 3, 1),
                _ => (0, 0, 0),
            };
            if radix > 0 {
                let start = i + skip;
                let mut j = start;
                while j < c.len() && j - start < max && c[j].is_digit(radix) {
                    j += 1;
                }
                if j > start {
                    let v: String = c[start..j].iter().collect();
                    if let Some(ch) = u32::from_str_radix(&v, radix).ok().and_then(char::from_u32) {
                        out.push(ch);
                        any = true;
                        i = j;
                        continue;
                    }
                }
            }
        }
        out.push(c[i]);
        i += 1;
    }
    any.then_some(out)
}

/// Decoded forms worth re-checking: printable text from base64 / hex tokens
/// and from backslash escapes.
fn decoded_variants(word: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(u) = unescape(word) {
        out.push(u);
    }
    for tok in word.split(|c: char| !(c.is_ascii_alphanumeric() || "+/=-_:".contains(c))) {
        for bytes in [b64decode(tok), hexdecode(tok)].into_iter().flatten() {
            if let Ok(s) = String::from_utf8(bytes) {
                let printable = s.chars().filter(|c| !c.is_control() || *c == '\n').count();
                if s.len() >= 4 && printable * 10 >= s.chars().count() * 9 {
                    out.push(s);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- commands

struct Cmd<'a> {
    pol: &'a Policy,
    cwd: PathBuf,
    depth: usize,
}

fn is_assignment(w: &Word) -> Option<(&str, &str)> {
    let (n, v) = w.text.split_once('=')?;
    (!n.is_empty()
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !n.starts_with(|c: char| c.is_ascii_digit()))
    .then_some((n, v))
}

fn interp_base(name: &str) -> bool {
    if INTERPRETERS.contains(&name) {
        return true;
    }
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-');
    [
        "python", "pypy", "perl", "ruby", "node", "php", "lua", "tclsh", "pip", "java", "gcc",
        "clang", "bash", "zsh",
    ]
    .contains(&stem)
}

const BIN_DIRS: &[&str] = &[
    "/bin",
    "/usr/bin",
    "/sbin",
    "/usr/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
];

fn looks_like_path(t: &str) -> bool {
    !t.is_empty()
        && !t.contains("://")
        && (t.starts_with('/')
            || t.starts_with('~')
            || t.starts_with("./")
            || t.starts_with("../")
            || t == "."
            || t == ".."
            || t.contains('/'))
}

fn check_host(host: &str, pol: &Policy, from_shell: bool) -> R {
    let h = host
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    let h = h.trim_end_matches('.');
    let private = h.is_empty()
        || h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".internal")
        || h.ends_with(".local")
        || h.starts_with("127.")
        || h.starts_with("0.")
        || h == "0"
        || h.starts_with("10.")
        || h.starts_with("192.168.")
        || h.starts_with("169.254.")
        || h == "::1"
        || h == "::"
        || h.starts_with("fc")
        || h.starts_with("fd")
        || h.starts_with("fe80")
        || h.starts_with("::ffff:")
        || h.starts_with("0x")
        || h.chars().all(|c| c.is_ascii_digit())
        || {
            let o: Vec<u32> = h.split('.').filter_map(|x| x.parse().ok()).collect();
            o.len() == 4
                && ((o[0] == 172 && (16..32).contains(&o[1]))
                    || (o[0] == 100 && (64..128).contains(&o[1])))
        };
    if private {
        return Err(deny("S7-loopback", format!("{h} is a loopback, private or metadata address (local services and keys live there)")));
    }
    if from_shell
        && !pol
            .net_allow
            .iter()
            .any(|a| h == a || h.ends_with(&format!(".{a}")))
    {
        return Err(deny(
            "S7-network",
            format!("network client to {h}: not on the allow list"),
        ));
    }
    Ok(())
}

/// Check a URL given to any tool.
pub fn check_url(url: &str, pol: &Policy, from_shell: bool) -> R {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    let Some((scheme, rest)) = lower.split_once("://") else {
        if lower.starts_with("data:")
            || lower.starts_with("javascript:")
            || lower.starts_with("file:")
        {
            return Err(deny("S7-scheme", "only http(s) URLs are allowed"));
        }
        return Ok(());
    };
    if scheme == "file" {
        check_path_x(&u[7..], Access::Read, &pol.cwd, pol, true)?;
        return Err(deny("S7-scheme", "file:// URLs read local files"));
    }
    if scheme != "http" && scheme != "https" {
        return Err(deny("S7-scheme", format!("{scheme}:// is not http(s)")));
    }
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = auth.rsplit('@').next().unwrap_or("");
    let host = if hostport.starts_with('[') {
        hostport
            .split(']')
            .next()
            .unwrap_or("")
            .trim_start_matches('[')
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    check_host(host, pol, from_shell)
}

impl<'a> Cmd<'a> {
    fn path(&self, raw: &str, a: Access) -> R {
        check_path_x(raw, a, &self.cwd, self.pol, true)
    }

    fn word_path(&self, w: &Word, a: Access) -> R {
        if w.glob {
            let t = &w.text;
            if t.starts_with('/') || w.tilde || t.contains("..") || a == Access::Write {
                return Err(deny(
                    "S2-glob-path",
                    format!("glob in a path that can not be resolved safely: {t}"),
                ));
            }
            // relative read glob: check the fixed directory part
            let fixed: String = t.split(['*', '?', '[']).next().unwrap_or("").to_string();
            let dir = fixed
                .rsplit_once('/')
                .map(|x| x.0.to_string())
                .unwrap_or_default();
            return self.path(if dir.is_empty() { "." } else { &dir }, a);
        }
        self.path(&w.text, a)
    }

    fn all(&mut self, src: &str) -> R {
        if src.len() > MAX_COMMAND_BYTES {
            return Err(deny("S9-size", "command too long to check"));
        }
        if let Some(c) = src.chars().find(|c| shell::suspicious_char(*c)) {
            return Err(deny(
                "S2-unicode",
                format!("invisible or look-alike character U+{:04X}", c as u32),
            ));
        }
        prior_check(src, true, true)?;
        let segs = shell::lex(src)?;
        // P again on the de-quoted, basename-normalised reconstruction
        let rebuilt: Vec<String> = segs
            .iter()
            .map(|s| {
                let mut v: Vec<String> = s.words.iter().map(|w| w.text.clone()).collect();
                if let Some(i) = s.words.iter().position(|w| is_assignment(w).is_none()) {
                    v[i] = v[i].rsplit('/').next().unwrap_or("").to_string();
                }
                for r in &s.redirs {
                    v.push(if r.kind == RedirKind::Write {
                        ">".into()
                    } else {
                        "<".into()
                    });
                    v.push(r.target.text.clone());
                }
                v.join(" ")
            })
            .collect();
        prior_check(&rebuilt.join(" ; "), true, true)?;
        prior_check(&rebuilt.join("\n"), true, true)?;
        // S3: decode hidden payloads and check them as commands too
        if self.depth < 2 {
            for s in &segs {
                for w in s.words.iter().chain(s.redirs.iter().map(|r| &r.target)) {
                    for d in decoded_variants(&w.text) {
                        let mut sub = Cmd {
                            pol: self.pol,
                            cwd: self.cwd.clone(),
                            depth: self.depth + 1,
                        };
                        if let Err(e) = sub.all(&d) {
                            return Err(deny(
                                "S3-encoded",
                                format!("decoded payload is refused ({e})"),
                            ));
                        }
                    }
                }
            }
        }
        let n = segs.len();
        for (i, s) in segs.iter().enumerate() {
            self.segment(s, i + 1 < n)?;
        }
        Ok(())
    }

    fn segment(&mut self, s: &Segment, _more: bool) -> R {
        for r in &s.redirs {
            let a = if r.kind == RedirKind::Write {
                Access::Write
            } else {
                Access::Read
            };
            self.word_path(&r.target, a)?;
        }
        let mut words = s.words.iter().peekable();
        while let Some(w) = words.peek() {
            let Some((name, val)) = is_assignment(w) else {
                break;
            };
            if !ENV_ALLOW.contains(&name) {
                return Err(deny(
                    "S5-env-assign",
                    format!("{name}= changes how programs load or find code"),
                ));
            }
            if looks_like_path(val) {
                self.path(val, Access::Read)?;
            }
            words.next();
        }
        let rest: Vec<&Word> = words.collect();
        let Some(cmd) = rest.first() else {
            return Ok(());
        };
        let args = &rest[1..];
        if cmd.glob {
            return Err(deny(
                "S2-glob-cmd",
                format!("glob in the command name: {}", cmd.text),
            ));
        }
        let mut name = cmd.text.clone();
        if name.contains('/') {
            let lex = lexical(&name, &self.cwd, &self.pol.home)?;
            let parent = lex
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            if BIN_DIRS.contains(&parent.as_str()) {
                name = lex
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
            } else {
                return Err(deny(
                    "S4-path-exec",
                    format!(
                        "running a file by path ({}) bypasses the command checks",
                        cmd.text
                    ),
                ));
            }
        }
        let lname = name.to_ascii_lowercase();
        let n = lname.as_str();
        if matches!(
            n,
            "if" | "then"
                | "else"
                | "elif"
                | "fi"
                | "for"
                | "while"
                | "until"
                | "do"
                | "done"
                | "case"
                | "esac"
                | "select"
                | "function"
                | "{"
                | "}"
                | "[["
                | "]]"
                | "!"
                | "coproc"
                | "in"
        ) || n.ends_with("()")
            || args
                .first()
                .is_some_and(|a| a.text == "()" || a.text.starts_with("()"))
        {
            return Err(deny(
                "S1-compound",
                format!("shell compound command or function definition ({n})"),
            ));
        }
        if self.pol.mode == Mode::Allowlist && !RESEARCH_ALLOW.contains(&n) {
            return Err(deny(
                "S0-allowlist",
                format!("{n} is not on the terminal allow list"),
            ));
        }
        if n == "env" && args.is_empty() || ENV_DUMPERS.contains(&n) && n != "env" {
            return Err(deny(
                "S5-env-dump",
                format!("{n} prints the environment (provider keys live there)"),
            ));
        }
        if interp_base(n) {
            return Err(deny(
                "S4-interpreter",
                format!("{n} runs code the deny list can not see"),
            ));
        }
        if WRAPPERS.contains(&n) {
            return Err(deny(
                "S4-wrapper",
                format!("{n} runs another command past the checks"),
            ));
        }
        if DECODERS.contains(&n) {
            return Err(deny("S3-decoder", format!("{n} decodes hidden payloads")));
        }
        if SYSTEM.contains(&n) || n.starts_with("mkfs") || n.starts_with("systemd-") {
            return Err(deny(
                "S8-system",
                format!("{n} changes services, processes or the system"),
            ));
        }
        if NETWORK.contains(&n) {
            if (n == "curl" || n == "wget") && !self.pol.net_allow.is_empty() {
                return self.net_client(n, args);
            }
            return Err(deny("S7-network", format!("{n} talks to the network")));
        }
        match n {
            "printf" => {
                if args
                    .iter()
                    .any(|a| a.text.contains('\\') || a.text.contains("%b"))
                {
                    return Err(deny("S3-escape", "printf with escapes builds hidden text"));
                }
            }
            "echo" => {
                if args
                    .iter()
                    .take_while(|a| a.text.starts_with('-'))
                    .any(|a| a.text.contains('e'))
                {
                    return Err(deny("S3-escape", "echo -e expands escapes"));
                }
            }
            "find" => {
                for a in args {
                    let t = a.text.as_str();
                    if matches!(
                        t,
                        "-exec"
                            | "-execdir"
                            | "-ok"
                            | "-okdir"
                            | "-delete"
                            | "-fprint"
                            | "-fprint0"
                            | "-fprintf"
                            | "-fls"
                    ) {
                        return Err(deny("S4-find-exec", format!("find {t} runs or writes")));
                    }
                }
            }
            "sed" => self.sed(args)?,
            "rg" | "sort" => {
                for a in args {
                    let t = a.text.as_str();
                    if t == "--pre"
                        || t.starts_with("--pre=")
                        || t.starts_with("--compress-program")
                    {
                        return Err(deny(
                            "S4-runs-program",
                            format!("{n} {t} runs another program"),
                        ));
                    }
                }
            }
            "jq" | "gojq" | "yq" => {
                if args.iter().any(|a| {
                    let t = a.text.as_str();
                    t.contains("$ENV")
                        || t.contains("input_filename")
                        || t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                            .any(|w| w == "env")
                }) {
                    return Err(deny(
                        "S5-env-dump",
                        format!("{n} env / $ENV prints the environment"),
                    ));
                }
            }
            "date" => {
                if args.iter().any(|a| {
                    a.text.starts_with("--set")
                        || (a.text.len() > 1
                            && a.text.starts_with('-')
                            && a.text[1..].chars().all(|c| "uRs".contains(c))
                            && a.text.contains('s'))
                }) {
                    return Err(deny("S8-system", "date -s sets the system clock"));
                }
            }
            "hostname" | "hostnamectl" => {
                if !args.is_empty() {
                    return Err(deny(
                        "S8-system",
                        "hostname with arguments changes the host name",
                    ));
                }
            }
            "uniq" => {
                let ops: Vec<&&Word> = args.iter().filter(|a| !a.text.starts_with('-')).collect();
                if let Some(out) = ops.get(1) {
                    self.word_path(out, Access::Write)?;
                }
            }
            "tar" | "zip" | "unzip" => self.archive(n, args)?,
            "git" => self.git(args)?,
            "cargo" => self.cargo(args)?,
            "cd" => {
                let target = args.iter().find(|a| !a.text.starts_with('-'));
                let Some(t) = target else {
                    return Err(deny("S6-cd", "cd with no directory goes to $HOME"));
                };
                if t.glob {
                    return Err(deny("S2-glob-path", "glob in cd"));
                }
                let lex = lexical(&t.text, &self.cwd, &self.pol.home)?;
                let phys = physical(&lex);
                if !self.pol.in_write_roots(&lex) || !self.pol.in_write_roots(&phys) {
                    return Err(deny(
                        "S6-cd",
                        format!("cd to {} leaves the workspace", lex.display()),
                    ));
                }
                self.cwd = phys;
                return Ok(());
            }
            _ => {}
        }
        // S6: path arguments
        let ops: Vec<&&Word> = args
            .iter()
            .filter(|a| !a.text.starts_with('-') || a.text == "-")
            .collect();
        let opt_values = || {
            args.iter().filter_map(|a| {
                a.text
                    .strip_prefix("--")
                    .and_then(|t| t.split_once('='))
                    .map(|(_, v)| v.to_string())
            })
        };
        let writes_all = WRITE_ALL_ARGS.contains(&n);
        let writes_last = WRITE_LAST_ARG.contains(&n);
        if n == "dd" {
            for a in args {
                if let Some(v) = a.text.strip_prefix("of=") {
                    self.path(v, Access::Write)?;
                } else if let Some(v) = a.text.strip_prefix("if=") {
                    self.path(v, Access::Read)?;
                }
            }
            return Ok(());
        }
        for (i, a) in ops.iter().enumerate() {
            let skip_first = matches!(n, "chmod" | "chown" | "chgrp") && i == 0;
            if skip_first {
                continue;
            }
            let write = writes_all || (writes_last && i + 1 == ops.len() && ops.len() > 1);
            if write {
                self.word_path(a, Access::Write)?;
                if n == "rm" || n == "rmdir" || n == "mv" {
                    let lex = lexical(&a.text, &self.cwd, &self.pol.home)?;
                    if self.pol.write_roots.iter().any(|w| under(w, &lex)) {
                        return Err(deny(
                            "S6-rm-root",
                            format!("{n} on the workspace root or above it"),
                        ));
                    }
                }
            } else if looks_like_path(&a.text) || a.glob {
                self.word_path(a, Access::Read)?;
            } else if !a.text.is_empty()
                && !a.text.contains("://")
                && (a.text.contains('.') || a.text.contains('_'))
            {
                // bare names are relative paths for most tools: secret names
                let p = lexical(&a.text, &self.cwd, &self.pol.home)?;
                if let Some(why) = secret_reason(&p) {
                    return Err(deny("S6-secret", why));
                }
            }
        }
        // values of --opt=value and -t DIR
        for v in opt_values() {
            if looks_like_path(&v) {
                self.path(
                    &v,
                    if writes_all || writes_last {
                        Access::Write
                    } else {
                        Access::Read
                    },
                )?;
            }
        }
        for pair in args.windows(2) {
            if (writes_last && (pair[0].text == "-t" || pair[0].text == "--target-directory"))
                || pair[0].text == "-o"
                || pair[0].text == "--output"
            {
                self.path(&pair[1].text, Access::Write)?;
            }
        }
        Ok(())
    }

    fn net_client(&self, n: &str, args: &[&Word]) -> R {
        for a in args {
            let t = a.text.as_str();
            if t.starts_with('@')
                || t.contains("=@")
                || t.starts_with("-T")
                || t == "--upload-file"
                || t.starts_with("-d")
                || t.starts_with("--data")
                || t.starts_with("-F")
                || t.starts_with("--form")
                || t == "--post-file"
                || t.starts_with("--post-file=")
                || t == "-K"
                || t == "--config"
                || t.starts_with("-o")
                || t.starts_with("-O")
                || t == "--output"
            {
                return Err(deny(
                    "S7-upload",
                    format!("{n} {t} uploads, posts or writes files"),
                ));
            }
            if t.contains("://") {
                check_url(t, self.pol, true)?;
            } else if !t.starts_with('-') {
                check_url(&format!("https://{t}"), self.pol, true)?;
            }
        }
        Ok(())
    }

    fn sed(&self, args: &[&Word]) -> R {
        let mut script: Option<String> = None;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let t = a.text.as_str();
            if t == "-i"
                || t.starts_with("--in-place")
                || (t.starts_with('-') && !t.starts_with("--") && t.contains('i'))
            {
                return Err(deny("S4-sed", "sed -i edits files in place"));
            }
            if t == "-f" || t.starts_with("--file") {
                return Err(deny("S4-sed", "sed -f runs a script file"));
            }
            if t == "-e" || t == "--expression" {
                script = it.next().map(|w| w.text.clone());
            } else if !t.starts_with('-') && script.is_none() {
                script = Some(t.to_string());
            } else if !t.starts_with('-') {
                check_path_x(t, Access::Read, &self.cwd, self.pol, true)?;
            }
        }
        let s = script.unwrap_or_default();
        let ok = |p: &str| -> bool {
            let f = Flags::default();
            ere::Regex::new(p, f)
                .map(|r| r.is_match(&s))
                .unwrap_or(false)
        };
        use ere::Flags;
        if s.is_empty()
            || ok(r"^([0-9]+|\$)?(,([0-9]+|\$))?[pd]$")
            || ok(r"^/[^/;]*/[pd]$")
            || ok(r"^[0-9]+q$")
            || ok(r"^s/[^/;]*/[^/;]*/[gpI0-9]*$")
            || ok(r"^s\|[^|;]*\|[^|;]*\|[gpI0-9]*$")
        {
            return Ok(());
        }
        Err(deny(
            "S4-sed",
            "sed script with e/w/r/W commands or an unknown shape",
        ))
    }

    fn archive(&self, n: &str, args: &[&Word]) -> R {
        for a in args {
            let t = a.text.as_str();
            if t.starts_with("--to-command")
                || t.starts_with("--checkpoint-action")
                || t.starts_with("--use-compress-program")
                || t == "-I"
                || t.starts_with("--rsh-command")
                || t.starts_with("--info-script")
                || t.starts_with("--new-volume-script")
                || t == "-F"
                || t == "-T"
                || t == "-TT"
                || t.starts_with("--unzip-command")
            {
                return Err(deny("S4-archive-exec", format!("{n} {t} runs a program")));
            }
        }
        let first = args.first().map(|a| a.text.as_str()).unwrap_or("");
        let extracting = n == "unzip"
            || (n == "tar"
                && (args
                    .iter()
                    .any(|a| a.text == "--extract" || a.text == "--get")
                    || (!first.starts_with("--") && first.contains('x'))));
        if extracting {
            let mut dir = ".".to_string();
            for pair in args.windows(2) {
                if pair[0].text == "-C" || pair[0].text == "-d" || pair[0].text == "--directory" {
                    dir = pair[1].text.clone();
                }
            }
            self.path(&dir, Access::Write)?;
        }
        let creating = n == "zip"
            || (n == "tar"
                && !first.starts_with("--")
                && (first.contains('c') || first.contains('r') || first.contains('u')));
        for pair in args.windows(2) {
            let f = pair[0].text.as_str();
            if f == "-f"
                || f == "--file"
                || (n == "tar" && !f.starts_with("--") && f.starts_with('-') && f.ends_with('f'))
                || (n == "tar" && !f.starts_with('-') && f == first && f.ends_with('f'))
            {
                self.path(
                    &pair[1].text,
                    if creating {
                        Access::Write
                    } else {
                        Access::Read
                    },
                )?;
            }
        }
        if n == "zip" {
            if let Some(out) = args.iter().find(|a| !a.text.starts_with('-')) {
                self.path(&out.text, Access::Write)?;
            }
        }
        for a in args.iter().skip(1) {
            if looks_like_path(&a.text) || a.glob {
                self.word_path(a, Access::Read)?;
            }
        }
        Ok(())
    }

    fn git(&self, args: &[&Word]) -> R {
        let mut i = 0;
        let mut cwd = self.cwd.clone();
        while i < args.len() && args[i].text.starts_with('-') {
            let t = args[i].text.as_str();
            match t {
                "-C" => {
                    let d = args.get(i + 1).map(|w| w.text.as_str()).unwrap_or("");
                    let lex = lexical(d, &self.cwd, &self.pol.home)?;
                    if !self.pol.in_write_roots(&lex) || !self.pol.in_write_roots(&physical(&lex)) {
                        return Err(deny("S8-git", "git -C outside the workspace"));
                    }
                    cwd = lex;
                    i += 2;
                    continue;
                }
                "--no-pager"
                | "--no-optional-locks"
                | "--literal-pathspecs"
                | "--no-replace-objects"
                | "-P"
                | "--version"
                | "--help" => {}
                _ => {
                    return Err(deny(
                        "S8-git",
                        format!("git option {t} can run programs or point git elsewhere"),
                    ));
                }
            }
            i += 1;
        }
        let Some(sub) = args.get(i) else {
            return Ok(());
        };
        let sub = sub.text.as_str();
        if !GIT_ALLOW.contains(&sub) {
            return Err(deny("S8-git", format!("git {sub} is not allowed (push, fetch, clone, config, hooks, gc, reset ... are refused)")));
        }
        let rest = &args[i + 1..];
        for a in rest {
            let t = a.text.as_str();
            if (sub == "grep"
                && (t == "-O"
                    || t.starts_with("--open-files-in-pager")
                    || (t.starts_with("-O") && !t.starts_with("--"))))
                || t.starts_with("--output")
                || t.starts_with("--ext-diff")
                || t.starts_with("--exec")
                || t.starts_with("--upload-pack")
                || t.starts_with("--receive-pack")
                || t.starts_with("--config")
                || t == "-c"
            {
                return Err(deny(
                    "S8-git",
                    format!("git {sub} {t} writes files or runs programs"),
                ));
            }
        }
        match sub {
            "branch" | "tag" => {
                if rest.iter().any(|a| {
                    matches!(
                        a.text.as_str(),
                        "-D" | "-d"
                            | "--delete"
                            | "-M"
                            | "-m"
                            | "-f"
                            | "--force"
                            | "-u"
                            | "--set-upstream-to"
                            | "--edit-description"
                    )
                }) {
                    return Err(deny(
                        "S8-git",
                        format!(
                            "git {sub}: delete / rename / force / upstream changes are refused"
                        ),
                    ));
                }
            }
            "remote" => {
                if rest
                    .iter()
                    .any(|a| !a.text.starts_with('-') && a.text != "show")
                {
                    return Err(deny("S8-git", "git remote: only listing is allowed"));
                }
            }
            "stash" => {
                if rest
                    .first()
                    .is_some_and(|a| matches!(a.text.as_str(), "drop" | "clear" | "pop"))
                {
                    return Err(deny("S8-git", "git stash drop/clear/pop"));
                }
            }
            "restore" | "switch"
                if rest.iter().any(|a| {
                    matches!(
                        a.text.as_str(),
                        "-f" | "--force" | "--discard-changes" | "-C" | "--force-create"
                    )
                }) =>
            {
                return Err(deny("S8-git", format!("git {sub} --force discards work")));
            }
            _ => {}
        }
        let sub_cmd = Cmd {
            pol: self.pol,
            cwd,
            depth: self.depth,
        };
        for a in rest {
            if looks_like_path(&a.text) && !a.text.starts_with('-') {
                sub_cmd.word_path(a, Access::Read)?;
            }
        }
        Ok(())
    }

    fn cargo(&self, args: &[&Word]) -> R {
        for a in args {
            let t = a.text.as_str();
            if t.starts_with("--config")
                || t.starts_with("-Z")
                || t.starts_with("--target-dir") && t.contains("..")
            {
                return Err(deny(
                    "S8-cargo",
                    format!("cargo {t} changes build behaviour"),
                ));
            }
        }
        let sub = args
            .iter()
            .find(|a| !a.text.starts_with('-') || a.text == "--version" || a.text == "-V");
        if args.iter().any(|a| a.text.starts_with('+')) {
            return Err(deny("S8-cargo", "cargo +toolchain"));
        }
        let Some(sub) = sub else { return Ok(()) };
        let s = sub.text.as_str();
        if !CARGO_ALLOW.contains(&s) {
            return Err(deny(
                "S8-cargo",
                format!("cargo {s} is not allowed (install, run, publish, login ... are refused)"),
            ));
        }
        for a in args {
            let t = a.text.as_str();
            if t.starts_with("--config")
                || t.starts_with("-Z")
                || t.starts_with("--target-dir") && t.contains("..")
            {
                return Err(deny(
                    "S8-cargo",
                    format!("cargo {t} changes build behaviour"),
                ));
            }
        }
        let offline = args
            .iter()
            .any(|a| matches!(a.text.as_str(), "--offline" | "--frozen"));
        if !offline && !matches!(s, "version" | "--version" | "-V" | "fmt") {
            return Err(deny(
                "S8-cargo",
                "cargo must run with --offline or --frozen (no downloads)",
            ));
        }
        for pair in args.windows(2) {
            if pair[0].text == "--manifest-path" {
                self.path(&pair[1].text, Access::Read)?;
            }
            if pair[0].text == "--target-dir" {
                self.path(&pair[1].text, Access::Write)?;
            }
        }
        Ok(())
    }
}

/// Check one shell command string.
pub fn check_command(cmd: &str, pol: &Policy) -> R {
    check_command_in(cmd, &pol.cwd.clone(), pol)
}

pub fn check_command_in(cmd: &str, cwd: &Path, pol: &Policy) -> R {
    let mut c = Cmd {
        pol,
        cwd: cwd.to_path_buf(),
        depth: 0,
    };
    c.all(cmd)
}

// --------------------------------------------------------------- tool calls

fn tokens(name: &str) -> Vec<String> {
    name.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}

fn has_hint(name: &str, hints: &[&str]) -> bool {
    let l = name.to_ascii_lowercase();
    let toks = tokens(name);
    hints.iter().any(|h| {
        if h.contains('_') {
            l.contains(h)
        } else {
            toks.iter().any(|t| t == h)
        }
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Kind {
    pub shell: bool,
    pub code: bool,
    pub write: bool,
    pub delegate: bool,
    pub vault: bool,
}

pub fn kind(tool: &str) -> Kind {
    let delegate = has_hint(tool, DELEGATE_TOOL_HINTS);
    Kind {
        code: has_hint(tool, CODE_TOOL_HINTS),
        shell: !delegate && has_hint(tool, SHELL_TOOL_HINTS),
        write: has_hint(tool, WRITE_TOOL_HINTS),
        delegate,
        vault: has_hint(tool, &["vault", "obsidian", "notes", "note"]),
    }
}

struct Walk<'a> {
    tool: &'a str,
    k: Kind,
    pol: &'a Policy,
    cwd: PathBuf,
    strings: usize,
    bytes: usize,
    commands: usize,
    /// Some string of the call asks for a write (delegated tasks).
    wants_write: bool,
}

fn key_in(key: &str, set: &[&str]) -> bool {
    let k = key.to_ascii_lowercase();
    set.contains(&k.as_str())
}

/// Path-like tokens in free text (goals, contexts, content, queries).
fn text_paths(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || "\"'`,;()[]{}<>|=".contains(c))
        .map(|t| t.trim_end_matches(['.', ':', '!', '?']))
        .filter(|t| {
            t.starts_with('/')
                || t.starts_with("~/")
                || t.contains("../")
                || t.starts_with("file://")
        })
        .map(String::from)
        .collect()
}

fn mentions_write(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    WRITE_VERBS.iter().any(|v| {
        l.match_indices(v).any(|(i, _)| {
            let before = l[..i]
                .chars()
                .last()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            let after = l[i + v.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            before && after
        })
    })
}

impl<'a> Walk<'a> {
    fn string(&mut self, key: &str, s: &str) -> R {
        self.strings += 1;
        self.bytes += s.len();
        if s.len() > MAX_STRING_BYTES || self.strings > MAX_STRINGS || self.bytes > MAX_TOTAL_BYTES
        {
            return Err(deny("S9-size", "tool arguments too large to check"));
        }
        if let Some(c) = s
            .chars()
            .find(|c| shell::suspicious_char(*c) && *c != '\u{feff}')
        {
            if self.k.shell || key_in(key, PATH_KEYS) || key_in(key, COMMAND_KEYS) {
                return Err(deny(
                    "S2-unicode",
                    format!("invisible or look-alike character U+{:04X}", c as u32),
                ));
            }
        }
        // P: secrets and guard-disarm floor rules on every string of every tool
        prior_check(s, false, false)?;
        let is_cmd_key = key_in(
            key,
            &["command", "cmd", "script", "commandline", "shell", "bash"],
        ) || (self.k.shell && key_in(key, COMMAND_KEYS));
        if self.k.shell && is_cmd_key {
            self.commands += 1;
            return check_command_in(s, &self.cwd.clone(), self.pol);
        }
        if is_cmd_key {
            // a command handed to a non-shell tool (MCP, the cargo tool):
            // same deny checks; the terminal allowlist does not apply
            self.commands += 1;
            let mut p = self.pol.clone();
            p.mode = Mode::Denylist;
            return check_command_in(s, &self.cwd.clone(), &p);
        }
        if key_in(key, &["cwd", "workdir", "working_dir", "working_directory"]) {
            let lex = lexical(s, &self.cwd, &self.pol.home)?;
            if !self.pol.in_write_roots(&lex) || !self.pol.in_write_roots(&physical(&lex)) {
                return Err(deny(
                    "S6-cd",
                    format!(
                        "working directory {} is outside the workspace",
                        lex.display()
                    ),
                ));
            }
            self.cwd = physical(&lex);
            return Ok(());
        }
        if key_in(key, PATH_KEYS)
            || (self.k.write && key.eq_ignore_ascii_case("name") && looks_like_path(s))
        {
            prior_check(s, true, false)?;
            let a = if self.k.write || self.k.delegate {
                Access::Write
            } else {
                Access::Read
            };
            if self.k.vault && self.k.write {
                return Err(deny(
                    "S9-vault-write",
                    format!("{} writes to the vault, which is read-only", self.tool),
                ));
            }
            return check_path_x(s, a, &self.cwd, self.pol, true);
        }
        if key_in(key, PATCH_KEYS) {
            for line in s.lines() {
                let p = line
                    .strip_prefix("+++ ")
                    .or_else(|| line.strip_prefix("--- "))
                    .or_else(|| line.strip_prefix("*** Update File: "))
                    .or_else(|| line.strip_prefix("*** Add File: "))
                    .or_else(|| line.strip_prefix("*** Delete File: "))
                    .or_else(|| line.strip_prefix("*** Move to: "))
                    .or_else(|| line.strip_prefix("rename to "))
                    .or_else(|| line.strip_prefix("copy to "));
                if let Some(p) = p {
                    let p = p.split('\t').next().unwrap_or("").trim();
                    let p = p
                        .strip_prefix("a/")
                        .or_else(|| p.strip_prefix("b/"))
                        .unwrap_or(p);
                    if p != "/dev/null" && !p.is_empty() {
                        check_path_x(p, Access::Write, &self.cwd, self.pol, true)?;
                    }
                }
                if let Some(rest) = line.strip_prefix("diff --git ") {
                    for p in rest.split_whitespace() {
                        let p = p
                            .strip_prefix("a/")
                            .or_else(|| p.strip_prefix("b/"))
                            .unwrap_or(p);
                        check_path_x(p, Access::Write, &self.cwd, self.pol, true)?;
                    }
                }
            }
            return Ok(());
        }
        if key_in(key, URL_KEYS) {
            return check_url(s, self.pol, false);
        }
        // free text: secret paths anywhere; protected writes for delegated work
        let wants_write = self.wants_write;
        for p in text_paths(s) {
            check_path_x(&p, Access::Read, &self.cwd, self.pol, false)
                .map_err(|e| deny(&e.rule, format!("path in {key}: {}", e.reason)))?;
            if wants_write && self.k.delegate {
                check_path_x(&p, Access::Write, &self.cwd, self.pol, false).map_err(|e| {
                    deny(
                        if e.rule == "S9-vault-write" {
                            "S9-vault-write"
                        } else {
                            "S9-delegate-write"
                        },
                        format!("delegated task asks to write {}: {}", p, e.reason),
                    )
                })?;
            }
            if p.contains("://") {
                check_url(&p, self.pol, false)?;
            }
        }
        if self.k.delegate && wants_write {
            let l = s.to_ascii_lowercase();
            if l.contains("vault") || l.contains("obsidian") || l.contains("build-session") {
                return Err(deny("S9-vault-write", "delegated task asks to write to the vault, which is read-only; delegation can not be used to get around a refusal"));
            }
            for w in [
                "soul.md",
                "agents.md",
                "guard.json",
                "dangerous-patterns",
                "command-guard",
                "config.yaml",
                ".env",
                "authorized_keys",
                "systemd",
                "crontab",
                "cron.d",
            ] {
                if l.contains(w) {
                    return Err(deny(
                        "S9-delegate-write",
                        format!("delegated task asks to write {w}, a protected file"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn value(&mut self, key: &str, v: &Value, depth: usize) -> R {
        if depth > MAX_DEPTH {
            return Err(deny("S9-size", "tool arguments nested too deeply"));
        }
        match v {
            Value::String(s) => self.string(key, s),
            Value::Array(a) => {
                if self.k.shell && key_in(key, &["argv", "args"]) && a.iter().all(Value::is_string)
                {
                    // literal argv: quote each element so the lexer sees words
                    let q: Vec<String> = a
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
                        .collect();
                    self.commands += 1;
                    return check_command_in(&q.join(" "), &self.cwd.clone(), self.pol);
                }
                if a.len() > MAX_STRINGS {
                    return Err(deny("S9-size", "tool arguments too large to check"));
                }
                for x in a {
                    self.value(key, x, depth + 1)?;
                }
                Ok(())
            }
            Value::Object(o) => {
                if key_in(key, ENV_KEYS) {
                    for (k, val) in o {
                        if !ENV_ALLOW.contains(&k.as_str()) {
                            return Err(deny(
                                "S5-tool-env",
                                format!("{k} may not be set through the tool env"),
                            ));
                        }
                        self.value("value", val, depth + 1)?;
                    }
                    return Ok(());
                }
                // working directory first, so later paths resolve against it
                for wk in ["cwd", "workdir", "working_dir", "working_directory"] {
                    if let Some(Value::String(s)) = o.get(wk) {
                        self.string(wk, s)?;
                    }
                }
                for (k, val) in o {
                    if key_in(k, &["cwd", "workdir", "working_dir", "working_directory"]) {
                        continue;
                    }
                    self.value(k, val, depth + 1)?;
                }
                Ok(())
            }
            Value::Number(n) => {
                if key_in(key, PATH_KEYS) || key_in(key, COMMAND_KEYS) {
                    self.string(key, &n.to_string())?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// The single check every tool call passes before it runs.
pub fn check_tool_call(tool: &str, args: &Value, pol: &Policy) -> R {
    let k = kind(tool);
    let name_l = tool.to_ascii_lowercase();
    prior_check(tool, false, false)?;
    if k.code {
        return Err(deny(
            "S9-code-tool",
            format!("{tool} runs inline code the deny list can not inspect"),
        ));
    }
    if k.vault && k.write {
        return Err(deny(
            "S9-vault-write",
            format!("{tool} writes to the vault, which is read-only"),
        ));
    }
    let mut w = Walk {
        tool,
        k,
        pol,
        cwd: pol.cwd.clone(),
        strings: 0,
        bytes: 0,
        commands: 0,
        wants_write: (k.delegate || k.write)
            && mentions_write(&args.to_string().replace("\\n", " ")),
    };
    match args {
        Value::Object(_) | Value::Null => w.value("", args, 0)?,
        Value::String(s) if k.shell => {
            w.commands += 1;
            check_command_in(s, &pol.cwd, pol)?
        }
        other => w.value("", other, 0)?,
    }
    if k.shell && w.commands == 0 {
        return Err(deny(
            "S9-shell-no-command",
            format!("{name_l}: no command argument the deny list can inspect"),
        ));
    }
    Ok(())
}

/// The message a refused tool call returns to the model.
pub fn refusal_text(d: &Deny) -> String {
    format!(
        "blocked by the Hermes deny list ({}): {}. Do not retry this, rephrase it, or route it through another tool or a delegated agent. Tell the operator what you wanted to do.",
        d.rule, d.reason
    )
}
