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
    /// Owner user and group, when the list was made with
    /// `find -printf '%m %y %u %g %p\n'` (slot 379).
    pub owner: Option<(String, String)>,
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
    // Optional `USER GROUP` before the path (`%u %g`): only when the path part
    // does not itself start with `/` or `.` and the remainder does.
    let (owner, path) = split_owner(path);
    Some(StatEntry {
        mode,
        kind,
        path: path.to_string(),
        owner,
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

/// Parse every valid line of a stat list.
pub fn parse_stat_list(text: &str) -> Vec<StatEntry> {
    text.lines().filter_map(parse_stat_line).collect()
}

/// Lint a whole stat list. Entries under a listed directory that lacks
/// other-execute are skipped (other users cannot reach them). Returns the
/// findings and the number of entries judged.
pub fn scan_stat_list(text: &str) -> (Vec<Finding>, usize) {
    let entries = parse_stat_list(text);
    let dirs = dir_modes(&entries);
    let reachable = |p: &str| reachable_in(&dirs, p);
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

/// Split an optional `USER GROUP ` prefix off a stat-list path.
fn split_owner(rest: &str) -> (Option<(String, String)>, &str) {
    if rest.starts_with('/') || rest.starts_with('.') {
        return (None, rest);
    }
    let mut it = rest.splitn(3, ' ');
    if let (Some(u), Some(g), Some(p)) = (it.next(), it.next(), it.next()) {
        if !u.is_empty() && !g.is_empty() && (p.starts_with('/') || p.starts_with('.')) {
            return (Some((u.to_string(), g.to_string())), p);
        }
    }
    (None, rest)
}

/// Directory modes by path, for the reachability walk.
fn dir_modes(entries: &[StatEntry]) -> HashMap<&str, u32> {
    entries
        .iter()
        .filter(|e| e.kind == Kind::Dir)
        .map(|e| (e.path.trim_end_matches('/'), e.mode))
        .collect()
}

/// Can a non-owner uid reach `p`: every listed ancestor has other-execute.
fn reachable_in(dirs: &HashMap<&str, u32>, p: &str) -> bool {
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
}

// ---------------------------------------------------------------------------
// Staged chmod plan (slot 379, D-356-3). The plan is DATA: one row per path
// with the expected owner, the mode now and the mode after. Nothing here
// changes a file; `render_plan_sh` prints guarded commands for the owner to
// run, and `verify_plan` compares a later stat list against the plan.
// ---------------------------------------------------------------------------

/// Why a path is in the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// other-writable directory without the sticky bit -> `o-w`
    DirOtherWritable,
    /// secret-like file other-writable -> 600
    SecretWritable,
    /// group/other-writable entry inside an installed tree (`--plan-tree`) ->
    /// 755 dirs / 755 executables / 644 files
    TreeNormalize,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Reason::DirOtherWritable => "dir-o+w-no-sticky",
            Reason::SecretWritable => "secret-o+w",
            Reason::TreeNormalize => "tree-normalize",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "dir-o+w-no-sticky" => Some(Reason::DirOtherWritable),
            "secret-o+w" => Some(Reason::SecretWritable),
            "tree-normalize" => Some(Reason::TreeNormalize),
            _ => None,
        }
    }
}

/// One plan row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRow {
    pub path: String,
    pub kind: Kind,
    /// expected `user:group` at apply time (`?` when the list had no owners)
    pub owner: String,
    pub old: u32,
    pub new: u32,
    pub reason: Reason,
}

/// Plan rows plus what was left out and why.
#[derive(Debug, Default)]
pub struct Plan {
    pub rows: Vec<PlanRow>,
    /// flagged but under a dir without other-execute: listed, not planned
    pub hidden: Vec<String>,
    /// planned paths not owned by root:root (need a reader/writer review)
    pub review: Vec<String>,
}

fn kind_char(k: Kind) -> char {
    match k {
        Kind::File => 'f',
        Kind::Dir => 'd',
        Kind::Other => 'o',
    }
}

