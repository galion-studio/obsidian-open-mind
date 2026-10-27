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
//! | `sha256sum --quiet -c SHA256SUMS`                | [`check_sums`] (same stdout lines)    |
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

/// `sha256sum --quiet -c SHA256SUMS` run in `dir`: failure lines to `out`
/// exactly as coreutils prints them (`NAME: FAILED`, `NAME: FAILED open or
/// read`); warnings to stderr; Err if anything failed or no line parsed.
/// Returns the number of files checked.
pub fn check_sums(dir: &Path, out: &mut dyn Write) -> Result<u64, Fail> {
    let list = dir.join("SHA256SUMS");
    let f = File::open(&list).map_err(ctx("sha256sum: SHA256SUMS"))?;
    let (mut ok, mut bad_fmt, mut mismatch, mut unreadable) = (0u64, 0u64, 0u64, 0u64);
    for line in BufReader::new(f).split(b'\n') {
        check_signal()?;
        let mut line = line.map_err(ctx("sha256sum: SHA256SUMS"))?;
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let Some((want, name)) = parse_sum_line(&line) else {
            bad_fmt += 1;
            continue;
        };
        let shown = String::from_utf8_lossy(&name).into_owned();
        // Names are relative to dir (./x as written by `find .`); an absolute
        // name is checked where it points, exactly like sha256sum would.
        let p = dir.join(OsStr::from_bytes(&name));
        match sha256_file(&p) {
            Ok(got) if got == want => ok += 1,
            Ok(_) => {
                mismatch += 1;
                writeln!(out, "{shown}: FAILED").ok();
            }
            Err(e) => {
                unreadable += 1;
                eprintln!("sha256sum: {shown}: {e}");
                writeln!(out, "{shown}: FAILED open or read").ok();
            }
        }
    }
    out.flush().ok();
    let plural = |n: u64, one: &str, many: &str| {
        if n == 1 {
            one.to_string()
        } else {
            many.to_string()
        }
    };
    if bad_fmt > 0 {
        eprintln!(
            "sha256sum: WARNING: {bad_fmt} {} improperly formatted",
            plural(bad_fmt, "line is", "lines are")
        );
    }
    if ok + mismatch + unreadable == 0 {
        return Err(Fail::Msg(
            "sha256sum: SHA256SUMS: no properly formatted checksum lines found".into(),
        ));
    }
    if unreadable > 0 {
        eprintln!(
            "sha256sum: WARNING: {unreadable} listed {} could not be read",
            plural(unreadable, "file", "files")
        );
    }
    if mismatch > 0 {
        eprintln!(
            "sha256sum: WARNING: {mismatch} computed {} did NOT match",
            plural(mismatch, "checksum", "checksums")
        );
    }
    if mismatch + unreadable > 0 {
        return Err(Fail::Msg(format!(
            "checksum check failed ({mismatch} mismatched, {unreadable} unreadable of {})",
            ok + mismatch + unreadable
        )));
    }
    Ok(ok)
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
    extract(GzReader::new(BufReader::with_capacity(256 * 1024, f)), &t.0).map_err(|e| match e {
        Fail::Msg(m) => Fail::Msg(format!("{}: {m}", a.display())),
        s => s,
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
    check_sums(&s, out)?;
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
