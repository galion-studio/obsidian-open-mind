// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `perms` check: secret-like files that other users can write (or read), and
//! world-writable directories without the sticky bit.
//!
//! Origin: D-318-2. On the VPS `galion-light/publish/obsidian/link.env` is mode
//! 666 and `play.sh` sources it as root, so any local uid can get root code
//! execution. This check works on file *metadata* only (mode bits and names);
//! it never opens a file it flags.
//!
//! Two inputs:
//! * the repo tree (`scan_tree`): mode bits from `std::fs::metadata`;
//! * a stat list (`--stat-list FILE|-`): lines `MODE TYPE PATH` as printed by
//!   `find ROOT -printf '%m %y %p\n'`. This lets an operator run a read-only
//!   `find` on a host (no binary copied there) and lint the result here.
//!
//! In stat-list mode a path counts as reachable by other users only when every
//! listed ancestor directory has the other-execute bit; `/opt/galion/hermes`
//! (700) therefore hides the 666 files below it.

use crate::{Check, Finding, Level};
use std::collections::HashMap;

const O_W: u32 = 0o002;
const O_R: u32 = 0o004;
const O_X: u32 = 0o001;
const G_W: u32 = 0o020;
const STICKY: u32 = 0o1000;

/// Suffixes that mark a template or source file, not a live secret.
const NOT_SECRET_SUFFIX: &[&str] = &[
    ".example",
    ".sample",
    ".template",
    ".dist",
    ".tmpl",
    ".md",
    ".rs",
    ".py",
    ".pyc",
    ".ts",
    ".tsx",
    ".js",
    ".mjs",
    ".cjs",
    ".json5",
    ".html",
    ".css",
    ".txt",
    ".go",
    ".sh",
    ".c",
    ".h",
    ".cpp",
    ".hpp",
    ".sol",
    ".sql",
    ".java",
    ".kt",
    ".rb",
];

/// Exact file names that hold credentials or are sourced as config.
const SECRET_NAMES: &[&str] = &[
    "link.env",
    ".env",
    ".envrc",
    ".netrc",
    ".pgpass",
    ".git-credentials",
    "credentials",
    "credentials.json",
    "authorized_keys",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "api_server_key",
];

/// Extensions of key / certificate / keystore files.
const SECRET_EXTS: &[&str] = &["env", "pem", "key", "p12", "pfx", "jks", "keystore", "kdbx"];

/// Name fragments that suggest a secret (only when no source/doc suffix).
const SECRET_WORDS: &[&str] = &[
    "secret",
    "token",
    "passw",
    "credential",
    "apikey",
    "api_key",
];

/// `w` occurs in `s`; for "token" the next char must not be `i` or `s`
/// (tokenizer, tokenize, added_tokens.json are model files, not secrets).
fn has_word(s: &str, w: &str) -> bool {
    s.match_indices(w)
        .any(|(i, _)| w != "token" || !matches!(s.as_bytes().get(i + w.len()), Some(b'i' | b's')))
}

/// Does this file name look like it holds a secret or is sourced as config?
/// Metadata-only heuristic on the last path component.
pub fn is_secret_like(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    if SECRET_NAMES.contains(&lower.as_str()) {
        return true;
    }
    if NOT_SECRET_SUFFIX.iter().any(|s| lower.ends_with(s)) {
        return false;
    }
    if lower.starts_with(".env.") {
        return true;
    }
    if let Some((_, e)) = lower.rsplit_once('.') {
        if SECRET_EXTS.contains(&e) {
            return true;
        }
    }
    SECRET_WORDS.iter().any(|w| has_word(&lower, w))
}

/// Entry kind from `find -printf %y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Other,
}

/// One parsed stat-list line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatEntry {
    pub mode: u32,
    pub kind: Kind,
    pub path: String,
}

