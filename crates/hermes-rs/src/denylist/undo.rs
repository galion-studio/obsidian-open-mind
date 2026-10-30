// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Undo copies for full-write mode: before a tool call that may delete or
//! overwrite files in the workspace runs, the guard copies what it would
//! touch into the undo store (`<workspace>/.hermes-trash/<id>/`, or
//! `HERMES_RS_GUARD_TRASH`). The copies are made by the gateway's own user
//! with the original modes minus group/world write (so `cp -a` restores a
//! file as it was), so the sandboxed terminal (which runs as `nobody`) can
//! read and copy them back but not change or delete them, and the guard
//! refuses every tool call that names the store. Each copy has a
//! `MANIFEST.txt` with the call and a restore line.
//!
//! * Known file commands (`rm`, `mv`, `cp`, `>`, `tee`, `sed -i`, `truncate`,
//!   `dd of=`, `gzip` ..., `write_file`, `edit_file`): only their existing
//!   targets are copied.
//! * Anything whose writes the deny list can not see (interpreters, scripts,
//!   `make`, `find -delete`, `git checkout/reset/clean`, globs ...): the whole
//!   workspace is copied, `.git/` included (regenerable build output
//!   `target/`, `node_modules/`, `__pycache__/`, `.tmp/` and the undo store
//!   itself excluded, also from targeted copies);
//!   unchanged files are hard links to the previous whole copy, so repeat
//!   copies cost almost nothing.
//! * Fail closed: if the copy can not be made (too big, store full, I/O
//!   error) the call is refused (S9-no-undo).
//!
//! Nothing here deletes: the store only grows. Pruning is an operator job.

use super::rules::*;
use super::shell::{RedirKind, Word};
use super::*;
use std::collections::BTreeSet;
use std::io::Write as _;

/// The undo store's directory name inside the workspace.
pub const TRASH_NAME: &str = ".hermes-trash";
/// Directories left out of a whole-workspace copy (regenerable output).
pub const WHOLE_EXCLUDE: &[&str] = &[TRASH_NAME, ".tmp", "target", "node_modules", "__pycache__"];
/// Regenerable build output: not copied by a targeted copy either
/// (`rm -rf target` / `node_modules` loses only build output).
pub const REGENERABLE: &[&str] = &["target", "node_modules", "__pycache__", ".tmp"];
/// Default cap on one undo copy (bytes, files).
pub const DEFAULT_MAX_COPY_BYTES: u64 = 64 << 20;
pub const DEFAULT_MAX_COPY_FILES: u64 = 20_000;
/// Default cap on the whole undo store (new bytes written, hard links free).
pub const DEFAULT_MAX_STORE_BYTES: u64 = 1 << 30;

/// What a call may delete or overwrite.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Existing paths (lexical, absolute) that will be copied.
    pub paths: Vec<PathBuf>,
    /// The call's writes can not be seen: copy the whole workspace.
    pub whole: bool,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        !self.whole && self.paths.is_empty()
    }
}

/// A finished undo copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub id: String,
    pub dir: PathBuf,
    pub files: u64,
    pub bytes: u64,
    pub whole: bool,
    /// Workspace-relative paths that were copied.
    pub rel: Vec<String>,
    pub workspace: PathBuf,
}

impl Saved {
    /// The note the model sees after the call.
    pub fn note(&self) -> String {
        let what = if self.whole {
            "a copy of the workspace".to_string()
        } else {
            format!("copies of {}", self.rel.join(", "))
        };
        format!(
            "undo: {what} taken before this call is in {} ({} file(s)); to undo, copy back with: cp -a {}/<path> <path> (from the workspace root)",
            self.dir.display(),
            self.files,
            self.dir.display()
        )
    }
}

fn env_u64(k: &str, d: u64) -> u64 {
    std::env::var(k)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(d)
}

