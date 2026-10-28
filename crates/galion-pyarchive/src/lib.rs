// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Phase A Python archive planner (wave-4 slot 330, follow-up of slot 308;
//! slot 345 adds overridden-base unit flags with an exact safe rollback;
//! `docs/py-archive-plan.md`).
//!
//! Read-only by design: this crate only reads files, symlinks and `/proc`.
//! It never moves, renames, deletes or chmods anything (a test greps this
//! source for filesystem-mutating calls). The only write is the plan output
//! the CLI creates in a fresh `--out` directory the operator chooses.
//!
//! Every path is "logical" (as on the VPS, e.g. `/opt/galion/bin/pw.py`) and
//! is resolved under `root` (default `/`) so tests can run on a fixture tree.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where Phase A goes, once the owner says yes (never created by this tool).
pub const ARCHIVE_DIR: &str = "/opt/galion/archive/py-bin-20261103";
/// Earliest Phase A date from docs/py-archive-plan.md section 5 (7 clean days).
pub const EARLIEST: &str = "2026-11-03T20:05:00+01:00";

/// One Phase A item (docs/py-archive-plan.md section 2 / section 5 row A).
#[derive(Debug, Clone, Copy)]
pub struct Item {
    pub path: &'static str,
    pub why: &'static str,
    /// Other Phase A paths this one needs at run time (restored together on
    /// rollback). The old desk ELF spawns `python3 desk-hermes.py`.
    pub needs: &'static [&'static str],
}

/// The Phase A set. `__pycache__` is a directory and is expanded to its files.
pub const PHASE_A: &[Item] = &[
    Item {
        path: "/opt/galion/bin/desk-hermes.py",
        why: "rollback copy since desk cutover 2026-10-27 19:56 CET (Rust desk-hermes live)",
        needs: &[],
    },
    Item {
        path: "/opt/galion/bin/galion-desk",
        why: "old ELF that spawns python3 desk-hermes.py; replaced by /opt/galion/rust/bin/galion-desk",
        needs: &["/opt/galion/bin/desk-hermes.py"],
    },
    Item {
        path: "/opt/galion/bin/pw.py",
        why: "dead Playwright script; /opt/galion/bin/pw -> rust/bin/pw-check since 2026-10-27 20:02 CET",
        needs: &[],
    },
    Item {
        path: "/opt/galion/bin/__pycache__",
        why: "bytecode cache of pw.py",
        needs: &[],
    },
    Item {
        path: "/opt/galion/bin/replicate-picture.py",
        why: "dead; /opt/galion/bin/replicate-picture -> rust/bin/replicate-picture since 2026-10-27 20:02 CET",
        needs: &[],
    },
];

/// systemd system unit dirs, highest precedence first.
pub const SYSTEM_UNIT_DIRS: &[&str] = &[
    "/etc/systemd/system",
    "/run/systemd/system",
    "/usr/local/lib/systemd/system",
    "/lib/systemd/system",
    "/usr/lib/systemd/system",
];
/// systemd user unit dirs, highest precedence first.
pub const USER_UNIT_DIRS: &[&str] = &[
    "/etc/systemd/user",
    "/run/systemd/user",
    "/usr/local/lib/systemd/user",
    "/lib/systemd/user",
    "/usr/lib/systemd/user",
];
/// Single cron files.
pub const CRON_FILES: &[&str] = &["/etc/crontab", "/etc/anacrontab"];
/// Cron directories (every regular file inside is scanned).
pub const CRON_DIRS: &[&str] = &[
    "/etc/cron.d",
    "/etc/cron.hourly",
    "/etc/cron.daily",
    "/etc/cron.weekly",
    "/etc/cron.monthly",
    "/var/spool/cron/crontabs",
    "/var/spool/cron",
];

/// Unit keys where a later assignment replaces the earlier one. Every other
/// key is treated as a list (only an empty `Key=` resets it), which is the
/// conservative choice: an unknown key can never hide a live reference.
const SINGLE_VALUE_KEYS: &[&str] = &[
    "WorkingDirectory",
    "RootDirectory",
    "User",
    "Group",
    "Type",
    "Description",
    "PIDFile",
    "StandardInput",
    "StandardOutput",
    "StandardError",
];