/// Parse `MODE TYPE PATH` (`find -printf '%m %y %p\n'`). `MODE PATH` (no type)
/// is accepted and treated as a file. Returns `None` for blank or bad lines.
pub fn parse_stat_line(line: &str) -> Option<StatEntry> {
    let line = line.trim_end_matches(['\r', '\n']);
    let (mode_s, rest) = line.split_once(' ')?;
    if mode_s.is_empty() || mode_s.len() > 4 || !mode_s.bytes().all(|b| (b'0'..=b'7').contains(&b))
    {
        return None;
    }
    let mode = u32::from_str_radix(mode_s, 8).ok()?;
    let (kind, path) = match rest.split_once(' ') {
        Some((t, p)) if t.len() == 1 && !p.is_empty() => {
            let k = match t {
                "f" => Kind::File,
                "d" => Kind::Dir,
                _ => Kind::Other,
            };
            (k, p)
        }
        _ => (Kind::File, rest),
    };
    if path.is_empty() {
        return None;
    }
    Some(StatEntry {
        mode,
        kind,
        path: path.to_string(),
    })
}

fn mk(level: Level, file: &str, msg: String) -> Finding {
    Finding {
        check: Check::Perms,
        level,
        file: file.to_string(),
        line: 0,
        msg,
    }
}

/// Judge one entry, assuming other users can reach it.
/// * secret-like file, other-writable -> error (rewrite = code run by whoever sources it)
/// * secret-like file, group-writable -> warn
/// * secret-like file, other-readable -> warn (only when `flag_readable`; the
///   repo tree scan sets it false because checked-in files are normally 644)
/// * directory other-writable without sticky bit -> error (any uid can replace entries)
pub fn check_mode(path: &str, mode: u32, kind: Kind, flag_readable: bool) -> Option<Finding> {
    let m = mode & 0o7777;
    match kind {
        Kind::Dir => (m & O_W != 0 && m & STICKY == 0).then(|| {
            mk(
                Level::Error,
                path,
                format!("directory mode {m:o} is world-writable without the sticky bit; any local user can replace files in it (fix: chmod o-w)"),
            )
        }),
        Kind::File if is_secret_like(path) => {
            if m & O_W != 0 {
                Some(mk(
                    Level::Error,
                    path,
                    format!("secret-like file mode {m:o} is world-writable; any local user can rewrite it (fix: chmod 600)"),
                ))
            } else if m & G_W != 0 {
                Some(mk(
                    Level::Warn,
                    path,
                    format!("secret-like file mode {m:o} is group-writable (fix: chmod g-w, or 600)"),
                ))
            } else if flag_readable && m & O_R != 0 {
                Some(mk(
                    Level::Warn,
                    path,
                    format!("secret-like file mode {m:o} is world-readable (fix: chmod 600 if it holds a secret)"),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Lint a whole stat list. Entries under a listed directory that lacks
/// other-execute are skipped (other users cannot reach them). Returns the
/// findings and the number of entries judged.
pub fn scan_stat_list(text: &str) -> (Vec<Finding>, usize) {
    let entries: Vec<StatEntry> = text.lines().filter_map(parse_stat_line).collect();
    let dirs: HashMap<&str, u32> = entries
        .iter()
        .filter(|e| e.kind == Kind::Dir)
        .map(|e| (e.path.trim_end_matches('/'), e.mode))
        .collect();
    let reachable = |p: &str| {
        let mut cur = p.trim_end_matches('/');
        while let Some((parent, _)) = cur.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            if let Some(m) = dirs.get(parent) {
                if m & O_X == 0 {
                    return false;
                }
            }
            cur = parent;
        }
        true
    };
    let mut out = Vec::new();
    for e in &entries {
        if !reachable(&e.path) {
            continue;
        }
        if let Some(f) = check_mode(&e.path, e.mode, e.kind, true) {
            out.push(f);
        }
    }
    (out, entries.len())
}

/// Mode bits of a file in the repo tree (unix only; elsewhere nothing to check).
#[cfg(unix)]
pub fn file_mode(path: &std::path::Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .ok()
        .map(|m| m.permissions().mode())
}

#[cfg(not(unix))]
pub fn file_mode(_path: &std::path::Path) -> Option<u32> {
    None
}
