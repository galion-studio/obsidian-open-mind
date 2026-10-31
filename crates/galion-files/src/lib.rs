// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Jailed file operations for the console explorer (`console-static --files`)
//! and the Hermes file tools.
//!
//! Every path is relative to one allowlisted [`Root`]. Paths are split into
//! plain segments (`..`, `.`, NUL and backslash refused), joined under the
//! root, and the real (canonical) path must stay inside the root, so symlinks
//! cannot lead out. Writes need [`Root::writable`]. Nothing is ever deleted:
//! [`Root::trash`] moves into `<root>/.trash/`, an edit keeps the previous
//! version in `<root>/.trash/edits/`, and no operation overwrites an existing
//! name (rename, move, copy, upload and extract pick a free name instead or
//! refuse).

#![forbid(unsafe_code)]

pub mod archive;
pub mod inflate;
pub mod kinds;
pub mod media;

use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The trash folder under the root (never emptied by this crate).
pub const TRASH: &str = ".trash";
/// Longest relative path accepted, in bytes.
pub const MAX_REL: usize = 4096;
/// Entries returned by one listing.
pub const MAX_LIST: usize = 5000;
/// Default cap for [`Root::read_text`].
pub const TEXT_MAX: u64 = 4 << 20;
/// Largest text file the editor saves.
pub const WRITE_MAX: usize = 16 << 20;
/// Largest hex page.
pub const HEX_MAX: u64 = 64 << 10;

/// Why an operation was refused. [`Error::status`] gives the HTTP code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NotFound,
    /// Bad path or argument (traversal, absolute path, bad name).
    Bad(String),
    /// Writes are off for this root.
    ReadOnly,
    /// The target name already exists.
    Exists(String),
    /// The file changed since the caller read it.
    Conflict,
    TooLarge(String),
    Unsupported(String),
    Io(String),
}