/// Commands that never delete or overwrite an existing file (they only
/// read, print, or create new directories / empty files / metadata).
const NON_DESTRUCTIVE: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "cut",
    "tr",
    "jq",
    "gojq",
    "yq",
    "diff",
    "cmp",
    "comm",
    "file",
    "stat",
    "du",
    "df",
    "pwd",
    "echo",
    "printf",
    "date",
    "tree",
    "realpath",
    "readlink",
    "basename",
    "dirname",
    "true",
    "false",
    "test",
    "[",
    "which",
    "mkdir",
    "touch",
    "chmod",
    "chown",
    "chgrp",
    "column",
    "nl",
    "fold",
    "fmt",
    "paste",
    "join",
    "expand",
    "unexpand",
    "rev",
    "tac",
    "sha256sum",
    "sha1sum",
    "sha512sum",
    "md5sum",
    "b2sum",
    "cksum",
    "cd",
    "seq",
    "yes",
    "sleep",
    "uname",
    "id",
    "whoami",
    "nproc",
    "free",
    "uptime",
    "zcat",
    "xzcat",
    "bzcat",
    "od",
    "hexdump",
    "strings",
    "less",
    "more",
    "type",
    "hostname",
    "ps",
    "env",
    "mkfifo",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "sed",
    "sort",
    "uniq",
    "find",
    "tar",
    "unzip",
    "zip",
    "git",
    "cargo",
    "timeout",
    "nice",
    "nohup",
    "stdbuf",
    "time",
    "command",
    "xargs",
    "ionice",
    "lsblk",
    "printenv",
    "getent",
    "locale",
    "nm",
    "objdump",
    "readelf",
];

/// Lexical path of `raw` against `cwd` (no symlink resolution: a symlink
/// target is copied as the link itself).
fn lex(raw: &str, cwd: &Path, pol: &Policy) -> Option<PathBuf> {
    lexical(raw, cwd, &pol.home).ok()
}

struct Planner<'a> {
    pol: &'a Policy,
    cwd: PathBuf,
    plan: Plan,
}

impl<'a> Planner<'a> {
    fn push(&mut self, raw: &str) {
        if raw.is_empty() || raw == "-" || raw.starts_with("/dev/") {
            return;
        }
        if let Some(p) = lex(raw, &self.cwd, self.pol) {
            self.plan.paths.push(p);
        }
    }
    fn push_word(&mut self, w: &Word) {
        if w.glob {
            self.plan.whole = true;
        } else {
            self.push(&w.text.clone());
        }
    }

    fn command(&mut self, src: &str) {
        let Ok(segs) = shell::lex(src) else {
            // run as opaque shell text (full-write fallback): unseen writes
            self.plan.whole = true;
            return;
        };
        for s in &segs {
            for r in &s.redirs {
                if r.kind == RedirKind::Write {
                    self.push_word(&r.target);
                }
            }
            let words: Vec<&Word> = s
                .words
                .iter()
                .skip_while(|w| is_assignment(w).is_some())
                .collect();
            self.words(&words, 0);
        }
    }