/// Resolve a logical absolute path under `root`.
pub fn real(root: &Path, logical: &str) -> PathBuf {
    root.join(logical.trim_start_matches('/'))
}

/// A file that would go into the tarball.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub mode: u32,
    pub sha256: String,
}

/// Result of listing the Phase A set.
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Logical paths named in the plan but absent on disk.
    pub missing: Vec<String>,
    /// Paths that are symlinks or special files (refused: archive only real files).
    pub refused: Vec<String>,
}

/// SHA-256 of a file, hex.
pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

fn entry(root: &Path, logical: &str) -> std::io::Result<Entry> {
    use std::os::unix::fs::MetadataExt;
    let p = real(root, logical);
    let md = fs::symlink_metadata(&p)?;
    Ok(Entry {
        path: logical.to_string(),
        size: md.len(),
        mtime: md.mtime(),
        mode: md.mode() & 0o7777,
        sha256: sha256_file(&p)?,
    })
}

fn walk(root: &Path, logical: &str, out: &mut Listing) -> std::io::Result<()> {
    let p = real(root, logical);
    let md = match fs::symlink_metadata(&p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            out.missing.push(logical.to_string());
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let ft = md.file_type();
    if ft.is_dir() {
        let mut names: Vec<String> = fs::read_dir(&p)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for n in names {
            walk(
                root,
                &format!("{}/{}", logical.trim_end_matches('/'), n),
                out,
            )?;
        }
    } else if ft.is_file() {
        out.entries.push(entry(root, logical)?);
    } else {
        out.refused.push(logical.to_string());
    }
    Ok(())
}

/// List (and hash) a set of logical paths under `root`.
pub fn list(root: &Path, paths: &[&str]) -> std::io::Result<Listing> {
    let mut out = Listing::default();
    for p in paths {
        walk(root, p, &mut out)?;
    }
    Ok(out)
}

/// How a reference counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Live: an effective unit line, a cron line, or a running process.
    Active,
    /// Unit line replaced by a later drop-in (`Key=` reset or single-value
    /// override). Not live, but a rollback that removes the drop-in revives it.
    Overridden,
    /// Unit fragment/drop-in shadowed by a same-named file of higher precedence.
    Shadowed,
    /// Unit is masked (symlink to /dev/null).
    Masked,
    /// Comment line.
    Comment,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Active => "active",
            Class::Overridden => "overridden",
            Class::Shadowed => "shadowed",
            Class::Masked => "masked",
            Class::Comment => "comment",
        }
    }
}

/// One textual (or process) reference to a Phase A path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub kind: &'static str,
    pub source: String,
    pub line: usize,
    pub target: String,
    pub class: Class,
    pub text: String,
}

/// Full reference scan.
#[derive(Debug, Default)]
pub struct Scan {
    pub refs: Vec<Ref>,
    /// Files we could not read: the proof cannot hold while this is non-empty.
    pub unreadable: Vec<String>,
    pub units_scanned: usize,
    pub cron_files_scanned: usize,
    pub procs_scanned: usize,
    /// Base unit lines that name a Phase A file but are overridden by a
    /// drop-in: removing that drop-in (a typical "rollback") revives them.
    pub overrides: Vec<Override>,
}

/// A base-unit assignment that names a Phase A file and is only kept dead by
/// a drop-in (wave-4 slot 345).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub unit: String,
    pub base_file: String,
    pub base_line: usize,
    pub section: String,
    pub key: String,
    pub target: String,
    /// Drop-in (or later fragment line) whose assignment is now effective.
    pub by_file: String,
    pub by_line: usize,
    pub by_text: String,
    /// Comment lines in the overriding drop-in that describe a rollback
    /// (file, line, redacted text). Usually "rm this drop-in ...": unsafe
    /// once the target is archived.
    pub rollback_notes: Vec<(String, usize, String)>,
}

impl Scan {
    pub fn count(&self, c: Class) -> usize {
        self.refs.iter().filter(|r| r.class == c).count()
    }
}

