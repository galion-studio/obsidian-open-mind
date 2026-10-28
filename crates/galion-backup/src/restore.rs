// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! `galion-backup restore-test [ARCHIVE]`: Rust port of the VPS
//! `/opt/galion/rust/bin/galion-restore-test.sh` (sha256 26439775...0f14,
//! read-only capture 2026-10-28 CET +01:00, wave-4 slot 326). Restores the
//! newest (or given) backup into a temp dir and verifies it; touches nothing
//! live. Step for step:
//!
//! | .sh                                              | here                                  |
//! |--------------------------------------------------|---------------------------------------|
//! | `umask 077`                                      | `libc::umask(0o077)` (main)           |
//! | `DEST=${GALION_BACKUP_DEST:-/var/backups/galion}`| same env, same default                |
//! | `A=${1:-$(ls -1t $DEST/galion-*.tar.gz\|head -1)}`| `backup::archives` + `newest_first`  |
//! | `mktemp -d /tmp/galion-restore-XXXXXX` + trap    | [`TempDir`] (0700, removed on drop; SIGTERM/INT/HUP caught) |
//! | `tar -C $T -xzf $A`                              | [`crate::gunzip`] + `galion_tar::read` + [`extract`] |
//! | `cd $T/galion-*`                                 | exactly one `galion-*` dir            |
//! | `sha256sum --quiet -c SHA256SUMS`                | [`check_sums`] (same stdout lines; strict, hard fail) |
//! | `galion-backup verify db/*.db` (old helper ELF)  | in-process `sqlite::verify`           |
//! | `tar -C restored -xf files/files.tar`            | [`extract`]                           |
//! | `find restored -type f \| wc -l`                 | [`count_files`]                       |
//! | `[ -e restored/$p ]` presence list               | `Path::exists` (follows symlinks)     |
//!
//! Stdout is line-for-line what the .sh prints (tests/real_restore_test.rs
//! diffs both on the same archives). Differences, all deliberate: restored
//! copies keep their modes (without setuid/setgid/sticky) but not owners or
//! mtimes (nothing is kept; the 0700 temp dir is removed); member names with
//! `..` or that would be written through a symlink are refused instead of
//! GNU tar's strip-and-warn; device nodes are skipped (the backup never
//! contains any). Errors go to stderr as `galion-backup: restore-test: ...`
//! with exit 1 (bash `set -e` exit codes varied by the failing tool).
//!
//! Checksums are strict (slot 361, fixes F-326-1 where the .sh printed
//! `FAILED` and still ended `restore test ok`, exit 0): ANY mismatch, missing
//! or unreadable listed file, malformed SHA256SUMS line, listed name that is
//! absolute / has `..` / is or goes through a symlink, or archive file not
//! covered by SHA256SUMS stops the test with exit 1 and one
//! `ARCHIVE: checksum check FAILED: ...; this backup is NOT verified` line.
//! gzip CRC32/ISIZE and tar header checksums are verified while reading too.

use crate::backup;
use crate::gunzip::GzReader;
use crate::sha256::{hex, Sha256};
use crate::sqlite;
use galion_tar::read::{MemberKind, Reader};
use std::ffi::{CString, OsStr};
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

/// The .sh's presence list, in order.
pub const PRESENT: &[&str] = &[
    "var/lib/memory-tencentdb/persona.md",
    "var/lib/hermes-rs",
    "root/galion-secrets/admin-device.json",
    "etc/openmind-memory.env",
];

pub const DEFAULT_TMP: &str = "/tmp";

#[derive(Debug, Clone)]
pub struct Opts {
    pub dest: PathBuf,
    pub archive: Option<PathBuf>,
    /// Parent of the temp dir (`/tmp` like the .sh; `GALION_RESTORE_TMP`, Rust only).
    pub tmp: PathBuf,
}

fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

impl Opts {
    pub fn from_env(archive: Option<&str>) -> Opts {
        Opts {
            dest: PathBuf::from(env_or("GALION_BACKUP_DEST", backup::DEFAULT_DEST)),
            // ${1:-...}: an empty argument means "newest".
            archive: archive.filter(|a| !a.is_empty()).map(PathBuf::from),
            tmp: PathBuf::from(env_or("GALION_RESTORE_TMP", DEFAULT_TMP)),
        }
    }
}