    fn words(&mut self, words: &[&Word], depth: usize) {
        let Some(cmd) = words.first() else { return };
        let args = &words[1..];
        let name = cmd
            .text
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let n = name.as_str();
        let ops: Vec<&&Word> = args
            .iter()
            .filter(|a| !a.text.starts_with('-') || a.text == "-")
            .collect();
        match n {
            "cd" => {
                if let Some(t) = ops.first() {
                    if let Some(p) = lex(&t.text, &self.cwd, self.pol) {
                        self.cwd = physical(&p);
                    }
                }
            }
            "rm" | "rmdir" | "unlink" | "shred" | "truncate" | "tee" | "sponge" | "gzip"
            | "gunzip" | "bzip2" | "bunzip2" | "xz" | "unxz" | "zstd" | "unzstd" | "lz4" | "mv"
            | "ln" | "link" | "patch" | "split" | "csplit" => {
                if matches!(n, "patch" | "split" | "csplit") {
                    self.plan.whole = true;
                    return;
                }
                for (i, a) in ops.iter().enumerate() {
                    if n == "truncate" && i == 0 && args.iter().any(|x| x.text == "-s") {
                        // -s SIZE: the size is not a file
                    }
                    self.push_word(a);
                }
                // mv/ln into a directory: the entry it would replace
                if matches!(n, "mv" | "ln") && ops.len() >= 2 {
                    self.dest_entries(&ops);
                }
                for pair in args.windows(2) {
                    if pair[0].text == "-t" || pair[0].text == "--target-directory" {
                        self.plan.whole = true;
                    }
                }
            }
            "cp" | "install" | "rsync" => {
                if args
                    .iter()
                    .any(|a| a.text == "-t" || a.text.starts_with("--target-directory"))
                {
                    self.plan.whole = true;
                    return;
                }
                if let Some(last) = ops.last() {
                    if ops.len() >= 2 {
                        self.push_word(last);
                        self.dest_entries(&ops);
                    }
                }
            }
            "dd" => {
                for a in args {
                    if let Some(v) = a.text.strip_prefix("of=") {
                        self.push(v);
                    }
                }
            }
            "sed" => {
                let inplace = args.iter().any(|a| {
                    let t = a.text.as_str();
                    t == "-i"
                        || t.starts_with("--in-place")
                        || (t.starts_with('-') && !t.starts_with("--") && t.contains('i'))
                });
                if inplace {
                    // every operand but the script is a file
                    let has_e = args
                        .iter()
                        .any(|a| a.text == "-e" || a.text == "--expression");
                    let mut skip_next = false;
                    let mut script_seen = has_e;
                    for a in args {
                        if skip_next {
                            skip_next = false;
                            continue;
                        }
                        if a.text == "-e" || a.text == "--expression" {
                            skip_next = true;
                            continue;
                        }
                        if a.text.starts_with('-') {
                            continue;
                        }
                        if !script_seen {
                            script_seen = true;
                            continue;
                        }
                        self.push_word(a);
                    }
                }
            }
            "sort" | "uniq" | "time" => {
                for pair in args.windows(2) {
                    if pair[0].text == "-o" || pair[0].text == "--output" {
                        self.push_word(pair[1]);
                    }
                }
                if n == "uniq" {
                    if let Some(out) = ops.get(1) {
                        self.push_word(out);
                    }
                }
                if n == "time" {
                    if let Ok(Some(inner)) = fullwrite::unwrap_wrapper(n, args) {
                        self.inner(inner, depth);
                    }
                }
            }
            "find" => {
                if args.iter().any(|a| {
                    matches!(
                        a.text.as_str(),
                        "-delete"
                            | "-exec"
                            | "-execdir"
                            | "-ok"
                            | "-okdir"
                            | "-fprint"
                            | "-fprint0"
                            | "-fprintf"
                            | "-fls"
                    )
                }) {
                    self.plan.whole = true;
                }
            }
            "tar" | "unzip" | "zip" => {
                let first = args.first().map(|a| a.text.as_str()).unwrap_or("");
                if n != "zip" || first.contains('x') || args.iter().any(|a| a.text == "--extract") {
                    // extraction overwrites unknown files; zip / tar -c the archive
                    self.plan.whole = true;
                }
            }
            "git" => {
                let sub = args
                    .iter()
                    .find(|a| !a.text.starts_with('-'))
                    .map(|a| a.text.as_str())
                    .unwrap_or("");
                if !matches!(
                    sub,
                    "status"
                        | "diff"
                        | "log"
                        | "show"
                        | "blame"
                        | "grep"
                        | "ls-files"
                        | "ls-tree"
                        | "rev-parse"
                        | "rev-list"
                        | "shortlog"
                        | "describe"
                        | "cat-file"
                        | "add"
                        | "commit"
                        | "branch"
                        | "tag"
                        | "remote"
                        | "merge-base"
                        | "name-rev"
                        | "whatchanged"
                        | "count-objects"
                        | "check-ignore"
                        | "help"
                        | "version"
                        | ""
                ) {
                    self.plan.whole = true;
                }
            }
            "cargo" => {
                if args
                    .iter()
                    .any(|a| matches!(a.text.as_str(), "fmt" | "fix" | "new" | "init"))
                {
                    self.plan.whole = true;
                }
            }
            "awk" | "gawk" | "mawk" | "nawk" => {
                // a program with redirects / system() writes unseen files
                if awk_program_ok(
                    &args
                        .iter()
                        .map(|a| a.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                )
                .is_err()
                {
                    self.plan.whole = true;
                }
            }
            "timeout" | "nice" | "ionice" | "nohup" | "stdbuf" | "xargs" | "command" | "env" => {
                if n == "xargs" {
                    // the arguments come from stdin: targets unknown
                    let inner = fullwrite::unwrap_wrapper(n, args).ok().flatten();
                    let destructive = inner
                        .and_then(|i| i.first())
                        .map(|w| w.text.rsplit('/').next().unwrap_or("").to_string())
                        .is_none_or(|c| !NON_DESTRUCTIVE.contains(&c.as_str()) || c == "xargs");
                    if destructive {
                        self.plan.whole = true;
                    }
                    return;
                }
                let inner: Vec<&Word> = if n == "env" {
                    match fullwrite::unwrap_wrapper(n, args) {
                        Ok(Some(i)) => i
                            .iter()
                            .copied()
                            .skip_while(|w| is_assignment(w).is_some())
                            .collect(),
                        _ => Vec::new(),
                    }
                } else {
                    fullwrite::unwrap_wrapper(n, args)
                        .ok()
                        .flatten()
                        .map(|i| i.to_vec())
                        .unwrap_or_default()
                };
                self.inner(&inner, depth);
            }
            _ => {
                if !NON_DESTRUCTIVE.contains(&n) {
                    // interpreters, make, scripts, unknown programs
                    self.plan.whole = true;
                }
            }
        }
    }

    fn inner(&mut self, inner: &[&Word], depth: usize) {
        if depth > 4 {
            self.plan.whole = true;
            return;
        }
        let saved = self.cwd.clone();
        self.words(inner, depth + 1);
        self.cwd = saved;
    }

    /// `mv a b dir/` or `cp a dir`: also the entries the sources would
    /// replace inside an existing destination directory.
    fn dest_entries(&mut self, ops: &[&&Word]) {
        let Some(last) = ops.last() else { return };
        if last.glob {
            return;
        }
        let Some(dest) = lex(&last.text, &self.cwd, self.pol) else {
            return;
        };
        if !dest.is_dir() {
            return;
        }
        for s in &ops[..ops.len() - 1] {
            if s.glob {
                self.plan.whole = true;
                continue;
            }
            if let Some(name) = Path::new(&s.text).file_name() {
                self.plan.paths.push(dest.join(name));
            }
        }
    }
}

fn path_strings<'v>(v: &'v Value, keys: &[&str], out: &mut Vec<&'v str>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    if let Value::Object(o) = v {
        for (k, val) in o {
            let hit = key_in(k, keys);
            match val {
                Value::String(s) if hit => out.push(s),
                Value::Array(a) if hit => {
                    out.extend(a.iter().filter_map(Value::as_str));
                }
                Value::Object(_) | Value::Array(_) => path_strings(val, keys, out, depth + 1),
                _ => {}
            }
        }
    } else if let Value::Array(a) = v {
        for x in a {
            path_strings(x, keys, out, depth + 1);
        }
    }
}