impl Error {
    pub fn status(&self) -> u16 {
        match self {
            Error::NotFound => 404,
            Error::Bad(_) => 400,
            Error::ReadOnly => 403,
            Error::Exists(_) | Error::Conflict => 409,
            Error::TooLarge(_) => 413,
            Error::Unsupported(_) => 415,
            Error::Io(_) => 500,
        }
    }
    pub fn message(&self) -> String {
        match self {
            Error::NotFound => "not found".into(),
            Error::Bad(m) => m.clone(),
            Error::ReadOnly => "read-only: writes are off for this folder".into(),
            Error::Exists(n) => format!("{n} already exists (nothing is overwritten)"),
            Error::Conflict => "the file changed since it was opened; reload first".into(),
            Error::TooLarge(m) => m.clone(),
            Error::Unsupported(m) => m.clone(),
            Error::Io(m) => m.clone(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// I/O error to [`Error`], without the absolute path (callers show `rel`).
pub fn io_err(e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::NotFound => Error::NotFound,
        io::ErrorKind::AlreadyExists => Error::Exists("target".into()),
        io::ErrorKind::PermissionDenied => Error::Io("permission denied".into()),
        _ => Error::Io(e.kind().to_string()),
    }
}

/// Seconds since the unix epoch (numbers only; shown in Europe/Zagreb).
pub fn now_secs() -> i64 {
    SystemTime::now() // repo-check: allow (unix epoch number for trash stamps; shown in Europe/Zagreb)
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `20261101-002503` in Europe/Zagreb, for trash and backup names.
pub fn stamp(secs: i64) -> String {
    let s = zagreb_time::from_unix(secs);
    let d: String = s.chars().take(19).filter(|c| c.is_ascii_digit()).collect();
    if d.len() == 14 {
        format!("{}-{}", &d[..8], &d[8..])
    } else {
        secs.to_string()
    }
}

fn mtime_secs(m: &fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The editor's change check: mtime (ns), size and CRC-32 of the content.
/// The CRC matters because file systems stamp mtimes with a coarse clock, so
/// two saves within a few ms can share one mtime.
pub fn version_of(abs: &Path) -> Result<String> {
    let m = fs::metadata(abs).map_err(io_err)?;
    let mut f = File::open(abs).map_err(io_err)?;
    let mut crc = inflate::Crc32::default();
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let k = f.read(&mut buf).map_err(io_err)?;
        if k == 0 {
            break;
        }
        crc.update(&buf[..k]);
    }
    Ok(format!("{}-{}-{:08x}", mtime_ns(&m), m.len(), crc.value()))
}

/// Modification time in nanoseconds.
fn mtime_ns(m: &fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Split a relative path into plain segments. `""`, `"/"` and `"."` are the
/// root. Leading and doubled slashes are tolerated; `..`, `.` segments in
/// the middle, NUL, backslash and over-long paths are refused.
pub fn segments(rel: &str) -> Result<Vec<String>> {
    if rel.len() > MAX_REL {
        return Err(Error::Bad("path too long".into()));
    }
    if rel.contains('\0') || rel.contains('\\') {
        return Err(Error::Bad("path has a NUL or backslash".into()));
    }
    let mut out = Vec::new();
    for seg in rel.split('/') {
        match seg {
            "" => continue,
            "." if out.is_empty() && rel.trim_matches('/') == "." => continue,
            "." | ".." => return Err(Error::Bad("path may not contain . or .. segments".into())),
            s => out.push(s.to_string()),
        }
    }
    Ok(out)
}

/// A single new file or folder name: no slash, not `.`/`..`, no NUL, at most
/// 255 bytes, no leading/trailing whitespace.
pub fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(Error::Bad("empty name".into()));
    }
    if name.len() > 255 {
        return Err(Error::Bad("name longer than 255 bytes".into()));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(Error::Bad("a name may not contain / \\ or NUL".into()));
    }
    if name.trim() != name {
        return Err(Error::Bad("a name may not start or end with spaces".into()));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(Error::Bad(
            "a name may not contain control characters".into(),
        ));
    }
    Ok(())
}

/// The jail. `base` is canonical.
#[derive(Debug, Clone)]
pub struct Root {
    base: PathBuf,
    pub writable: bool,
}

static TMP_N: AtomicU64 = AtomicU64::new(0);

impl Root {
    /// `dir` must be an existing directory.
    pub fn new(dir: impl AsRef<Path>, writable: bool) -> io::Result<Root> {
        let base = fs::canonicalize(dir.as_ref())?;
        if !base.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "root is not a directory",
            ));
        }
        Ok(Root { base, writable })
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    fn inside(&self, p: &Path) -> bool {
        p == self.base || p.starts_with(&self.base)
    }

    /// Absolute path of an existing entry; its real path must be inside.
    pub fn existing(&self, rel: &str) -> Result<PathBuf> {
        let segs = segments(rel)?;
        let mut p = self.base.clone();
        for s in &segs {
            p.push(s);
        }
        let real = fs::canonicalize(&p).map_err(io_err)?;
        if !self.inside(&real) {
            return Err(Error::Bad("path leaves the allowed folder".into()));
        }
        Ok(real)
    }

    /// Absolute path for a name that must not exist yet, inside an existing
    /// folder (`dir_rel`). Returns (folder, full path).
    fn fresh(&self, dir_rel: &str, name: &str) -> Result<(PathBuf, PathBuf)> {
        check_name(name)?;
        let dir = self.existing(dir_rel)?;
        if !dir.is_dir() {
            return Err(Error::Bad("target is not a folder".into()));
        }
        let p = dir.join(name);
        if fs::symlink_metadata(&p).is_ok() {
            return Err(Error::Exists(name.to_string()));
        }
        Ok((dir, p))
    }

    /// Relative path (with `/`) of an absolute path inside the root.
    pub fn rel_of(&self, abs: &Path) -> String {
        let r = abs.strip_prefix(&self.base).unwrap_or(Path::new(""));
        let parts: Vec<String> = r
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        parts.join("/")
    }

    fn trash_dir(&self) -> PathBuf {
        self.base.join(TRASH)
    }

    fn in_trash(&self, abs: &Path) -> bool {
        abs.starts_with(self.trash_dir())
    }

    fn need_write(&self) -> Result<()> {
        if self.writable {
            Ok(())
        } else {
            Err(Error::ReadOnly)
        }
    }

    /// Refuse writing new things into `.trash` (only [`Root::trash`] does).
    fn not_trash(&self, abs: &Path) -> Result<()> {
        if self.in_trash(abs) {
            return Err(Error::Bad("the trash only takes things via delete".into()));
        }
        Ok(())
    }

    // ------------------------------------------------------------ reading

    fn entry_json(&self, abs: &Path, name: &str) -> Value {
        let lmeta = fs::symlink_metadata(abs).ok();
        let link = lmeta.as_ref().is_some_and(|m| m.file_type().is_symlink());
        // Follow a link only when it stays inside.
        let meta = if link {
            fs::canonicalize(abs)
                .ok()
                .filter(|r| self.inside(r))
                .and_then(|r| fs::metadata(r).ok())
        } else {
            lmeta.clone()
        };
        let Some(m) = meta else {
            return json!({"name": name, "path": self.rel_of(abs), "dir": false, "link": link,
                "broken": true, "kind": "binary", "preview": "hex", "hidden": name.starts_with('.')});
        };
        let dir = m.is_dir();
        let t = if dir {
            None
        } else {
            Some(kinds::by_name(name).unwrap_or_else(|| {
                let mut head = [0u8; 512];
                let n = File::open(abs)
                    .and_then(|mut f| f.read(&mut head))
                    .unwrap_or(0);
                kinds::detect(name, &head[..n])
            }))
        };
        let secs = mtime_secs(&m);
        json!({
            "name": name,
            "path": self.rel_of(abs),
            "dir": dir,
            "size": if dir { 0 } else { m.len() },
            "mtime": secs,
            "mtime_local": zagreb_time::from_unix(secs),
            "kind": t.map(|t| t.kind.as_str()).unwrap_or("folder"),
            "mime": t.map(|t| t.mime).unwrap_or("inode/directory"),
            "lang": t.map(|t| t.lang).unwrap_or(""),
            "preview": t.map(|t| t.kind.preview()).unwrap_or("list"),
            "hidden": name.starts_with('.'),
            "link": link,
        })
    }

    /// Folder listing: folders first, then names (case-insensitive).
    /// Dotfiles only when `hidden`.
    pub fn list(&self, rel: &str, hidden: bool) -> Result<Value> {
        let dir = self.existing(rel)?;
        if !dir.is_dir() {
            return Err(Error::Bad("not a folder".into()));
        }
        let mut names: Vec<(bool, String)> = Vec::new();
        let mut total = 0usize;
        for e in fs::read_dir(&dir).map_err(io_err)? {
            let Ok(e) = e else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                continue;
            }
            total += 1;
            let is_dir = e.path().is_dir();
            names.push((!is_dir, name));
        }
        names.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        let truncated = names.len() > MAX_LIST;
        names.truncate(MAX_LIST);
        let entries: Vec<Value> = names
            .iter()
            .map(|(_, n)| self.entry_json(&dir.join(n), n))
            .collect();
        let here = self.rel_of(&dir);
        let parent = if here.is_empty() {
            Value::Null
        } else {
            json!(here.rsplit_once('/').map(|x| x.0).unwrap_or(""))
        };
        Ok(json!({
            "ok": true, "path": here, "parent": parent, "entries": entries,
            "total": total, "truncated": truncated, "writable": self.writable,
            "in_trash": self.in_trash(&dir),
        }))
    }