/// Error type: message, or the signal number that interrupted us.
#[derive(Debug)]
pub enum Fail {
    Msg(String),
    Signal(i32),
}

impl From<String> for Fail {
    fn from(s: String) -> Self {
        Fail::Msg(s)
    }
}

fn ctx<E: std::fmt::Display>(c: impl std::fmt::Display) -> impl FnOnce(E) -> Fail {
    move |e| Fail::Msg(format!("{c}: {e}"))
}

fn check_signal() -> Result<(), Fail> {
    match backup::pending_signal() {
        Some(s) => Err(Fail::Signal(s)),
        None => Ok(()),
    }
}

/// `mktemp -d PARENT/galion-restore-XXXXXX` (0700), removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn create(parent: &Path) -> io::Result<TempDir> {
        for _ in 0..100 {
            let p = parent.join(format!("galion-restore-{}", backup::random_suffix()));
            match DirBuilder::new().mode(0o700).create(&p) {
                Ok(()) => return Ok(TempDir(p)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("mktemp: too many collisions"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.0).is_err() {
            // Read-only dirs from the archive (non-root runs): make them writable, retry.
            fn open_up(d: &Path) {
                let _ = fs::set_permissions(d, fs::Permissions::from_mode(0o700));
                if let Ok(rd) = fs::read_dir(d) {
                    for e in rd.flatten() {
                        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                            open_up(&e.path());
                        }
                    }
                }
            }
            open_up(&self.0);
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

/// Member name -> safe relative path: leading `/` and `.` components are
/// dropped (GNU tar strips leading `/` too); `..` is refused. None = the
/// archive root itself (`./`).
pub fn safe_rel(name: &[u8]) -> Result<Option<PathBuf>, String> {
    let mut out = PathBuf::new();
    for c in Path::new(OsStr::from_bytes(name)).components() {
        match c {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(s) => out.push(s),
            Component::ParentDir | Component::Prefix(_) => {
                return Err(format!(
                    "refusing member with '..': {}",
                    String::from_utf8_lossy(name)
                ))
            }
        }
    }
    Ok((!out.as_os_str().is_empty()).then_some(out))
}

/// Create the parents of `root/rel` as needed, refusing to go through a
/// symlink (a member like `a -> /etc` followed by `a/x` must not escape).
fn parent_dirs(root: &Path, rel: &Path) -> Result<(), Fail> {
    let mut p = root.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for c in &comps[..comps.len().saturating_sub(1)] {
        p.push(c);
        match fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(Fail::Msg(format!(
                    "refusing to extract through symlink {}",
                    rel.display()
                )))
            }
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(Fail::Msg(format!("not a directory: {}", p.display()))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                DirBuilder::new()
                    .mode(0o700)
                    .create(&p)
                    .map_err(ctx(p.display()))?;
            }
            Err(e) => return Err(ctx(p.display())(e)),
        }
    }
    Ok(())
}

/// Remove whatever is at `p` (not following symlinks) so it can be replaced,
/// like GNU tar's default unlink-before-extract. Directories are kept.
fn clear(p: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => Ok(true),
        Ok(_) => fs::remove_file(p).map(|_| false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Counts of what [`extract`] created.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub hardlinks: u64,
    pub fifos: u64,
    pub devices_skipped: u64,
    pub bytes: u64,
}

/// `tar -C root -x`: extract every member of a tar stream under `root`.
pub fn extract<R: Read>(r: R, root: &Path) -> Result<Extracted, Fail> {
    let mut t = Reader::new(r);
    let mut x = Extracted::default();
    let mut dir_modes: Vec<(PathBuf, u32)> = Vec::new();
    while let Some(m) = t.next_member().map_err(ctx("tar"))? {
        check_signal()?;
        let Some(rel) = safe_rel(&m.name)? else {
            continue;
        };
        parent_dirs(root, &rel)?;
        let p = root.join(&rel);
        let mode = m.mode & 0o777;
        match m.kind {
            MemberKind::Dir => {
                if !clear(&p).map_err(ctx(p.display()))? {
                    DirBuilder::new()
                        .mode(0o700)
                        .create(&p)
                        .map_err(ctx(p.display()))?;
                }
                dir_modes.push((p, mode));
                x.dirs += 1;
            }
            MemberKind::File => {
                clear(&p).map_err(ctx(p.display()))?;
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&p)
                    .map_err(ctx(p.display()))?;
                x.bytes += t.copy_data(&mut f).map_err(ctx(p.display()))?;
                f.set_permissions(fs::Permissions::from_mode(mode))
                    .map_err(ctx(p.display()))?;
                x.files += 1;
            }
            MemberKind::Hardlink => {
                let Some(trel) = safe_rel(&m.link)? else {
                    return Err(Fail::Msg(format!(
                        "hard link to archive root: {}",
                        m.name_lossy()
                    )));
                };
                let target = root.join(&trel);
                // The target must be a regular file this extraction made, not reached via a symlink.
                parent_dirs(root, &trel)?;
                match fs::symlink_metadata(&target) {
                    Ok(tm) if tm.is_file() => {}
                    _ => {
                        return Err(Fail::Msg(format!(
                            "hard link target missing or not a file: {} -> {}",
                            m.name_lossy(),
                            String::from_utf8_lossy(&m.link)
                        )))
                    }
                }
                clear(&p).map_err(ctx(p.display()))?;
                fs::hard_link(&target, &p).map_err(ctx(p.display()))?;
                x.hardlinks += 1;
            }
            MemberKind::Symlink => {
                clear(&p).map_err(ctx(p.display()))?;
                std::os::unix::fs::symlink(OsStr::from_bytes(&m.link), &p)
                    .map_err(ctx(p.display()))?;
                x.symlinks += 1;
            }
            MemberKind::Fifo => {
                clear(&p).map_err(ctx(p.display()))?;
                let c = CString::new(p.as_os_str().as_bytes())
                    .map_err(|_| Fail::Msg("NUL in path".into()))?;
                // SAFETY: c is a valid NUL-terminated path.
                if unsafe { libc::mkfifo(c.as_ptr(), mode as libc::mode_t) } != 0 {
                    return Err(ctx(p.display())(io::Error::last_os_error()));
                }
                x.fifos += 1;
            }
            MemberKind::Char | MemberKind::Block => {
                x.devices_skipped += 1;
            }
        }
    }
    // Directory modes last (deepest first), so read-only dirs don't block their children.
    for (p, mode) in dir_modes.into_iter().rev() {
        fs::set_permissions(&p, fs::Permissions::from_mode(mode)).map_err(ctx(p.display()))?;
    }
    Ok(x)
}

/// `find DIR -type f | wc -l` (no symlink following; hard links count each name).
pub fn count_files(dir: &Path) -> io::Result<u64> {
    let mut n = 0;
    for e in fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        if ft.is_dir() {
            n += count_files(&e.path())?;
        } else if ft.is_file() {
            n += 1;
        }
    }
    Ok(n)
}