/// What a call (already allowed by [`check_tool_call`]) may delete or
/// overwrite. Empty unless full-write mode is on.
pub fn plan(tool: &str, args: &Value, pol: &Policy) -> Plan {
    if !pol.full_write_on() {
        return Plan::default();
    }
    let k = kind(tool);
    let mut cwd = pol.cwd.clone();
    if let Value::Object(o) = args {
        for wk in ["cwd", "workdir", "working_dir", "working_directory"] {
            if let Some(Value::String(s)) = o.get(wk) {
                if let Some(p) = lex(s, &cwd, pol) {
                    cwd = physical(&p);
                }
            }
        }
    }
    let mut pl = Planner {
        pol,
        cwd: cwd.clone(),
        plan: Plan::default(),
    };
    if k.shell {
        match args {
            Value::String(s) => pl.command(s),
            Value::Object(o) => {
                for (key, v) in o {
                    if !key_in(key, COMMAND_KEYS) {
                        continue;
                    }
                    match v {
                        Value::String(s) => pl.command(s),
                        Value::Array(a) => {
                            let q: Vec<String> = a
                                .iter()
                                .filter_map(Value::as_str)
                                .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
                                .collect();
                            pl.command(&q.join(" "));
                        }
                        _ => {}
                    }
                    pl.cwd = cwd.clone();
                }
            }
            _ => {}
        }
    } else if k.write && !k.delegate {
        let mut ps = Vec::new();
        path_strings(args, PATH_KEYS, &mut ps, 0);
        for p in ps {
            pl.push(p);
        }
        let mut patches = Vec::new();
        path_strings(args, PATCH_KEYS, &mut patches, 0);
        if !patches.is_empty() {
            pl.plan.whole = true;
        }
    }
    let mut plan = pl.plan;
    // only existing paths inside a writable root (and not the store)
    let mut seen = BTreeSet::new();
    plan.paths.retain(|p| {
        let regen = p
            .strip_prefix(
                root_for(p, pol)
                    .map(|r| r.as_path())
                    .unwrap_or(Path::new("/")),
            )
            .ok()
            .and_then(|r| r.components().next())
            .is_some_and(|c| REGENERABLE.iter().any(|x| c.as_os_str() == *x));
        !regen
            && p.symlink_metadata().is_ok()
            && pol.in_write_roots(p)
            && !fullwrite::in_trash(p, pol)
            && seen.insert(p.clone())
    });
    if plan.whole {
        plan.paths.clear();
    }
    plan
}