/// Needles for one target: the full path always; the basename too for
/// `.py`/`.pyc` files (catches `cd /opt/galion/bin && python3 pw.py`).
pub fn needles(target: &str) -> Vec<String> {
    let mut v = vec![target.to_string()];
    let base = target.rsplit('/').next().unwrap_or(target);
    if (base.ends_with(".py") || base.ends_with(".pyc")) && base != target {
        v.push(base.to_string());
    }
    v
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
}

/// True when `needle` occurs in `hay` with no path-ish character glued on
/// either side (so `/opt/galion/rust/bin/galion-desk` does not match
/// `/opt/galion/bin/galion-desk`, and `pw.pyc` does not match `pw.py`).
pub fn hit(hay: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let s = from + i;
        let e = s + needle.len();
        let before_ok = if needle.starts_with('/') {
            hay[..s]
                .chars()
                .next_back()
                .is_none_or(|c| !word(c) && c != '/')
        } else {
            hay[..s].chars().next_back().is_none_or(|c| !word(c))
        };
        let after_ok = hay[e..].chars().next().is_none_or(|c| !word(c));
        if before_ok && after_ok {
            return true;
        }
        from = s + 1;
    }
    false
}

fn matching<'a>(line: &str, targets: &'a [String]) -> Vec<&'a str> {
    targets
        .iter()
        .filter(|t| needles(t).iter().any(|n| hit(line, n)))
        .map(|t| t.as_str())
        .collect()
}

/// Never echo anything that looks like a credential.
pub fn redact(line: &str) -> String {
    let l = line.to_ascii_lowercase();
    if ["key", "token", "secret", "passw", "auth", "bearer"]
        .iter()
        .any(|w| l.contains(w))
    {
        return "[line redacted: credential-like words]".to_string();
    }
    let t = line.trim();
    if t.chars().count() > 200 {
        let s: String = t.chars().take(200).collect();
        format!("{s}...")
    } else {
        t.to_string()
    }
}