fn sha256_file(p: &Path) -> io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    let mut b = vec![0u8; 256 * 1024];
    loop {
        let n = match f.read(&mut b) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        h.update(&b[..n]);
    }
    Ok(hex(&h.finish()))
}

/// One parsed `sha256sum` line: (hex digest lowercased, file name bytes).
fn parse_sum_line(line: &[u8]) -> Option<(String, Vec<u8>)> {
    let (escaped, l) = match line.first() {
        Some(b'\\') => (true, &line[1..]),
        _ => (false, line),
    };
    if l.len() < 66 || !l[..64].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    if l[64] != b' ' || !(l[65] == b' ' || l[65] == b'*') {
        return None;
    }
    let raw = &l[66..];
    if raw.is_empty() {
        return None;
    }
    let name = if escaped {
        let mut o = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'\\' {
                match raw.get(i + 1) {
                    Some(b'n') => o.push(b'\n'),
                    Some(b'r') => o.push(b'\r'),
                    Some(b'\\') => o.push(b'\\'),
                    _ => return None,
                }
                i += 2;
            } else {
                o.push(raw[i]);
                i += 1;
            }
        }
        o
    } else {
        raw.to_vec()
    };
    Some((String::from_utf8_lossy(&l[..64]).to_ascii_lowercase(), name))
}