fn root_for<'p>(p: &Path, pol: &'p Policy) -> Option<&'p PathBuf> {
    pol.write_roots.iter().find(|w| under(p, w))
}

struct Copier {
    files: u64,
    bytes: u64,
    new_bytes: u64,
    max_bytes: u64,
    max_files: u64,
    prev: Option<PathBuf>,
}

fn too_big(what: &str) -> Deny {
    deny(
        "S9-no-undo",
        format!("{what}; this call would delete or overwrite files and no undo copy could be kept, so it was not run. Use explicit file commands (rm, mv, cp, sed -i on named files) so only those files are copied, or ask the operator to raise HERMES_RS_GUARD_UNDO_MAX_BYTES"),
    )
}

impl Copier {
    /// Size pass: count what a copy of `p` would take.
    fn measure(&mut self, p: &Path, top_whole: bool) -> R {
        let md = p
            .symlink_metadata()
            .map_err(|e| too_big(&format!("can not read {}: {e}", p.display())))?;
        if md.is_dir() {
            let rd = std::fs::read_dir(p)
                .map_err(|e| too_big(&format!("can not list {}: {e}", p.display())))?;
            for e in rd.flatten() {
                let name = e.file_name();
                if top_whole && WHOLE_EXCLUDE.iter().any(|x| *x == name.to_string_lossy()) {
                    continue;
                }
                self.measure(&e.path(), false)?;
            }
        } else {
            self.files += 1;
            if md.is_file() {
                self.bytes += md.len();
            }
        }
        if self.files > self.max_files {
            return Err(too_big(&format!(
                "more than {} files to copy",
                self.max_files
            )));
        }
        if self.bytes > self.max_bytes {
            return Err(too_big(&format!(
                "more than {} MiB to copy",
                self.max_bytes >> 20
            )));
        }
        Ok(())
    }

    fn same_as_prev(&self, rel: &Path, src: &Path, len: u64) -> Option<PathBuf> {
        let prev = self.prev.as_ref()?.join(rel);
        let md = prev.symlink_metadata().ok()?;
        if !md.is_file() || md.len() != len {
            return None;
        }
        let a = std::fs::read(&prev).ok()?;
        let b = std::fs::read(src).ok()?;
        (a == b).then_some(prev)
    }

    fn copy(&mut self, src: &Path, dst: &Path, rel: &Path, top_whole: bool) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let md = src.symlink_metadata()?;
        if md.file_type().is_symlink() {
            let t = std::fs::read_link(src)?;
            std::os::unix::fs::symlink(t, dst)?;
        } else if md.is_dir() {
            std::fs::create_dir_all(dst)?;
            for e in std::fs::read_dir(src)?.flatten() {
                let name = e.file_name();
                if top_whole && WHOLE_EXCLUDE.iter().any(|x| *x == name.to_string_lossy()) {
                    continue;
                }
                self.copy(&e.path(), &dst.join(&name), &rel.join(&name), false)?;
            }
            // original mode (so `cp -a` restores it as it was), minus
            // group/world write: the copy stays owned by the guard's user
            let mode = md.permissions().mode() & 0o7755;
            std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode))?;
        } else if md.is_file() {
            if let Some(prev) = self.same_as_prev(rel, src, md.len()) {
                if std::fs::hard_link(&prev, dst).is_ok() {
                    return Ok(());
                }
            }
            std::fs::copy(src, dst)?;
            self.new_bytes += md.len();
            let mode = md.permissions().mode() & 0o755;
            std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode))?;
        }
        // fifos, sockets, devices: nothing to keep
        Ok(())
    }
}

fn stamp() -> String {
    // 2026-10-30T15:12:00+01:00 -> 20261030T151200+0100 (Europe/Zagreb)
    crate::time::zagreb_rfc3339(crate::time::now_secs())
        .chars()
        .filter(|c| *c != '-' && *c != ':')
        .collect()
}

fn label_of(tool: &str) -> String {
    tool.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(32)
        .collect()
}