/// Target mode for an entry inside an installed tree.
pub fn normalized_mode(mode: u32, kind: Kind) -> u32 {
    match kind {
        Kind::Dir => 0o755,
        _ if mode & 0o111 != 0 => 0o755,
        _ => 0o644,
    }
}

fn under(path: &str, prefix: &str) -> bool {
    let p = prefix.trim_end_matches('/');
    path == p || path.strip_prefix(p).is_some_and(|r| r.starts_with('/'))
}

/// Build the staged plan from a stat list. `trees` are installed trees (for
/// example `/opt/galion/console-dist`) whose group/other-writable entries are
/// normalized to 755/644 even when they are not secret-like.
pub fn build_plan(entries: &[StatEntry], trees: &[String]) -> Plan {
    let dirs = dir_modes(entries);
    let mut plan = Plan::default();
    for e in entries {
        let m = e.mode & 0o7777;
        let in_tree = trees.iter().any(|t| under(&e.path, t));
        let (new, reason) = if in_tree && e.kind != Kind::Other && m & (O_W | G_W) != 0 {
            (normalized_mode(m, e.kind), Reason::TreeNormalize)
        } else if e.kind == Kind::Dir && m & O_W != 0 && m & STICKY == 0 {
            (m & !O_W, Reason::DirOtherWritable)
        } else if e.kind == Kind::File && m & O_W != 0 && is_secret_like(&e.path) {
            (0o600, Reason::SecretWritable)
        } else {
            continue;
        };
        if !reachable_in(&dirs, &e.path) {
            plan.hidden.push(e.path.clone());
            continue;
        }
        let owner = match &e.owner {
            Some((u, g)) => format!("{u}:{g}"),
            None => "?".to_string(),
        };
        if owner != "root:root" {
            plan.review.push(format!("{} ({owner})", e.path));
        }
        plan.rows.push(PlanRow {
            path: e.path.clone(),
            kind: e.kind,
            owner,
            old: m,
            new,
            reason,
        });
    }
    // dirs first (parents before children), then files: a later file chmod
    // never runs while its directory is still world-writable.
    plan.rows
        .sort_by(|a, b| (a.kind != Kind::Dir, &a.path).cmp(&(b.kind != Kind::Dir, &b.path)));
    plan
}

/// Swap old and new: the rollback plan.
pub fn rollback(rows: &[PlanRow]) -> Vec<PlanRow> {
    rows.iter()
        .map(|r| PlanRow {
            old: r.new,
            new: r.old,
            ..r.clone()
        })
        .collect()
}

/// TSV: `path<TAB>type<TAB>owner<TAB>old<TAB>new<TAB>reason`, `#` comments.
pub fn render_plan_tsv(rows: &[PlanRow], header: &str) -> String {
    let mut s = String::new();
    for l in header.lines() {
        s.push_str("# ");
        s.push_str(l);
        s.push('\n');
    }
    s.push_str("# path\ttype\towner\told\tnew\treason\n");
    for r in rows {
        s.push_str(&format!(
            "{}\t{}\t{}\t{:o}\t{:o}\t{}\n",
            r.path,
            kind_char(r.kind),
            r.owner,
            r.old,
            r.new,
            r.reason.name()
        ));
    }
    s
}

/// Parse a plan TSV written by [`render_plan_tsv`]. Errors name the line.
pub fn parse_plan_tsv(text: &str) -> Result<Vec<PlanRow>, String> {
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let bad = |w: &str| format!("plan line {}: {w}", i + 1);
        if f.len() != 6 {
            return Err(bad("expected 6 tab-separated fields"));
        }
        let oct = |s: &str| {
            u32::from_str_radix(s, 8)
                .ok()
                .filter(|m| *m <= 0o7777)
                .ok_or_else(|| bad("bad octal mode"))
        };
        let kind = match f[1] {
            "f" => Kind::File,
            "d" => Kind::Dir,
            "o" => Kind::Other,
            _ => return Err(bad("type must be f, d or o")),
        };
        if !f[0].starts_with('/') {
            return Err(bad("path must be absolute"));
        }
        rows.push(PlanRow {
            path: f[0].to_string(),
            kind,
            owner: f[2].to_string(),
            old: oct(f[3])?,
            new: oct(f[4])?,
            reason: Reason::parse(f[5]).ok_or_else(|| bad("unknown reason"))?,
        });
    }
    Ok(rows)
}