/// What [`check_sums`] found. Any non-empty problem list is a hard failure
/// (F-326-1 / slot 361): the restore test must never say `ok` for an archive
/// whose contents cannot all be proven by SHA256SUMS.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SumReport {
    /// Listed files whose digest matched.
    pub ok: u64,
    /// Listed files whose digest did NOT match (`NAME: FAILED`).
    pub mismatched: Vec<String>,
    /// Listed files that are missing or unreadable (`NAME: FAILED open or read`).
    pub unreadable: Vec<String>,
    /// SHA256SUMS lines that are not `<64 hex><sp><sp|*><name>` (sha256sum --strict).
    pub malformed: u64,
    /// Listed names that are absolute, contain `..`, or are symlinks: they
    /// would hash something outside the archive.
    pub unsafe_names: Vec<String>,
    /// Regular files in the archive that SHA256SUMS does not list, and
    /// entries that are neither a regular file nor a directory (symlinks,
    /// fifos): unprovable content.
    pub unlisted: Vec<String>,
}

impl SumReport {
    pub fn problems(&self) -> usize {
        self.mismatched.len()
            + self.unreadable.len()
            + self.malformed as usize
            + self.unsafe_names.len()
            + self.unlisted.len()
    }

    /// One clear line for stderr / the journal.
    pub fn summary(&self) -> String {
        fn names(v: &[String]) -> String {
            if v.is_empty() {
                return String::new();
            }
            let mut s: Vec<&str> = v.iter().take(5).map(|x| x.as_str()).collect();
            let more = v.len().saturating_sub(5);
            let tail = if more > 0 {
                format!(" +{more} more")
            } else {
                String::new()
            };
            s.sort();
            format!(" [{}{tail}]", s.join(", "))
        }
        let listed = self.ok
            + (self.mismatched.len() + self.unreadable.len() + self.unsafe_names.len()) as u64;
        format!(
            "{} mismatched{}, {} unreadable{}, {} malformed line(s), {} unsafe name(s){}, {} unlisted{} ({} of {listed} listed files ok)",
            self.mismatched.len(),
            names(&self.mismatched),
            self.unreadable.len(),
            names(&self.unreadable),
            self.malformed,
            self.unsafe_names.len(),
            names(&self.unsafe_names),
            self.unlisted.len(),
            names(&self.unlisted),
            self.ok
        )
    }
}

/// `./a/b` / `a/b` -> `a/b`; None if absolute or containing `..`.
fn listed_rel(name: &[u8]) -> Option<PathBuf> {
    if name.first() == Some(&b'/') {
        return None;
    }
    match safe_rel(name) {
        Ok(Some(p)) => Some(p),
        _ => None,
    }
}

/// Every non-directory entry under `dir` (no symlink following), as
/// relative paths, with whether it is a regular file.
fn walk(dir: &Path, rel: &Path, acc: &mut Vec<(PathBuf, bool)>) -> io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        let r = rel.join(e.file_name());
        if ft.is_dir() {
            walk(&e.path(), &r, acc)?;
        } else {
            acc.push((r, ft.is_file()));
        }
    }
    Ok(())
}