    /// Metadata and type (from the name, then the first 8 KiB).
    pub fn info(&self, rel: &str) -> Result<Value> {
        let abs = self.existing(rel)?;
        let name = abs
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut v = self.entry_json(&abs, &name);
        let m = fs::metadata(&abs).map_err(io_err)?;
        if m.is_file() {
            let mut head = vec![0u8; 8192];
            let n = File::open(&abs)
                .and_then(|mut f| f.read(&mut head))
                .unwrap_or(0);
            head.truncate(n);
            let t = kinds::detect(&name, &head);
            v["kind"] = json!(t.kind.as_str());
            v["mime"] = json!(t.mime);
            v["lang"] = json!(t.lang);
            v["preview"] = json!(t.kind.preview());
            v["sniffed"] = json!(t.sniffed);
            v["ext"] = json!(kinds::extension(&name));
            v["text"] = json!(t.kind.is_text());
            v["mtime_ns"] = json!(mtime_ns(&m).to_string());
            v["editable"] = json!(
                self.writable
                    && t.kind.is_text()
                    && m.len() <= WRITE_MAX as u64
                    && !self.in_trash(&abs)
            );
        } else {
            v["children"] = json!(fs::read_dir(&abs).map(|d| d.count()).unwrap_or(0));
        }
        {
            use std::os::unix::fs::PermissionsExt;
            v["mode"] = json!(format!("{:o}", m.permissions().mode() & 0o7777));
        }
        v["ok"] = json!(true);
        v["writable"] = json!(self.writable);
        v["in_trash"] = json!(self.in_trash(&abs));
        Ok(v)
    }