/// Single-quote a path for POSIX sh.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Guarded commands: each path is changed only when its current
/// `mode user:group` is exactly what the plan expects; anything else is
/// skipped and counted. `-h`/no-follow: `stat` without `-L`, and the type is
/// re-checked so a swapped-in symlink is never chmodded.
pub fn render_plan_sh(rows: &[PlanRow], title: &str) -> String {
    let mut s = String::new();
    s.push_str(
        "#!/bin/sh\n# SPDX-License-Identifier: MIT\n# Copyright (c) 3000 Galion / galion-studio\n",
    );
    for l in title.lines() {
        s.push_str("# ");
        s.push_str(l);
        s.push('\n');
    }
    s.push_str(
        "# Generated by `repo-check --chmod-plan --sh` from a plan TSV. STAGED, NOT APPLIED.\n",
    );
    s.push_str("set -u\nok=0; skip=0\n");
    s.push_str("g() { # path type expect_mode expect_owner new_mode\n");
    s.push_str("  t=$(stat -c %F -- \"$1\" 2>/dev/null) || { echo \"SKIP missing $1\"; skip=$((skip+1)); return; }\n");
    s.push_str("  case \"$2:$t\" in d:directory|f:\"regular file\"|f:\"regular empty file\") ;; *) echo \"SKIP type $1 ($t)\"; skip=$((skip+1)); return;; esac\n");
    s.push_str("  cur=$(stat -c '%a %U:%G' -- \"$1\")\n");
    s.push_str("  if [ \"$4\" = \"?\" ]; then want_o=\"${cur#* }\"; else want_o=\"$4\"; fi\n");
    s.push_str("  if [ \"$cur\" = \"$3 $want_o\" ]; then chmod \"$5\" -- \"$1\" && ok=$((ok+1)); else echo \"SKIP drift $1 (now $cur, plan $3 $4)\"; skip=$((skip+1)); fi\n");
    s.push_str("}\n");
    for r in rows {
        s.push_str(&format!(
            "g {} {} {:o} {} {:o}\n",
            sh_quote(&r.path),
            kind_char(r.kind),
            r.old,
            sh_quote(&r.owner),
            r.new
        ));
    }
    s.push_str("echo \"changed $ok, skipped $skip\"\n[ \"$skip\" -eq 0 ]\n");
    s
}

/// State of one plan row in a later stat list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    Pending,
    Applied,
    Missing,
    /// mode is neither old nor new
    Drift(u32),
    /// owner differs from the plan (only when both are known)
    OwnerChanged(String),
}

/// Compare a stat list with a plan.
pub fn verify_plan(rows: &[PlanRow], entries: &[StatEntry]) -> Vec<(PlanRow, RowState)> {
    let by_path: HashMap<&str, &StatEntry> = entries.iter().map(|e| (e.path.as_str(), e)).collect();
    rows.iter()
        .map(|r| {
            let st = match by_path.get(r.path.as_str()) {
                None => RowState::Missing,
                Some(e) => {
                    let m = e.mode & 0o7777;
                    let owner = e.owner.as_ref().map(|(u, g)| format!("{u}:{g}"));
                    match owner {
                        Some(o) if r.owner != "?" && o != r.owner => RowState::OwnerChanged(o),
                        _ if m == r.new => RowState::Applied,
                        _ if m == r.old => RowState::Pending,
                        _ => RowState::Drift(m),
                    }
                }
            };
            (r.clone(), st)
        })
        .collect()
}