fn read_text(p: &Path) -> std::io::Result<String> {
    let b = fs::read(p)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// Logical lines with backslash continuation joined; (first line no, text).
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut cur: Option<(usize, String)> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let trimmed = raw.trim_start();
        let is_comment = trimmed.starts_with('#') || trimmed.starts_with(';');
        match cur.take() {
            Some((start, mut acc)) => {
                // systemd: comment lines inside a continuation are skipped.
                if is_comment {
                    out.push((n, raw.to_string()));
                    cur = Some((start, acc));
                    continue;
                }
                if let Some(s) = raw.strip_suffix('\\') {
                    acc.push(' ');
                    acc.push_str(s);
                    cur = Some((start, acc));
                } else {
                    acc.push(' ');
                    acc.push_str(raw);
                    out.push((start, acc));
                }
            }
            None => {
                if !is_comment {
                    if let Some(s) = raw.strip_suffix('\\') {
                        cur = Some((n, s.to_string()));
                        continue;
                    }
                }
                out.push((n, raw.to_string()));
            }
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

struct Assign {
    file: String,
    line: usize,
    section: String,
    key: String,
    value: String,
    text: String,
}

fn is_unit_name(n: &str) -> bool {
    const SUF: &[&str] = &[
        ".service",
        ".timer",
        ".socket",
        ".path",
        ".mount",
        ".automount",
        ".target",
        ".slice",
        ".scope",
        ".swap",
        ".device",
    ];
    SUF.iter().any(|s| n.ends_with(s))
}

fn dedup_dirs(root: &Path, dirs: &[&str]) -> Vec<String> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for d in dirs {
        let p = real(root, d);
        let Ok(c) = fs::canonicalize(&p) else {
            continue;
        };
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        out.push(d.to_string());
    }
    out
}

fn sorted_names(p: &Path) -> Vec<(String, fs::FileType)> {
    let mut v: Vec<(String, fs::FileType)> = match fs::read_dir(p) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let ft = e.file_type().ok()?;
                Some((e.file_name().to_string_lossy().into_owned(), ft))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn masked_link(p: &Path) -> bool {
    fs::read_link(p)
        .map(|t| t == Path::new("/dev/null"))
        .unwrap_or(false)
}

/// Scan one systemd scope (system or user dirs) with precedence, drop-ins,
/// masking and `Key=` reset semantics.
fn scan_units(root: &Path, dirs: &[&str], targets: &[String], scan: &mut Scan) {
    let dirs = dedup_dirs(root, dirs);
    // unit name -> fragments in precedence order (logical paths)
    let mut frags: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // unit name -> drop-in file name -> logical paths in precedence order
    let mut drops: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for d in &dirs {
        for (name, ft) in sorted_names(&real(root, d)) {
            let lp = format!("{d}/{name}");
            if ft.is_dir() {
                if let Some(unit) = name.strip_suffix(".d") {
                    for (cn, cft) in sorted_names(&real(root, &lp)) {
                        if cn.ends_with(".conf") && !cft.is_dir() {
                            drops
                                .entry(unit.to_string())
                                .or_default()
                                .entry(cn.clone())
                                .or_default()
                                .push(format!("{lp}/{cn}"));
                        }
                    }
                }
                continue;
            }
            if is_unit_name(&name) {
                frags.entry(name).or_default().push(lp);
            }
        }
    }
    let mut names: Vec<String> = frags.keys().cloned().collect();
    for k in drops.keys() {
        if !frags.contains_key(k) {
            names.push(k.clone());
        }
    }
    names.sort();
    for unit in names {
        scan.units_scanned += 1;
        let fl = frags.get(&unit).cloned().unwrap_or_default();
        let masked = fl.first().is_some_and(|f| masked_link(&real(root, f)));
        // Effective files in order; others are shadowed.
        let mut effective: Vec<String> = Vec::new();
        let mut shadowed: Vec<String> = Vec::new();
        if let Some((first, rest)) = fl.split_first() {
            if !masked {
                effective.push(first.clone());
            }
            shadowed.extend(rest.iter().cloned());
        }
        if let Some(dm) = drops.get(&unit) {
            for paths in dm.values() {
                if let Some((first, rest)) = paths.split_first() {
                    effective.push(first.clone());
                    shadowed.extend(rest.iter().cloned());
                }
            }
        }
        let other_class = if masked {
            Class::Masked
        } else {
            Class::Shadowed
        };
        for f in &shadowed {
            scan_plain_unit(root, f, targets, other_class, scan);
        }
        if masked {
            // effective list holds drop-ins only; a masked unit never runs.
            for f in &effective {
                scan_plain_unit(root, f, targets, Class::Masked, scan);
            }
            continue;
        }
        let mut assigns: Vec<Assign> = Vec::new();
        for f in &effective {
            let text = match read_text(&real(root, f)) {
                Ok(t) => t,
                Err(_) => {
                    scan.unreadable.push(f.clone());
                    continue;
                }
            };
            let mut section = String::new();
            for (n, l) in logical_lines(&text) {
                let t = l.trim();
                if t.is_empty() {
                    continue;
                }
                if t.starts_with('#') || t.starts_with(';') {
                    for tg in matching(t, targets) {
                        scan.refs.push(Ref {
                            kind: "unit",
                            source: f.clone(),
                            line: n,
                            target: tg.to_string(),
                            class: Class::Comment,
                            text: redact(t),
                        });
                    }
                    continue;
                }
                if t.starts_with('[') && t.ends_with(']') {
                    section = t.to_string();
                    continue;
                }
                match t.split_once('=') {
                    Some((k, v)) => assigns.push(Assign {
                        file: f.clone(),
                        line: n,
                        section: section.clone(),
                        key: k.trim().to_string(),
                        value: v.trim().to_string(),
                        text: t.to_string(),
                    }),
                    None => {
                        // Unparsable line: count any hit as live (conservative).
                        for tg in matching(t, targets) {
                            scan.refs.push(Ref {
                                kind: "unit",
                                source: f.clone(),
                                line: n,
                                target: tg.to_string(),
                                class: Class::Active,
                                text: redact(t),
                            });
                        }
                    }
                }
            }
        }
        let mut live = vec![true; assigns.len()];
        for i in 0..assigns.len() {
            let a = &assigns[i];
            let single = SINGLE_VALUE_KEYS.contains(&a.key.as_str());
            if a.value.is_empty() || single {
                for j in 0..i {
                    if assigns[j].section == a.section && assigns[j].key == a.key {
                        live[j] = false;
                    }
                }
            }
        }
        for (i, a) in assigns.iter().enumerate() {
            for tg in matching(&a.text, targets) {
                scan.refs.push(Ref {
                    kind: "unit",
                    source: a.file.clone(),
                    line: a.line,
                    target: tg.to_string(),
                    class: if live[i] {
                        Class::Active
                    } else {
                        Class::Overridden
                    },
                    text: redact(&a.text),
                });
            }
        }
        collect_overrides(root, &unit, &assigns, &live, targets, scan);
    }
}

fn scan_plain_unit(root: &Path, f: &str, targets: &[String], class: Class, scan: &mut Scan) {
    let p = real(root, f);
    if masked_link(&p) {
        return;
    }
    let text = match read_text(&p) {
        Ok(t) => t,
        Err(_) => {
            scan.unreadable.push(f.to_string());
            return;
        }
    };
    for (n, l) in logical_lines(&text) {
        let t = l.trim();
        let c = if t.starts_with('#') || t.starts_with(';') {
            Class::Comment
        } else {
            class
        };
        for tg in matching(t, targets) {
            scan.refs.push(Ref {
                kind: "unit",
                source: f.to_string(),
                line: n,
                target: tg.to_string(),
                class: c,
                text: redact(t),
            });
        }
    }
}

fn scan_cron_file(root: &Path, f: &str, targets: &[String], scan: &mut Scan) {
    let text = match read_text(&real(root, f)) {
        Ok(t) => t,
        Err(_) => {
            scan.unreadable.push(f.to_string());
            return;
        }
    };
    scan.cron_files_scanned += 1;
    for (i, l) in text.lines().enumerate() {
        let t = l.trim();
        let c = if t.starts_with('#') {
            Class::Comment
        } else {
            Class::Active
        };
        for tg in matching(t, targets) {
            scan.refs.push(Ref {
                kind: "cron",
                source: f.to_string(),
                line: i + 1,
                target: tg.to_string(),
                class: c,
                text: redact(t),
            });
        }
    }
}

fn scan_cron(root: &Path, targets: &[String], scan: &mut Scan) {
    for f in CRON_FILES {
        if real(root, f).is_file() {
            scan_cron_file(root, f, targets, scan);
        }
    }
    for d in CRON_DIRS {
        let p = real(root, d);
        if !p.is_dir() {
            continue;
        }
        if fs::read_dir(&p).is_err() {
            scan.unreadable.push(d.to_string());
            continue;
        }
        for (name, ft) in sorted_names(&p) {
            if ft.is_dir() || name.starts_with('.') {
                continue;
            }
            scan_cron_file(root, &format!("{d}/{name}"), targets, scan);
        }
    }
}

fn scan_procs(root: &Path, targets: &[String], scan: &mut Scan) {
    let me = std::process::id().to_string();
    let proc_dir = real(root, "/proc");
    for (name, ft) in sorted_names(&proc_dir) {
        if !ft.is_dir() || !name.bytes().all(|b| b.is_ascii_digit()) || name == me {
            continue;
        }
        let base = proc_dir.join(&name);
        let Ok(cmd) = fs::read(base.join("cmdline")) else {
            continue; // exited or kernel thread
        };
        scan.procs_scanned += 1;
        let cmd: String = String::from_utf8_lossy(&cmd).replace('\0', " ");
        let exe = fs::read_link(base.join("exe"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let exe = exe.trim_end_matches(" (deleted)").to_string();
        let mut hits: Vec<&str> = matching(&cmd, targets);
        for t in targets {
            if exe == *t && !hits.contains(&t.as_str()) {
                hits.push(t);
            }
        }
        for tg in hits {
            scan.refs.push(Ref {
                kind: "process",
                source: format!("/proc/{name}"),
                line: 0,
                target: tg.to_string(),
                class: Class::Active,
                text: redact(&format!("exe={exe} cmdline={cmd}")),
            });
        }
    }
}

/// Scan systemd units (system + user), timers included, cron, and `/proc`.
pub fn scan_refs(root: &Path, targets: &[String], procs: bool) -> Scan {
    let mut scan = Scan::default();
    scan_units(root, SYSTEM_UNIT_DIRS, targets, &mut scan);
    scan_units(root, USER_UNIT_DIRS, targets, &mut scan);
    scan_cron(root, targets, &mut scan);
    if procs {
        scan_procs(root, targets, &mut scan);
    }
    scan.refs.sort_by(|a, b| {
        (a.class, &a.source, a.line, &a.target).cmp(&(b.class, &b.source, b.line, &b.target))
    });
    scan
}

/// Comment lines mentioning a rollback in one unit file (redacted).
fn rollback_comments(root: &Path, f: &str) -> Vec<(String, usize, String)> {
    let Ok(text) = read_text(&real(root, f)) else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            (t.starts_with('#') || t.starts_with(';'))
                && t.to_ascii_lowercase().contains("rollback")
        })
        .map(|(i, l)| (f.to_string(), i + 1, redact(l)))
        .collect()
}

/// Record every overridden assignment that names a target, with the line that
/// overrides it and any rollback note in that drop-in.
fn collect_overrides(
    root: &Path,
    unit: &str,
    assigns: &[Assign],
    live: &[bool],
    targets: &[String],
    scan: &mut Scan,
) {
    for (j, a) in assigns.iter().enumerate() {
        if live[j] {
            continue;
        }
        let hits = matching(&a.text, targets);
        if hits.is_empty() {
            continue;
        }
        // Effective replacement: the last live non-empty assignment of the
        // same key after j; else the reset line itself (`Key=`).
        let same = |k: &Assign| k.section == a.section && k.key == a.key;
        let by = assigns
            .iter()
            .enumerate()
            .skip(j + 1)
            .rfind(|(i, k)| same(k) && live[*i] && !k.value.is_empty())
            .or_else(|| {
                assigns
                    .iter()
                    .enumerate()
                    .skip(j + 1)
                    .rfind(|(_, k)| same(k))
            });
        let Some((_, b)) = by else { continue };
        let notes = if b.file != a.file {
            rollback_comments(root, &b.file)
        } else {
            Vec::new()
        };
        for tg in hits {
            scan.overrides.push(Override {
                unit: unit.to_string(),
                base_file: a.file.clone(),
                base_line: a.line,
                section: a.section.clone(),
                key: a.key.clone(),
                target: tg.to_string(),
                by_file: b.file.clone(),
                by_line: b.line,
                by_text: redact(&b.text),
                rollback_notes: notes.clone(),
            });
        }
    }
}

/// Logical path -> file name inside [`ARCHIVE_DIR`] used by the Phase A `mv`.
pub fn archived_name(path: &str) -> String {
    format!(
        "{ARCHIVE_DIR}/{}",
        path.trim_start_matches('/').replace('/', "__")
    )
}

/// Phase A paths a target needs at run time (its own path first).
pub fn with_needs(target: &str) -> Vec<String> {
    let mut v = vec![target.to_string()];
    if let Some(it) = PHASE_A.iter().find(|i| i.path == target) {
        for n in it.needs {
            if !v.iter().any(|x| x == n) {
                v.push((*n).to_string());
            }
        }
    }
    v
}

/// Exact safe rollback for one overridden base unit, AFTER Phase A ran.
/// Text only, never executed: restore the archived target (and what it
/// needs), prove the bytes, and only then remove the overriding drop-in.
pub fn safe_rollback(o: &Override, manifest_path: &str) -> Vec<String> {
    let files = with_needs(&o.target);
    let mut v = Vec::new();
    v.push(format!(
        "# {}: base {}:{} {}= names archived {}; kept dead only by {}:{}",
        o.unit, o.base_file, o.base_line, o.key, o.target, o.by_file, o.by_line
    ));
    for (f, l, _) in &o.rollback_notes {
        v.push(format!(
            "# STALE rollback note {f}:{l} (\"rm this drop-in\") is UNSAFE after Phase A: do steps 1-3 first"
        ));
    }
    v.push("# 1. restore the archived file(s) to their exact paths (mode kept by mv):".into());
    for f in &files {
        v.push(format!("mv -n {} {f}", archived_name(f)));
    }
    v.push("# 2. prove the bytes before systemd can start them:".into());
    let pat = files
        .iter()
        .map(|f| format!("  {}$", f.replace('.', "\\.")))
        .collect::<Vec<_>>()
        .join("|");
    v.push(format!(
        "grep -E '{pat}' {manifest_path} | sha256sum -c -   # expect {} OK",
        files.len()
    ));
    v.push(format!(
        "test -x {}   # base ExecStart must exist and be executable",
        o.target
    ));
    v.push(format!(
        "# 3. only now the unit rollback: remove {} (unit-edit approval), then:",
        o.by_file
    ));
    v.push("systemctl daemon-reload".into());
    v.push(format!(
        "systemctl show -p ExecStart {}   # must show {}",
        o.unit, o.target
    ));
    v.push(format!(
        "systemctl restart {} && systemctl is-active {}",
        o.unit, o.unit
    ));
    v.push(format!(
        "# Never remove {} while {} is archived: {} would fail to start (Restart= loop).",
        o.by_file, o.target, o.unit
    ));
    v
}

/// `sha256sum -c` compatible manifest (two spaces, absolute paths).
pub fn manifest(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|e| format!("{}  {}\n", e.sha256, e.path))
        .collect()
}

/// Parse a manifest written by [`manifest`]. Skips blank and `#` lines.
pub fn parse_manifest(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut v = Vec::new();
    for (i, l) in text.lines().enumerate() {
        let t = l.trim_end();
        if t.trim().is_empty() || t.starts_with('#') {
            continue;
        }
        let (h, p) = t
            .split_once("  ")
            .ok_or_else(|| format!("line {}: expected '<sha256>  <path>'", i + 1))?;
        if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("line {}: bad sha256", i + 1));
        }
        if !p.starts_with('/') {
            return Err(format!("line {}: path must be absolute", i + 1));
        }
        v.push((h.to_ascii_lowercase(), p.to_string()));
    }
    if v.is_empty() {
        return Err("manifest has no entries".into());
    }
    Ok(v)
}