    /// A text file (UTF-8, lossy), up to `max` bytes.
    pub fn read_text(&self, rel: &str, max: u64) -> Result<Value> {
        let abs = self.existing(rel)?;
        let m = fs::metadata(&abs).map_err(io_err)?;
        if !m.is_file() {
            return Err(Error::Bad("not a file".into()));
        }
        let mut buf = Vec::new();
        File::open(&abs)
            .map_err(io_err)?
            .take(max)
            .read_to_end(&mut buf)
            .map_err(io_err)?;
        let name = abs
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let t = kinds::detect(&name, &buf[..buf.len().min(8192)]);
        if !t.kind.is_text() && !kinds::looks_text(&buf[..buf.len().min(8192)]) {
            return Err(Error::Unsupported("not a text file; use hex".into()));
        }
        Ok(json!({
            "ok": true, "path": self.rel_of(&abs), "text": String::from_utf8_lossy(&buf),
            "size": m.len(), "truncated": m.len() > max, "lang": t.lang,
            "mtime_ns": mtime_ns(&m).to_string(),
            "version": version_of(&abs)?,
            "editable": self.writable && m.len() <= max && m.len() <= WRITE_MAX as u64 && !self.in_trash(&abs),
        }))
    }

    /// Classic hex dump page: 16 bytes a row, offset, hex, ASCII.
    pub fn hex(&self, rel: &str, offset: u64, len: u64) -> Result<Value> {
        let abs = self.existing(rel)?;
        let m = fs::metadata(&abs).map_err(io_err)?;
        if !m.is_file() {
            return Err(Error::Bad("not a file".into()));
        }
        let len = len.clamp(16, HEX_MAX);
        let mut f = File::open(&abs).map_err(io_err)?;
        f.seek(SeekFrom::Start(offset)).map_err(io_err)?;
        let mut buf = Vec::new();
        f.take(len).read_to_end(&mut buf).map_err(io_err)?;
        Ok(json!({
            "ok": true, "path": self.rel_of(&abs), "offset": offset, "len": buf.len(),
            "size": m.len(), "more": offset + (buf.len() as u64) < m.len(),
            "dump": hexdump(offset, &buf),
        }))
    }

    /// Open a file for streaming (download, media, images). Returns the
    /// file, its size and the MIME type from the table.
    pub fn open(&self, rel: &str) -> Result<(File, u64, kinds::TypeInfo, String)> {
        let abs = self.existing(rel)?;
        let m = fs::metadata(&abs).map_err(io_err)?;
        if !m.is_file() {
            return Err(Error::Bad("not a file".into()));
        }
        let name = abs
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut f = File::open(&abs).map_err(io_err)?;
        let mut head = [0u8; 512];
        let n = f.read(&mut head).unwrap_or(0);
        f.seek(SeekFrom::Start(0)).map_err(io_err)?;
        let t = kinds::detect(&name, &head[..n]);
        Ok((f, m.len(), t, name))
    }

    // ------------------------------------------------------------ writing