/// `sha256sum --quiet --strict -c SHA256SUMS` run in `dir`, plus a
/// completeness check. Failure lines for mismatched / unreadable files go to
/// `out` exactly as coreutils prints them (`NAME: FAILED`, `NAME: FAILED
/// open or read`); everything else to stderr. Unlike the live .sh
/// (F-326-1), ANY problem is an Err: a mismatch, a missing/unreadable file,
/// a malformed line, a name that points outside the archive or at a symlink,
/// or a file in the archive that SHA256SUMS does not cover.
pub fn check_sums(dir: &Path, out: &mut dyn Write) -> Result<SumReport, Fail> {
    let list = dir.join("SHA256SUMS");
    match fs::symlink_metadata(&list) {
        Ok(m) if m.is_file() => {}
        Ok(_) => {
            return Err(Fail::Msg(
                "checksum check FAILED: SHA256SUMS is not a regular file".into(),
            ))
        }
        Err(e) => return Err(ctx("checksum check FAILED: SHA256SUMS")(e)),
    }
    let f = File::open(&list).map_err(ctx("checksum check FAILED: SHA256SUMS"))?;
    let mut r = SumReport::default();
    let mut listed = std::collections::BTreeSet::new();
    let mut parsed = 0u64;
    for line in BufReader::new(f).split(b'\n') {
        check_signal()?;
        let mut line = line.map_err(ctx("checksum check FAILED: SHA256SUMS"))?;
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let Some((want, name)) = parse_sum_line(&line) else {
            r.malformed += 1;
            continue;
        };
        parsed += 1;
        let shown = String::from_utf8_lossy(&name).into_owned();
        let Some(rel) = listed_rel(&name) else {
            eprintln!(
                "galion-backup: restore-test: {shown}: listed name points outside the archive"
            );
            r.unsafe_names.push(shown);
            continue;
        };
        let p = dir.join(&rel);
        // Never follow a symlink (in the name's parents or at the end).
        let mut through_link = false;
        let mut q = dir.to_path_buf();
        for c in rel.components() {
            q.push(c);
            if fs::symlink_metadata(&q)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                through_link = true;
                break;
            }
        }
        if through_link {
            eprintln!(
                "galion-backup: restore-test: {shown}: listed name is or goes through a symlink"
            );
            r.unsafe_names.push(shown);
            continue;
        }
        listed.insert(rel);
        match sha256_file(&p) {
            Ok(got) if got == want => r.ok += 1,
            Ok(_) => {
                writeln!(out, "{shown}: FAILED").ok();
                r.mismatched.push(shown);
            }
            Err(e) => {
                eprintln!("sha256sum: {shown}: {e}");
                writeln!(out, "{shown}: FAILED open or read").ok();
                r.unreadable.push(shown);
            }
        }
    }
    out.flush().ok();
    let mut all = Vec::new();
    walk(dir, Path::new(""), &mut all).map_err(ctx("checksum check FAILED: walk archive"))?;
    all.sort();
    for (rel, regular) in all {
        if rel.as_os_str() == "SHA256SUMS" {
            continue;
        }
        let shown = format!("./{}", rel.display());
        if !regular {
            eprintln!("galion-backup: restore-test: {shown}: not a regular file (unprovable)");
            r.unlisted.push(shown);
        } else if !listed.contains(&rel) {
            eprintln!("galion-backup: restore-test: {shown}: not listed in SHA256SUMS");
            r.unlisted.push(shown);
        }
    }
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            one.to_string()
        } else {
            many.to_string()
        }
    };
    if r.malformed > 0 {
        eprintln!(
            "sha256sum: WARNING: {} {} improperly formatted",
            r.malformed,
            plural(r.malformed as usize, "line is", "lines are")
        );
    }
    if parsed == 0 {
        return Err(Fail::Msg(
            "checksum check FAILED: SHA256SUMS: no properly formatted checksum lines found".into(),
        ));
    }
    if !r.unreadable.is_empty() {
        eprintln!(
            "sha256sum: WARNING: {} listed {} could not be read",
            r.unreadable.len(),
            plural(r.unreadable.len(), "file", "files")
        );
    }
    if !r.mismatched.is_empty() {
        eprintln!(
            "sha256sum: WARNING: {} computed {} did NOT match",
            r.mismatched.len(),
            plural(r.mismatched.len(), "checksum", "checksums")
        );
    }
    if r.problems() > 0 {
        return Err(Fail::Msg(format!(
            "checksum check FAILED: {}; this backup is NOT verified, do not restore from it",
            r.summary()
        )));
    }
    Ok(r)
}

/// `$DEST/galion-*.tar.gz`, newest first; Err if none (the .sh's `ls` failed).
pub fn newest_archive(dest: &Path) -> Result<PathBuf, Fail> {
    backup::newest_first(backup::archives(dest))
        .into_iter()
        .next()
        .ok_or_else(|| Fail::Msg(format!("no {}/galion-*.tar.gz to test", dest.display())))
}

fn line(out: &mut dyn Write, s: &str) -> Result<(), Fail> {
    writeln!(out, "{s}")
        .and_then(|_| out.flush())
        .map_err(ctx("stdout"))
}