/// The commands the owner would run for Phase A. Text only: nothing here is
/// executed by this crate.
pub fn proposed_commands(entries: &[Entry], overrides: &[Override]) -> String {
    let tar = format!("{ARCHIVE_DIR}/phase-a.tar");
    let mut s = String::new();
    s.push_str(&format!(
        "# NOT EXECUTED. Phase A, earliest {EARLIEST}, needs owner (Slavik) OK.\n"
    ));
    s.push_str("# 0. re-run: galion-pyarchive verify --manifest phase-a.sha256  (must exit 0)\n");
    s.push_str(&format!("install -d -m 0700 {ARCHIVE_DIR}\n"));
    s.push_str(&format!("cp phase-a.sha256 phase-a.files {ARCHIVE_DIR}/\n"));
    s.push_str(&format!(
        "tar -cPf {tar} --files-from {ARCHIVE_DIR}/phase-a.files\n"
    ));
    s.push_str(&format!(
        "tar -tvPf {tar}   # expect {} entries\n",
        entries.len()
    ));
    s.push_str(&format!("sha256sum {tar} > {tar}.sha256\n"));
    s.push_str(&format!(
        "sha256sum -c {ARCHIVE_DIR}/phase-a.sha256   # originals still match\n"
    ));
    s.push_str("# then move each original into the archive dir:\n");
    for e in entries {
        s.push_str(&format!("mv -n {} {}\n", e.path, archived_name(&e.path)));
    }
    s.push_str("# watch nerve /state 15 min; galion-desk must stay active.\n");
    s.push_str("\n# ROLLBACK of Phase A (file level, exact; restores bytes and mode):\n");
    for e in entries {
        s.push_str(&format!("mv -n {} {}\n", archived_name(&e.path), e.path));
    }
    s.push_str(&format!(
        "sha256sum -c {ARCHIVE_DIR}/phase-a.sha256   # all OK (or: tar -xPf {tar} then the same check)\n"
    ));
    if !overrides.is_empty() {
        s.push_str(&format!(
            "\n# OVERRIDDEN-BASE UNITS ({}): a unit rollback that removes the drop-in revives the base line.\n# Safe order per unit (unit-level rollback AFTER Phase A):\n",
            overrides.len()
        ));
        let man = format!("{ARCHIVE_DIR}/phase-a.sha256");
        for o in overrides {
            for l in safe_rollback(o, &man) {
                s.push_str(&l);
                s.push('\n');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_respects_path_boundaries() {
        let t = "/opt/galion/bin/galion-desk";
        assert!(hit("ExecStart=/opt/galion/bin/galion-desk", t));
        assert!(hit("ExecStart=/opt/galion/bin/galion-desk --x", t));
        assert!(!hit("ExecStart=/opt/galion/rust/bin/galion-desk", t));
        assert!(!hit("ExecStart=/opt/galion/bin/galion-desk-v2", t));
        assert!(!hit("/srv/x/opt/galion/bin/galion-desk", t));
        assert!(hit("python3 pw.py", "pw.py"));
        assert!(!hit("python3 pw.pyc", "pw.py"));
        assert!(!hit("python3 xpw.py", "pw.py"));
        assert!(hit("\"/opt/galion/bin/pw.py\"", "/opt/galion/bin/pw.py"));
    }

    #[test]
    fn needles_add_basename_only_for_python() {
        assert_eq!(needles("/opt/galion/bin/galion-desk").len(), 1);
        assert_eq!(
            needles("/opt/galion/bin/pw.py"),
            vec!["/opt/galion/bin/pw.py".to_string(), "pw.py".to_string()]
        );
    }

    #[test]
    fn sha256_known_vectors() {
        let d = std::env::temp_dir().join(format!("pyarchive-unit-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let p = d.join("abc");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let e = d.join("empty");
        fs::write(&e, b"").unwrap();
        assert_eq!(
            sha256_file(&e).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn manifest_roundtrip_and_rejects() {
        let e = Entry {
            path: "/opt/galion/bin/pw.py".into(),
            size: 3,
            mtime: 0,
            mode: 0o755,
            sha256: "a".repeat(64),
        };
        let m = manifest(&[e]);
        let p = parse_manifest(&m).unwrap();
        assert_eq!(p, vec![("a".repeat(64), "/opt/galion/bin/pw.py".into())]);
        assert!(parse_manifest("").is_err());
        assert!(parse_manifest("zz  /x").is_err());
        assert!(parse_manifest(&format!("{}  rel/path", "a".repeat(64))).is_err());
    }

    #[test]
    fn redact_hides_credential_lines() {
        assert!(redact("Environment=API_TOKEN=x /opt/galion/bin/pw.py").contains("redacted"));
        assert_eq!(redact("  ExecStart=/a  "), "ExecStart=/a");
    }

    #[test]
    fn continuation_lines_join() {
        let l = logical_lines("ExecStart=/bin/a \\\n  /opt/galion/bin/pw.py\nX=1\n");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].0, 1);
        assert!(l[0].1.contains("/opt/galion/bin/pw.py"));
    }

    /// Guard: this crate must stay read-only on the scanned tree.
    #[test]
    fn source_has_no_mutating_fs_calls() {
        let src = [include_str!("lib.rs"), include_str!("main.rs")].concat();
        let banned = [
            ["remove", "_file("].concat(),
            ["remove", "_dir("].concat(),
            ["remove", "_dir_all("].concat(),
            ["fs::", "rename("].concat(),
            ["set_", "permissions("].concat(),
            ["Command", "::new("].concat(),
            ["hard_", "link("].concat(),
            ["sym", "link("].concat(),
        ];
        for b in banned {
            assert!(!src.contains(&b), "found banned call {b}");
        }
    }
}