    /// Save a text file. `expect_version` (from [`Root::read_text`]) guards against
    /// overwriting someone else's change; the previous content is kept in
    /// `.trash/edits/`. `create` makes a new file (refused if it exists).
    pub fn write_text(
        &self,
        rel: &str,
        content: &str,
        expect_version: Option<&str>,
        create: bool,
    ) -> Result<Value> {
        self.need_write()?;
        if content.len() > WRITE_MAX {
            return Err(Error::TooLarge(format!(
                "text larger than {} MiB",
                WRITE_MAX >> 20
            )));
        }
        let segs = segments(rel)?;
        let Some((name, dir_segs)) = segs.split_last() else {
            return Err(Error::Bad("no file name".into()));
        };
        let dir_rel = dir_segs.join("/");
        if create {
            let (dir, p) = self.fresh(&dir_rel, name)?;
            self.not_trash(&dir)?;
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&p)
                .map_err(io_err)?;
            f.write_all(content.as_bytes()).map_err(io_err)?;
            f.sync_all().map_err(io_err)?;
            return self.saved(&p, None);
        }
        let abs = self.existing(rel)?;
        self.not_trash(&abs)?;
        let m = fs::metadata(&abs).map_err(io_err)?;
        if !m.is_file() {
            return Err(Error::Bad("not a file".into()));
        }
        if let Some(want) = expect_version {
            if want != version_of(&abs)? {
                return Err(Error::Conflict);
            }
        }
        // Keep the previous version.
        let edits = self.trash_dir().join("edits");
        fs::create_dir_all(&edits).map_err(io_err)?;
        let bak = unique(&edits, &format!("{}-{}.bak", stamp(now_secs()), name));
        fs::copy(&abs, &bak).map_err(io_err)?;
        // Atomic replace through a sibling temp file, same permissions.
        let dir = abs.parent().ok_or(Error::NotFound)?;
        let tmp = dir.join(format!(
            ".{name}.galion-save-{}-{}",
            std::process::id(),
            TMP_N.fetch_add(1, Ordering::SeqCst)
        ));
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(io_err)?;
            f.write_all(content.as_bytes()).map_err(io_err)?;
            f.sync_all().map_err(io_err)?;
        }
        let _ = fs::set_permissions(&tmp, m.permissions());
        if let Err(e) = fs::rename(&tmp, &abs) {
            let _ = fs::rename(&tmp, tmp.with_extension("off"));
            return Err(io_err(e));
        }
        self.saved(&abs, Some(&bak))
    }

    fn saved(&self, abs: &Path, bak: Option<&Path>) -> Result<Value> {
        let m = fs::metadata(abs).map_err(io_err)?;
        Ok(json!({
            "ok": true, "path": self.rel_of(abs), "size": m.len(),
            "mtime_ns": mtime_ns(&m).to_string(),
            "version": version_of(abs)?,
            "backup": bak.map(|b| self.rel_of(b)),
        }))
    }

    /// New empty folder.
    pub fn mkdir(&self, dir_rel: &str, name: &str) -> Result<Value> {
        self.need_write()?;
        let (dir, p) = self.fresh(dir_rel, name)?;
        self.not_trash(&dir)?;
        fs::create_dir(&p).map_err(io_err)?;
        Ok(json!({"ok": true, "path": self.rel_of(&p)}))
    }

    /// Rename in place. Never overwrites.
    pub fn rename(&self, rel: &str, new_name: &str) -> Result<Value> {
        self.need_write()?;
        let abs = self.existing_entry(rel)?;
        let dir = abs
            .parent()
            .ok_or(Error::Bad("cannot rename the root".into()))?;
        check_name(new_name)?;
        let to = dir.join(new_name);
        if fs::symlink_metadata(&to).is_ok() {
            return Err(Error::Exists(new_name.into()));
        }
        fs::rename(&abs, &to).map_err(io_err)?;
        Ok(json!({"ok": true, "from": self.rel_of(&abs), "path": self.rel_of(&to)}))
    }

    /// The entry itself (a symlink is moved as a link, not followed), which
    /// must not be the root or the trash folder.
    fn existing_entry(&self, rel: &str) -> Result<PathBuf> {
        let segs = segments(rel)?;
        if segs.is_empty() {
            return Err(Error::Bad("not allowed on the root folder".into()));
        }
        let parent_rel = segs[..segs.len() - 1].join("/");
        let parent = self.existing(&parent_rel)?;
        let p = parent.join(&segs[segs.len() - 1]);
        fs::symlink_metadata(&p).map_err(io_err)?;
        if p == self.trash_dir() {
            return Err(Error::Bad(
                "the trash folder itself cannot be changed".into(),
            ));
        }
        Ok(p)
    }

    /// Move into another folder. Never overwrites; a folder cannot move into
    /// itself.
    pub fn move_to(&self, rel: &str, dest_dir_rel: &str) -> Result<Value> {
        self.need_write()?;
        let abs = self.existing_entry(rel)?;
        let dest = self.existing(dest_dir_rel)?;
        if !dest.is_dir() {
            return Err(Error::Bad("target is not a folder".into()));
        }
        self.not_trash(&dest)?;
        if dest.starts_with(&abs) {
            return Err(Error::Bad("a folder cannot move into itself".into()));
        }
        let name = abs.file_name().ok_or(Error::NotFound)?;
        let to = dest.join(name);
        if fs::symlink_metadata(&to).is_ok() {
            return Err(Error::Exists(name.to_string_lossy().into_owned()));
        }
        fs::rename(&abs, &to).map_err(io_err)?;
        Ok(json!({"ok": true, "from": self.rel_of(&abs), "path": self.rel_of(&to)}))
    }

    /// Copy a file or folder (recursive) into a folder; a taken name gets
    /// ` (1)`, ` (2)` ... Symlinks inside folders are skipped.
    pub fn copy_to(&self, rel: &str, dest_dir_rel: &str) -> Result<Value> {
        self.need_write()?;
        let abs = self.existing(rel)?;
        if abs == self.base {
            return Err(Error::Bad("not allowed on the root folder".into()));
        }
        let dest = self.existing(dest_dir_rel)?;
        if !dest.is_dir() {
            return Err(Error::Bad("target is not a folder".into()));
        }
        self.not_trash(&dest)?;
        if dest.starts_with(&abs) {
            return Err(Error::Bad("a folder cannot be copied into itself".into()));
        }
        let name = abs
            .file_name()
            .ok_or(Error::NotFound)?
            .to_string_lossy()
            .into_owned();
        let to = unique(&dest, &name);
        let mut count = 0u64;
        copy_tree(&abs, &to, &mut count)?;
        Ok(json!({"ok": true, "from": self.rel_of(&abs), "path": self.rel_of(&to), "files": count}))
    }

    /// "Delete": move into `.trash/<stamp>-<name>`. Nothing is removed from
    /// disk; restore by moving it back. Things already in the trash stay.
    pub fn trash(&self, rel: &str) -> Result<Value> {
        self.need_write()?;
        let abs = self.existing_entry(rel)?;
        if self.in_trash(&abs) {
            return Err(Error::Bad(
                "already in the trash; nothing is ever deleted for real (move it out to restore)"
                    .into(),
            ));
        }
        let t = self.trash_dir();
        fs::create_dir_all(&t).map_err(io_err)?;
        let name = abs
            .file_name()
            .ok_or(Error::NotFound)?
            .to_string_lossy()
            .into_owned();
        let to = unique(&t, &format!("{}-{}", stamp(now_secs()), name));
        fs::rename(&abs, &to).map_err(io_err)?;
        Ok(
            json!({"ok": true, "from": self.rel_of(&abs), "path": self.rel_of(&to),
            "note": "moved to the trash; move it back to restore"}),
        )
    }

    /// Stream an upload into `dir_rel/name` (a taken name gets ` (1)` ...).
    /// The bytes land in a hidden temp file first; on error or when over
    /// `max` bytes it is renamed to `*.off` (kept, never deleted).
    pub fn upload<R: Read>(
        &self,
        dir_rel: &str,
        name: &str,
        mut body: R,
        max: u64,
    ) -> Result<Value> {
        self.need_write()?;
        check_name(name)?;
        let dir = self.existing(dir_rel)?;
        if !dir.is_dir() {
            return Err(Error::Bad("target is not a folder".into()));
        }
        self.not_trash(&dir)?;
        let tmp = dir.join(format!(
            ".{name}.galion-upload-{}-{}",
            std::process::id(),
            TMP_N.fetch_add(1, Ordering::SeqCst)
        ));
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(io_err)?;
        let mut buf = vec![0u8; 64 << 10];
        let mut total = 0u64;
        let fail = |e: Error| {
            let _ = fs::rename(&tmp, tmp.with_extension("off"));
            e
        };
        loop {
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(fail(io_err(e))),
            };
            total += n as u64;
            if total > max {
                return Err(fail(Error::TooLarge(format!(
                    "upload larger than {max} bytes"
                ))));
            }
            if let Err(e) = f.write_all(&buf[..n]) {
                return Err(fail(io_err(e)));
            }
        }
        f.sync_all().map_err(io_err)?;
        drop(f);
        let to = unique(&dir, name);
        fs::rename(&tmp, &to).map_err(io_err)?;
        Ok(json!({"ok": true, "path": self.rel_of(&to), "size": total}))
    }

    // ------------------------------------------------------------ archives

    /// Members of a zip / rar / tar(.gz).
    pub fn archive_list(&self, rel: &str) -> Result<Value> {
        let abs = self.existing(rel)?;
        let mut v = archive::list(&abs)?;
        v["path"] = json!(self.rel_of(&abs));
        v["ok"] = json!(true);
        Ok(v)
    }

    /// Extract an archive into a new folder named after it (inside
    /// `dest_dir_rel`, default the archive's folder). Member names are
    /// cleaned (no absolute paths, no `..`), links are skipped, and the total
    /// is capped at `max_bytes`. The work happens in a hidden staging folder
    /// that is renamed into place at the end (or to `*.off` on failure).
    pub fn extract(&self, rel: &str, dest_dir_rel: Option<&str>, max_bytes: u64) -> Result<Value> {
        self.need_write()?;
        let abs = self.existing(rel)?;
        let dest = match dest_dir_rel {
            Some(d) => self.existing(d)?,
            None => abs.parent().ok_or(Error::NotFound)?.to_path_buf(),
        };
        if !dest.is_dir() {
            return Err(Error::Bad("target is not a folder".into()));
        }
        self.not_trash(&dest)?;
        let name = abs
            .file_name()
            .ok_or(Error::NotFound)?
            .to_string_lossy()
            .into_owned();
        let stem = archive::stem(&name);
        let stage = dest.join(format!(
            ".{stem}.galion-extract-{}-{}",
            std::process::id(),
            TMP_N.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&stage).map_err(io_err)?;
        let res = archive::extract(&abs, &stage, max_bytes);
        match res {
            Ok(stats) => {
                let to = unique(&dest, &stem);
                fs::rename(&stage, &to).map_err(io_err)?;
                Ok(
                    json!({"ok": true, "path": self.rel_of(&to), "files": stats.files,
                    "bytes": stats.bytes, "skipped": stats.skipped}),
                )
            }
            Err(e) => {
                let _ = fs::rename(&stage, stage.with_extension("off"));
                Err(e)
            }
        }
    }
}