/// The whole restore test. Lines go to `out` (stdout in production).
pub fn run(o: &Opts, out: &mut dyn Write) -> Result<(), Fail> {
    let a = match &o.archive {
        Some(a) => a.clone(),
        None => newest_archive(&o.dest)?,
    };
    let t = TempDir::create(&o.tmp).map_err(ctx(format!("mktemp in {}", o.tmp.display())))?;
    check_signal()?;
    let f = File::open(&a).map_err(ctx(a.display()))?;
    let mut gz = GzReader::new(BufReader::with_capacity(256 * 1024, f));
    extract(&mut gz, &t.0).map_err(|e| match e {
        Fail::Msg(m) => Fail::Msg(format!("{}: {m}", a.display())),
        s => s,
    })?;
    // The tar reader stops at the end-of-archive blocks; read the rest of the
    // gzip stream so its CRC32 / ISIZE trailer is ALWAYS verified (slot 361:
    // a flipped trailer bit used to pass). `tar -xzf` does the same via gzip.
    io::copy(&mut gz, &mut io::sink()).map_err(|e| {
        Fail::Msg(format!(
            "{}: gzip check FAILED: {e}; this backup is NOT verified, do not restore from it",
            a.display()
        ))
    })?;
    // cd "$T"/galion-*: exactly one directory.
    let mut cands: Vec<PathBuf> = fs::read_dir(&t.0)
        .map_err(ctx(t.0.display()))?
        .flatten()
        .filter(|e| e.file_name().as_bytes().starts_with(b"galion-"))
        .map(|e| e.path())
        .collect();
    cands.sort();
    let s = match cands.as_slice() {
        [one]
            if one.is_dir()
                && !fs::symlink_metadata(one)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(true) =>
        {
            one.clone()
        }
        [] => return Err(Fail::Msg("archive has no galion-* directory".into())),
        [_] => {
            return Err(Fail::Msg(
                "galion-* in the archive is not a directory".into(),
            ))
        }
        _ => {
            return Err(Fail::Msg(format!(
                "archive has {} galion-* entries, expected 1",
                cands.len()
            )))
        }
    };
    check_sums(&s, out).map_err(|e| match e {
        Fail::Msg(m) => Fail::Msg(format!("{}: {m}", a.display())),
        s => s,
    })?;
    line(out, "checksums ok")?;
    // for d in db/*.db (glob: no dotfiles, byte order)
    let mut dbs: Vec<Vec<u8>> = fs::read_dir(s.join("db"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().as_bytes().to_vec())
                .filter(|n| n.len() > 3 && n.ends_with(b".db") && n[0] != b'.')
                .collect()
        })
        .unwrap_or_default();
    dbs.sort();
    if dbs.is_empty() {
        return Err(Fail::Msg("verify: no db/*.db in the archive".into()));
    }
    for n in dbs {
        check_signal()?;
        let rel = format!("db/{}", String::from_utf8_lossy(&n));
        let (r, tables) =
            sqlite::verify(&s.join(OsStr::from_bytes(b"db")).join(OsStr::from_bytes(&n)))
                .map_err(|e| Fail::Msg(format!("verify {rel}: {e}")))?;
        line(out, &format!("verify {rel} integrity={r} tables={tables}"))?;
        if r != "ok" {
            return Err(Fail::Msg(format!("verify {rel}: integrity_check = {r}")));
        }
    }
    let restored = s.join("restored");
    fs::create_dir_all(&restored).map_err(ctx(restored.display()))?;
    let ft = s.join("files/files.tar");
    let tf = File::open(&ft).map_err(ctx("files/files.tar"))?;
    extract(BufReader::with_capacity(256 * 1024, tf), &restored).map_err(|e| match e {
        Fail::Msg(m) => Fail::Msg(format!("files/files.tar: {m}")),
        s => s,
    })?;
    let n = count_files(&restored).map_err(ctx("count restored"))?;
    line(out, &format!("files restored: {n}"))?;
    for p in PRESENT {
        if restored.join(p).exists() {
            line(out, &format!("present: /{p}"))?;
        } else {
            line(out, &format!("MISSING: /{p}"))?;
        }
    }
    let base = a
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_else(|| a.display().to_string());
    line(out, &format!("restore test ok: {base}"))?;
    drop(t);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_rel_rules() {
        assert_eq!(safe_rel(b"a/b").unwrap(), Some(PathBuf::from("a/b")));
        assert_eq!(safe_rel(b"/etc/x").unwrap(), Some(PathBuf::from("etc/x")));
        assert_eq!(safe_rel(b"./a//./b/").unwrap(), Some(PathBuf::from("a/b")));
        assert_eq!(safe_rel(b"./").unwrap(), None);
        assert!(safe_rel(b"../x").is_err());
        assert!(safe_rel(b"a/../../x").is_err());
    }

    #[test]
    fn sum_lines() {
        let h = "a".repeat(64);
        assert_eq!(
            parse_sum_line(format!("{h}  ./db/x.db").as_bytes()),
            Some((h.clone(), b"./db/x.db".to_vec()))
        );
        assert_eq!(
            parse_sum_line(format!("{h} *bin").as_bytes()),
            Some((h.clone(), b"bin".to_vec()))
        );
        assert_eq!(
            parse_sum_line(format!("\\{h}  a\\nb\\\\c").as_bytes()),
            Some((h.clone(), b"a\nb\\c".to_vec()))
        );
        assert_eq!(
            parse_sum_line(format!("{}  x", "A".repeat(64)).as_bytes())
                .unwrap()
                .0,
            h
        );
        assert_eq!(parse_sum_line(b""), None);
        assert_eq!(parse_sum_line(format!("{h} x").as_bytes()), None);
        assert_eq!(
            parse_sum_line(format!("{}  x", "g".repeat(64)).as_bytes()),
            None
        );
        assert_eq!(parse_sum_line(format!("{h}  ").as_bytes()), None);
    }
}