fn store_usage(store: &Path) -> u64 {
    std::fs::read_to_string(store.join(".usage"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Make the undo copy for `plan` (no-op for an empty plan). `tool` and
/// `what` (the command or path) go into the manifest.
pub fn snapshot(plan: &Plan, pol: &Policy, tool: &str, what: &str) -> Result<Option<Saved>, Deny> {
    use std::os::unix::fs::PermissionsExt;
    if plan.is_empty() || !pol.full_write_on() {
        return Ok(None);
    }
    let Some(store) = pol.trash_dir() else {
        return Err(too_big("no undo store is configured"));
    };
    let ws = match plan.paths.first() {
        Some(p) => root_for(p, pol).cloned(),
        None => pol.write_roots.first().cloned(),
    }
    .ok_or_else(|| too_big("no writable workspace"))?;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = Copier {
        files: 0,
        bytes: 0,
        new_bytes: 0,
        max_bytes: env_u64("HERMES_RS_GUARD_UNDO_MAX_BYTES", DEFAULT_MAX_COPY_BYTES),
        max_files: env_u64("HERMES_RS_GUARD_UNDO_MAX_FILES", DEFAULT_MAX_COPY_FILES),
        prev: None,
    };
    // drop paths nested under another planned path
    let mut paths: Vec<PathBuf> = plan.paths.clone();
    paths.sort();
    let mut top: Vec<PathBuf> = Vec::new();
    for p in paths {
        if !top.iter().any(|t| under(&p, t)) {
            top.push(p);
        }
    }
    if plan.whole {
        c.measure(&ws, true)?;
    } else {
        for p in &top {
            c.measure(p, false)?;
        }
    }
    let cap = env_u64("HERMES_RS_GUARD_TRASH_MAX_BYTES", DEFAULT_MAX_STORE_BYTES);
    let used = store_usage(&store);
    if used.saturating_add(c.bytes) > cap {
        return Err(too_big(&format!(
            "the undo store {} is full ({} MiB of {} MiB); the operator has to prune it",
            store.display(),
            used >> 20,
            cap >> 20
        )));
    }
    let io = |e: std::io::Error| too_big(&format!("the undo copy failed: {e}"));
    std::fs::create_dir_all(&store).map_err(io)?;
    let _ = std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755));
    let id = format!(
        "{}-{}-{}-{}",
        stamp(),
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        label_of(tool)
    );
    let dir = store.join(&id);
    std::fs::create_dir(&dir).map_err(io)?;
    let mut rel_list = Vec::new();
    if plan.whole {
        c.prev = std::fs::read_to_string(store.join(".last-whole"))
            .ok()
            .map(|s| store.join(s.trim()))
            .filter(|p| p.is_dir());
        for e in std::fs::read_dir(&ws).map_err(io)?.flatten() {
            let name = e.file_name();
            if WHOLE_EXCLUDE.iter().any(|x| *x == name.to_string_lossy()) {
                continue;
            }
            c.copy(&e.path(), &dir.join(&name), Path::new(&name), false)
                .map_err(io)?;
            rel_list.push(name.to_string_lossy().to_string());
        }
    } else {
        for p in &top {
            let rel = p.strip_prefix(&ws).unwrap_or(p);
            let dst = dir.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            c.copy(p, &dst, rel, false).map_err(io)?;
            rel_list.push(rel.display().to_string());
        }
    }
    let mut m = std::fs::File::create(dir.join("MANIFEST.txt")).map_err(io)?;
    let _ = writeln!(
        m,
        "hermes undo copy {id}\ntaken: {}\ntool: {tool}\ncall: {}\nworkspace: {}\nwhole workspace: {}\nfiles: {}  bytes: {}\ncopied (relative to the workspace):",
        crate::time::zagreb_rfc3339(crate::time::now_secs()),
        what.replace('\n', "\\n").chars().take(2000).collect::<String>(),
        ws.display(),
        plan.whole,
        c.files,
        c.bytes
    );
    for r in &rel_list {
        let _ = writeln!(m, "  {r}");
    }
    let _ = writeln!(
        m,
        "restore one path: cp -a {}/<path> {}/<path>",
        dir.display(),
        ws.display()
    );
    drop(m);
    let _ = std::fs::set_permissions(
        dir.join("MANIFEST.txt"),
        std::fs::Permissions::from_mode(0o444),
    );
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555));
    if plan.whole {
        let _ = std::fs::write(store.join(".last-whole"), &id);
    }
    let _ = std::fs::write(
        store.join(".usage"),
        used.saturating_add(c.new_bytes).to_string(),
    );
    Ok(Some(Saved {
        id,
        dir,
        files: c.files,
        bytes: c.bytes,
        whole: plan.whole,
        rel: rel_list,
        workspace: ws,
    }))
}