/// `name`, or `name (1).ext`, `name (2).ext` ... whichever is free in `dir`.
pub fn unique(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if fs::symlink_metadata(&p).is_err() {
        return p;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1..10_000 {
        let c = dir.join(format!("{stem} ({n}){ext}"));
        if fs::symlink_metadata(&c).is_err() {
            return c;
        }
    }
    dir.join(format!("{stem} ({}){ext}", now_secs()))
}

fn copy_tree(from: &Path, to: &Path, count: &mut u64) -> Result<()> {
    let m = fs::symlink_metadata(from).map_err(io_err)?;
    if m.file_type().is_symlink() {
        // Top-level links were resolved by the caller; nested ones are skipped.
        return Ok(());
    }
    if m.is_dir() {
        fs::create_dir(to).map_err(io_err)?;
        for e in fs::read_dir(from).map_err(io_err)? {
            let e = e.map_err(io_err)?;
            copy_tree(&e.path(), &to.join(e.file_name()), count)?;
        }
    } else {
        if fs::symlink_metadata(to).is_ok() {
            return Err(Error::Exists(
                to.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ));
        }
        fs::copy(from, to).map_err(io_err)?;
        *count += 1;
    }
    Ok(())
}

/// `00000010  48 65 6c 6c 6f 0a ...  |Hello.|`
pub fn hexdump(offset: u64, data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 4 + 16);
    for (i, chunk) in data.chunks(16).enumerate() {
        s.push_str(&format!("{:08x}  ", offset + (i as u64) * 16));
        for j in 0..16 {
            match chunk.get(j) {
                Some(b) => s.push_str(&format!("{b:02x} ")),
                None => s.push_str("   "),
            }
            if j == 7 {
                s.push(' ');
            }
        }
        s.push_str(" |");
        for &b in chunk {
            s.push(if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            });
        }
        s.push_str("|\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_refuse_traversal() {
        assert_eq!(segments("").unwrap(), Vec::<String>::new());
        assert_eq!(segments("/").unwrap(), Vec::<String>::new());
        assert_eq!(segments(".").unwrap(), Vec::<String>::new());
        assert_eq!(segments("a//b/").unwrap(), vec!["a", "b"]);
        assert_eq!(segments(".env").unwrap(), vec![".env"]);
        for bad in ["..", "a/../b", "./a", "a/.", "a\\b", "a\0b"] {
            assert!(segments(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn names() {
        assert!(check_name("ok.txt").is_ok());
        assert!(check_name(".env").is_ok());
        for bad in ["", ".", "..", "a/b", " x", "x ", "a\nb"] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn hexdump_rows() {
        let d = hexdump(16, b"Hello\n");
        assert!(d.starts_with("00000010  48 65 6c 6c 6f 0a"));
        assert!(d.trim_end().ends_with("|Hello.|"));
    }

    #[test]
    fn stamp_is_local_digits() {
        let s = stamp(1_793_000_000);
        assert_eq!(s.len(), 15);
        assert_eq!(&s[8..9], "-");
    }
}