#[cfg(test)]
mod sum_tests {
    use super::*;

    struct D(PathBuf);
    impl Drop for D {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sumline(data: &[u8], name: &str) -> String {
        let mut h = Sha256::new();
        h.update(data);
        format!("{}  {name}\n", hex(&h.finish()))
    }

    /// A tiny galion-* dir: db/a.db, MAP, files/files.tar, all listed.
    fn good(tag: &str) -> (D, String) {
        let d = std::env::temp_dir().join(format!(
            "gb361-{tag}-{}-{}",
            std::process::id(),
            backup::random_suffix()
        ));
        fs::create_dir_all(d.join("db")).unwrap();
        fs::create_dir_all(d.join("files")).unwrap();
        let files: [(&str, &[u8]); 3] = [
            ("db/a.db", b"sqlite-ish"),
            ("db/MAP", b"a.db /var/lib/a.db\n"),
            ("files/files.tar", b"tarbytes"),
        ];
        let mut sums = String::new();
        for (n, b) in files {
            fs::write(d.join(n), b).unwrap();
            sums.push_str(&sumline(b, &format!("./{n}")));
        }
        fs::write(d.join("SHA256SUMS"), &sums).unwrap();
        (D(d), sums)
    }

    fn check(d: &Path) -> (Result<SumReport, Fail>, String) {
        let mut out = Vec::new();
        let r = check_sums(d, &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    fn err(r: Result<SumReport, Fail>) -> String {
        match r {
            Err(Fail::Msg(m)) => m,
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[test]
    fn clean_archive_passes() {
        let (d, _) = good("ok");
        let (r, out) = check(&d.0);
        assert_eq!(r.unwrap().ok, 3);
        assert_eq!(out, "");
    }

    #[test]
    fn single_byte_flip_fails_hard() {
        let (d, _) = good("flip");
        fs::write(d.0.join("files/files.tar"), b"tarbytez").unwrap();
        let (r, out) = check(&d.0);
        assert_eq!(out, "./files/files.tar: FAILED\n");
        let m = err(r);
        assert!(
            m.starts_with("checksum check FAILED: 1 mismatched [./files/files.tar]"),
            "{m}"
        );
        assert!(m.contains("NOT verified"), "{m}");
    }

    #[test]
    fn missing_listed_file_fails() {
        let (d, _) = good("miss");
        fs::remove_file(d.0.join("db/MAP")).unwrap();
        let (r, out) = check(&d.0);
        assert_eq!(out, "./db/MAP: FAILED open or read\n");
        assert!(err(r).contains("1 unreadable [./db/MAP]"));
    }

    #[test]
    fn malformed_line_fails_even_if_rest_ok() {
        let (d, sums) = good("malformed");
        fs::write(d.0.join("SHA256SUMS"), format!("{sums}garbage line\n")).unwrap();
        assert!(err(check(&d.0).0).contains("1 malformed line(s)"));
    }

    #[test]
    fn truncated_digest_line_fails() {
        // A tampered line that drops one hex digit is malformed, and its
        // file is then unlisted: two reasons, still one hard failure.
        let (d, sums) = good("trunc");
        let mut lines: Vec<String> = sums.lines().map(String::from).collect();
        lines[0].remove(0);
        fs::write(d.0.join("SHA256SUMS"), lines.join("\n") + "\n").unwrap();
        let m = err(check(&d.0).0);
        assert!(
            m.contains("1 malformed") && m.contains("1 unlisted [./db/a.db]"),
            "{m}"
        );
    }

    #[test]
    fn unlisted_extra_file_fails() {
        let (d, _) = good("extra");
        fs::write(d.0.join("db/evil.db"), b"injected").unwrap();
        assert!(err(check(&d.0).0).contains("1 unlisted [./db/evil.db]"));
    }

    #[test]
    fn dropped_sums_line_fails() {
        let (d, sums) = good("drop");
        let kept: String = sums.lines().skip(1).map(|l| format!("{l}\n")).collect();
        fs::write(d.0.join("SHA256SUMS"), kept).unwrap();
        assert!(err(check(&d.0).0).contains("1 unlisted [./db/a.db]"));
    }

    #[test]
    fn absolute_and_dotdot_names_refused() {
        for name in ["/etc/hostname", "../outside", "./db/../../x"] {
            let (d, sums) = good("abs");
            fs::write(
                d.0.join("SHA256SUMS"),
                format!("{sums}{}", sumline(b"x", name)),
            )
            .unwrap();
            assert!(err(check(&d.0).0).contains("1 unsafe name(s)"), "{name}");
        }
    }

    #[test]
    fn symlinked_listed_file_refused() {
        let (d, sums) = good("link");
        let outside = d.0.with_extension("outside");
        fs::write(&outside, b"sqlite-ish").unwrap();
        fs::remove_file(d.0.join("db/a.db")).unwrap();
        std::os::unix::fs::symlink(&outside, d.0.join("db/a.db")).unwrap();
        fs::write(d.0.join("SHA256SUMS"), &sums).unwrap();
        let m = err(check(&d.0).0);
        let _ = fs::remove_file(&outside);
        assert!(m.contains("1 unsafe name(s) [./db/a.db]"), "{m}");
        // Same digest as the real file, so only the symlink rule catches it.
        assert!(m.contains("0 mismatched"), "{m}");
    }

    #[test]
    fn unlisted_symlink_entry_fails() {
        let (d, _) = good("ulink");
        std::os::unix::fs::symlink("/etc", d.0.join("db/x.db")).unwrap();
        assert!(err(check(&d.0).0).contains("1 unlisted [./db/x.db]"));
    }

    #[test]
    fn missing_or_empty_sums_fails() {
        let (d, _) = good("nosums");
        fs::write(d.0.join("SHA256SUMS"), b"").unwrap();
        assert!(err(check(&d.0).0).contains("no properly formatted"));
        fs::remove_file(d.0.join("SHA256SUMS")).unwrap();
        assert!(err(check(&d.0).0).contains("checksum check FAILED: SHA256SUMS"));
    }

    #[test]
    fn uppercase_hex_and_binary_marker_still_ok() {
        let (d, sums) = good("upper");
        let s2: String = sums
            .lines()
            .map(|l| {
                let (h, n) = l.split_at(64);
                format!("{} *{}\n", h.to_ascii_uppercase(), &n[2..])
            })
            .collect();
        fs::write(d.0.join("SHA256SUMS"), s2).unwrap();
        assert_eq!(check(&d.0).0.unwrap().ok, 3);
    }
}

/// argv[0] basenames that mean `restore-test` (the staged slot-361 swap
/// replaces `/opt/galion/rust/bin/galion-restore-test.sh` by a symlink).
pub fn is_restore_test_alias(argv0: &str) -> bool {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    base == "galion-restore-test" || base == "galion-restore-test.sh"
}

#[cfg(test)]
mod alias_tests {
    #[test]
    fn aliases() {
        use super::is_restore_test_alias as f;
        assert!(f("/opt/galion/rust/bin/galion-restore-test.sh"));
        assert!(f("galion-restore-test"));
        assert!(!f("/opt/galion/rust/bin/galion-backup"));
        assert!(!f("galion-restore-test.sh.bak"));
        assert!(!f(""));
    }
}
