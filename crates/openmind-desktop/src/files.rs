// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Pick, read, and upload files for the control console.
//!
//! Three rules hold for every byte that leaves this module:
//! - a read opens a path only when that path is inside a directory the
//!   operator passed, or is the one file they passed;
//! - `..`, a symlink, and a path that does not stay under that choice are
//!   refused before the file is opened;
//! - a name is uploaded only when its extension is on the allow-list, its
//!   bytes match that type, and the size is within [`MAX_FILE_BYTES`].
//!
//! The browser picker sends bytes, not a path. Those bytes are checked the
//! same way and never used as a filesystem path. Upload dials a loopback
//! address only (Galion One's `POST /files` by default).

use crate::error::DesktopError;
use galion_common::json;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Largest file this desktop will read or upload.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;

/// How many names [`Files::pick`] returns.
pub const MAX_PICK: usize = 64;

/// Directory entries examined while picking, so a huge folder stops early.
const MAX_SCAN: usize = 1024;

/// Text preview returned by a read. The upload still sends the whole file.
pub const MAX_PREVIEW: usize = 4096;

/// Stored display name, in characters.
const MAX_NAME: usize = 80;

/// JSON path bodies are a single object, not a file.
const MAX_JSON: usize = 8192;

/// Index of stored files. Ids are hex, so this name cannot collide with one.
const INDEX_NAME: &str = "index.jsonl";

/// How much of the index a list will read.
const MAX_INDEX: u64 = 256 * 1024;

/// Galion One's loopback file intake (`POST /files`).
pub const DEFAULT_STACK: &str = "127.0.0.1:8091";

/// One extension the desktop will read and upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileType {
    /// Lower-case extension without the dot.
    pub ext: &'static str,
    /// `text`, `pdf`, or `image`.
    pub kind: &'static str,
}

/// Text, PDF, and the three image types Galion One already treats as pictures.
/// Executables, archives, and markup are not on this list.
pub const ALLOWED: &[FileType] = &[
    FileType {
        ext: "txt",
        kind: "text",
    },
    FileType {
        ext: "md",
        kind: "text",
    },
    FileType {
        ext: "json",
        kind: "text",
    },
    FileType {
        ext: "csv",
        kind: "text",
    },
    FileType {
        ext: "log",
        kind: "text",
    },
    FileType {
        ext: "pdf",
        kind: "pdf",
    },
    FileType {
        ext: "jpg",
        kind: "image",
    },
    FileType {
        ext: "jpeg",
        kind: "image",
    },
    FileType {
        ext: "png",
        kind: "image",
    },
    FileType {
        ext: "webp",
        kind: "image",
    },
];

/// `accept` attribute for the page's file input. The server checks again.
#[must_use]
pub fn accept_list() -> String {
    let mut out = String::new();
    for (i, kind) in ALLOWED.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('.');
        out.push_str(kind.ext);
    }
    out
}

/// Why a pick, read, or upload stopped.
#[derive(Debug)]
pub enum FileError {
    /// Zero bytes.
    Empty,
    /// Larger than the desktop cap.
    TooLarge,
    /// Extension is not on [`ALLOWED`].
    Type,
    /// Bytes do not match the extension.
    Magic,
    /// The path contains `..`.
    Traversal,
    /// The path is not one the operator chose.
    Outside,
    /// A symlink would leave the chosen path.
    Symlink,
    /// Nothing at that path inside the choice.
    NotFound,
    /// A directory, or some other non-regular file.
    NotFile,
    /// No inbox and no stack address.
    NoDestination,
    /// The stack address is not loopback, so nothing was dialled.
    NotLoopback,
    /// The loopback intake did not accept the connection.
    Unreachable,
    /// The intake answered, and the answer was a refusal (for example unpaired).
    Pair,
    /// The intake answered with a failure status.
    StackStatus,
    /// The name is empty or not a single safe component.
    BadName,
    /// The path object was not `{"path":"..."}`.
    BadJson,
    /// Reading or storing failed. The text is for the operator's terminal.
    Io(String),
}

impl FileError {
    /// Stable token for the JSON error body. No path and no file bytes.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            FileError::Empty => "empty",
            FileError::TooLarge => "too-large",
            FileError::Type => "type",
            FileError::Magic => "magic",
            FileError::Traversal => "traversal",
            FileError::Outside => "outside",
            FileError::Symlink => "symlink",
            FileError::NotFound => "not-found",
            FileError::NotFile => "not-file",
            FileError::NoDestination => "no-destination",
            FileError::NotLoopback => "not-loopback",
            FileError::Unreachable => "unreachable",
            FileError::Pair => "pair",
            FileError::StackStatus => "stack",
            FileError::BadName => "bad-name",
            FileError::BadJson => "bad-json",
            FileError::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Io(msg) => write!(f, "io: {msg}"),
            other => f.write_str(other.code()),
        }
    }
}

#[derive(Debug)]
struct Grant {
    /// Canonical directory. For a chosen file this is its parent.
    root: PathBuf,
    /// Set when the operator named one file. Nothing else under `root` is readable.
    only: Option<PathBuf>,
}

struct Inner {
    grants: Vec<Grant>,
    inbox: Option<PathBuf>,
    stack: Option<SocketAddr>,
    max_bytes: usize,
    seq: AtomicU64,
}

/// The operator's choices: which paths may be read, where a copy is kept,
/// and which loopback intake receives the upload.
#[derive(Clone)]
pub struct Files {
    inner: Arc<Inner>,
}

/// One name [`Files::pick`] can offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    /// Path relative to the chosen directory, `/`-separated.
    pub path: String,
    /// Sanitized file name.
    pub name: String,
    /// `text`, `pdf`, or `image`.
    pub kind: &'static str,
    /// Size in bytes, from the directory entry. The file is not opened.
    pub size: u64,
}

/// A file that was read inside a chosen path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFile {
    /// Sanitized file name.
    pub name: String,
    /// `text`, `pdf`, or `image`.
    pub kind: &'static str,
    /// Size in bytes.
    pub size: usize,
    /// Path relative to the chosen directory.
    pub path: String,
    /// Start of a text file, at most [`MAX_PREVIEW`] bytes. Absent for pdf and image.
    pub text: Option<String>,
}

/// What an upload did with an accepted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// Sanitized file name.
    pub name: String,
    /// `text`, `pdf`, or `image`.
    pub kind: &'static str,
    /// Size in bytes.
    pub size: usize,
    /// A copy was written under the inbox, named by id.
    pub stored: bool,
    /// Inbox id, when `stored`.
    pub id: Option<String>,
    /// `off`, `ok`, `pair`, `unreachable`, or `refused`.
    pub stack: &'static str,
}

struct ParsedPath {
    absolute: bool,
    parts: Vec<String>,
}

struct Resolved {
    path: PathBuf,
    rel: Vec<String>,
    name: String,
}

impl Files {
    /// No grants, no inbox, no stack. Reads and uploads fail closed.
    #[must_use]
    pub fn closed() -> Self {
        Self::build(Vec::new(), None, None, MAX_FILE_BYTES)
    }

    /// Check the operator's paths and the stack address.
    ///
    /// `stack` is `None` for off, or `host:port`. Anything that is not
    /// loopback is refused here, before a socket exists.
    ///
    /// # Errors
    /// [`DesktopError::Files`] when a grant or the inbox is missing, is a
    /// symlink, or is not a file or directory, or when `stack` is not loopback.
    pub fn open(
        grants: &[PathBuf],
        inbox: Option<&Path>,
        stack: Option<&str>,
    ) -> Result<Self, DesktopError> {
        Self::open_limited(grants, inbox, stack, MAX_FILE_BYTES)
    }

    fn open_limited(
        grants: &[PathBuf],
        inbox: Option<&Path>,
        stack: Option<&str>,
        max_bytes: usize,
    ) -> Result<Self, DesktopError> {
        if grants.len() > 32 {
            return Err(DesktopError::Files(
                "openmind-desktop: at most 32 --grant paths".into(),
            ));
        }
        let mut opened = Vec::with_capacity(grants.len());
        for path in grants {
            opened.push(open_grant(path)?);
        }
        let inbox = match inbox {
            Some(path) => Some(open_inbox(path)?),
            None => None,
        };
        let stack = match stack {
            Some(addr) => Some(parse_stack(addr)?),
            None => None,
        };
        Ok(Self::build(opened, inbox, stack, max_bytes))
    }

    fn build(
        grants: Vec<Grant>,
        inbox: Option<PathBuf>,
        stack: Option<SocketAddr>,
        max_bytes: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                grants,
                inbox,
                stack,
                max_bytes,
                seq: AtomicU64::new(1),
            }),
        }
    }

    fn max_bytes(&self) -> usize {
        self.inner.max_bytes
    }

    /// Names inside the chosen paths that the allow-list would accept.
    /// Does not open file contents and does not follow symlinks.
    #[must_use]
    pub fn pick(&self) -> Vec<Pick> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        let max = self.max_bytes();
        for grant in &self.inner.grants {
            if out.len() >= MAX_PICK {
                break;
            }
            if let Some(only) = &grant.only {
                if let Some(item) = pick_file(only, max) {
                    if !seen.contains(&item.0) {
                        seen.push(item.0);
                        out.push(item.1);
                    }
                }
                continue;
            }
            let Ok(rd) = fs::read_dir(&grant.root) else {
                continue;
            };
            let mut scanned = 0usize;
            for ent in rd {
                scanned += 1;
                if scanned > MAX_SCAN || out.len() >= MAX_PICK {
                    break;
                }
                let Ok(ent) = ent else { continue };
                let Ok(meta) = fs::symlink_metadata(ent.path()) else {
                    continue;
                };
                if meta.file_type().is_symlink() || !meta.is_file() {
                    continue;
                }
                let Ok(name) = ent.file_name().into_string() else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                let Some(item) = describe(&ent.path(), &name, meta.len(), max) else {
                    continue;
                };
                if seen.contains(&item.0) {
                    continue;
                }
                seen.push(item.0);
                out.push(item.1);
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// Read one file inside a chosen path.
    ///
    /// # Errors
    /// See [`FileError`]. The error carries no file bytes.
    pub fn read_path(&self, input: &str) -> Result<ReadFile, FileError> {
        let resolved = self.resolve(input)?;
        let bytes = read_limited(&resolved.path, self.max_bytes())?;
        let kind = classify(&resolved.name, &bytes)?;
        let text = if kind == "text" {
            Some(preview(&bytes))
        } else {
            None
        };
        Ok(ReadFile {
            name: resolved.name,
            kind,
            size: bytes.len(),
            path: rel_display(&resolved.rel),
            text,
        })
    }

    /// Read a chosen path and upload those bytes.
    ///
    /// # Errors
    /// See [`FileError`]. A refusal happens before the stack is dialled.
    pub fn upload_path(&self, input: &str) -> Result<Receipt, FileError> {
        let resolved = self.resolve(input)?;
        let bytes = read_limited(&resolved.path, self.max_bytes())?;
        self.accept(&resolved.name, &bytes)
    }

    /// Upload bytes the browser picker already read. `name` is a label.
    /// It is never opened as a path.
    ///
    /// # Errors
    /// See [`FileError`].
    pub fn upload_bytes(&self, name: &str, bytes: &[u8]) -> Result<Receipt, FileError> {
        let name = sanitize_name(name)?;
        self.accept(&name, bytes)
    }

    fn accept(&self, name: &str, bytes: &[u8]) -> Result<Receipt, FileError> {
        if bytes.is_empty() {
            return Err(FileError::Empty);
        }
        if bytes.len() > self.max_bytes() {
            return Err(FileError::TooLarge);
        }
        let kind = classify(name, bytes)?;
        let have_inbox = self.inner.inbox.is_some();
        let have_stack = self.inner.stack.is_some();
        if !have_inbox && !have_stack {
            return Err(FileError::NoDestination);
        }
        let mut stored = false;
        let mut id = None;
        if let Some(dir) = &self.inner.inbox {
            let fid = self.alloc_id(bytes);
            store(dir, &fid, name, kind, bytes)?;
            stored = true;
            id = Some(fid);
        }
        let stack = if let Some(addr) = self.inner.stack {
            stack_outcome(forward(addr, name, bytes), stored)?
        } else {
            "off"
        };
        Ok(Receipt {
            name: name.to_string(),
            kind,
            size: bytes.len(),
            stored,
            id,
            stack,
        })
    }

    fn resolve(&self, input: &str) -> Result<Resolved, FileError> {
        let parsed = parse_path(input)?;
        if self.inner.grants.is_empty() {
            return Err(FileError::Outside);
        }
        let mut last: Option<FileError> = None;
        for grant in &self.inner.grants {
            match resolve_in(grant, &parsed, self.max_bytes()) {
                Ok(found) => return Ok(found),
                Err(err) => {
                    last = Some(match last {
                        Some(prev) => prefer(prev, err),
                        None => err,
                    });
                }
            }
        }
        Err(last.unwrap_or(FileError::Outside))
    }

    fn alloc_id(&self, bytes: &[u8]) -> String {
        let n = self.inner.seq.fetch_add(1, Ordering::Relaxed);
        let dig = galion_common::sha256::digest(bytes);
        format!("{n:08x}{}", galion_common::hex::encode(&dig[..4]))
    }

    /// JSON array of files stored in the inbox. Empty when there is no inbox.
    #[must_use]
    pub fn sent_json(&self) -> String {
        let Some(dir) = &self.inner.inbox else {
            return "{\"sent\":[]}".to_string();
        };
        let lines = read_index(&dir.join(INDEX_NAME));
        format!("{{\"sent\":[{}]}}", lines.join(","))
    }

    /// JSON for [`Files::pick`].
    #[must_use]
    pub fn pick_json(&self) -> String {
        let mut parts = Vec::new();
        for item in self.pick() {
            parts.push(format!(
                "{{\"path\":{},\"name\":{},\"kind\":{},\"size\":{}}}",
                json::string(&item.path),
                json::string(&item.name),
                json::string(item.kind),
                item.size
            ));
        }
        format!("{{\"files\":[{}]}}", parts.join(","))
    }
}

impl ReadFile {
    /// JSON object. `text` is present only for a text preview.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = format!(
            "{{\"name\":{},\"kind\":{},\"size\":{},\"path\":{}",
            json::string(&self.name),
            json::string(self.kind),
            self.size,
            json::string(&self.path)
        );
        if let Some(text) = &self.text {
            out.push_str(",\"text\":");
            out.push_str(&json::string(text));
        }
        out.push('}');
        out
    }
}

impl Receipt {
    /// JSON object. `stack` is a short token, not the intake's body.
    #[must_use]
    pub fn to_json(&self) -> String {
        let id = self.id.as_deref().unwrap_or("");
        format!(
            "{{\"name\":{},\"kind\":{},\"size\":{},\"stored\":{},\"id\":{},\"stack\":{}}}",
            json::string(&self.name),
            json::string(self.kind),
            self.size,
            if self.stored { "true" } else { "false" },
            json::string(id),
            json::string(self.stack)
        )
    }
}

/// `{"error":"<code>"}`.
#[must_use]
pub fn error_json(err: &FileError) -> String {
    format!("{{\"error\":{}}}", json::string(err.code()))
}

/// HTTP status for [`FileError`].
#[must_use]
pub fn error_status(err: &FileError) -> u16 {
    match err {
        FileError::Empty | FileError::BadName | FileError::BadJson | FileError::NotFile => 400,
        FileError::TooLarge => 413,
        FileError::Type | FileError::Magic => 415,
        FileError::Traversal | FileError::Outside | FileError::Symlink | FileError::NotLoopback => {
            403
        }
        FileError::NotFound => 404,
        FileError::NoDestination => 503,
        FileError::Unreachable | FileError::Pair | FileError::StackStatus => 502,
        FileError::Io(_) => 500,
    }
}

fn stack_outcome(
    result: Result<&'static str, FileError>,
    stored: bool,
) -> Result<&'static str, FileError> {
    match result {
        Ok(_) => Ok("ok"),
        Err(FileError::Pair) if stored => Ok("pair"),
        Err(FileError::Unreachable) if stored => Ok("unreachable"),
        Err(FileError::StackStatus) if stored => Ok("refused"),
        Err(FileError::NotLoopback) => Err(FileError::NotLoopback),
        Err(_) if stored => Ok("unreachable"),
        Err(err) => Err(err),
    }
}

fn prefer(current: FileError, new: FileError) -> FileError {
    fn rank(err: &FileError) -> u8 {
        match err {
            FileError::Symlink | FileError::Traversal => 5,
            FileError::Outside => 4,
            FileError::NotFile => 3,
            FileError::TooLarge | FileError::Type | FileError::Magic | FileError::Empty => 2,
            FileError::NotFound => 1,
            _ => 2,
        }
    }
    if rank(&new) >= rank(&current) {
        new
    } else {
        current
    }
}

fn parse_stack(addr: &str) -> Result<SocketAddr, DesktopError> {
    let parsed: SocketAddr = addr.parse().map_err(|_| {
        DesktopError::Files(format!(
            "openmind-desktop: --stack {addr:?} is not host:port"
        ))
    })?;
    if parsed.ip().is_loopback() {
        Ok(parsed)
    } else {
        Err(DesktopError::Files(format!(
            "openmind-desktop: --stack {addr} is not a loopback address"
        )))
    }
}

fn open_grant(path: &Path) -> Result<Grant, DesktopError> {
    let meta = fs::symlink_metadata(path).map_err(|err| {
        DesktopError::Files(format!("openmind-desktop: grant {}: {err}", path.display()))
    })?;
    if meta.file_type().is_symlink() {
        return Err(DesktopError::Files(format!(
            "openmind-desktop: grant {} is a symlink; pass the real path",
            path.display()
        )));
    }
    if !meta.is_file() && !meta.is_dir() {
        return Err(DesktopError::Files(format!(
            "openmind-desktop: grant {} is not a file or directory",
            path.display()
        )));
    }
    let canon = path.canonicalize().map_err(|err| {
        DesktopError::Files(format!("openmind-desktop: grant {}: {err}", path.display()))
    })?;
    if meta.is_file() {
        let Some(root) = canon.parent().map(Path::to_path_buf) else {
            return Err(DesktopError::Files(format!(
                "openmind-desktop: grant {} has no parent",
                path.display()
            )));
        };
        Ok(Grant {
            root,
            only: Some(canon),
        })
    } else {
        Ok(Grant {
            root: canon,
            only: None,
        })
    }
}

fn open_inbox(path: &Path) -> Result<PathBuf, DesktopError> {
    if path.as_os_str().is_empty() {
        return Err(DesktopError::Files(
            "openmind-desktop: --inbox path is empty".into(),
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(DesktopError::Files(format!(
            "openmind-desktop: inbox {} is a symlink",
            path.display()
        ))),
        Ok(meta) if meta.is_dir() => path.canonicalize().map_err(|err| {
            DesktopError::Files(format!("openmind-desktop: inbox {}: {err}", path.display()))
        }),
        Ok(_) => Err(DesktopError::Files(format!(
            "openmind-desktop: inbox {} is not a directory",
            path.display()
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => create_inbox(path),
        Err(err) => Err(DesktopError::Files(format!(
            "openmind-desktop: inbox {}: {err}",
            path.display()
        ))),
    }
}

fn create_inbox(path: &Path) -> Result<PathBuf, DesktopError> {
    let name = path
        .file_name()
        .ok_or_else(|| DesktopError::Files("openmind-desktop: --inbox path has no name".into()))?;
    if name == "." || name == ".." {
        return Err(DesktopError::Files(
            "openmind-desktop: --inbox path has no name".into(),
        ));
    }
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let parent_meta = fs::symlink_metadata(parent).map_err(|err| {
        DesktopError::Files(format!(
            "openmind-desktop: inbox parent {}: {err}",
            parent.display()
        ))
    })?;
    if parent_meta.file_type().is_symlink() {
        return Err(DesktopError::Files(format!(
            "openmind-desktop: inbox parent {} is a symlink",
            parent.display()
        )));
    }
    if !parent_meta.is_dir() {
        return Err(DesktopError::Files(format!(
            "openmind-desktop: inbox parent {} is not a directory",
            parent.display()
        )));
    }
    let parent = parent.canonicalize().map_err(|err| {
        DesktopError::Files(format!(
            "openmind-desktop: inbox parent {}: {err}",
            parent.display()
        ))
    })?;
    let dest = parent.join(name);
    create_private_dir(&dest).map_err(|err| {
        DesktopError::Files(format!("openmind-desktop: inbox {}: {err}", dest.display()))
    })?;
    Ok(dest)
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(false)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

fn pick_file(path: &Path, max: usize) -> Option<(PathBuf, Pick)> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    describe(path, name, meta.len(), max)
}

fn describe(path: &Path, name: &str, size: u64, max: usize) -> Option<(PathBuf, Pick)> {
    if size == 0 || size > max as u64 {
        return None;
    }
    let name = sanitize_name(name).ok()?;
    let ext = extension(&name)?;
    let kind = ALLOWED.iter().find(|item| item.ext == ext)?.kind;
    let display = name.clone();
    Some((
        path.to_path_buf(),
        Pick {
            path: display.clone(),
            name,
            kind,
            size,
        },
    ))
}

fn parse_path(input: &str) -> Result<ParsedPath, FileError> {
    let raw = input.trim();
    if raw.is_empty() || raw.len() > 1024 {
        return Err(FileError::BadName);
    }
    if raw.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err(FileError::BadName);
    }
    let mut parts = Vec::new();
    let mut absolute = false;
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(part) => {
                let Some(part) = part.to_str() else {
                    return Err(FileError::BadName);
                };
                if part.is_empty() || part.len() > 255 {
                    return Err(FileError::BadName);
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(FileError::Traversal),
            Component::RootDir => {
                if !parts.is_empty() {
                    return Err(FileError::BadName);
                }
                absolute = true;
            }
            Component::Prefix(_) => return Err(FileError::Outside),
        }
    }
    if parts.is_empty() || parts.len() > 32 {
        return Err(FileError::BadName);
    }
    Ok(ParsedPath { absolute, parts })
}

fn resolve_in(grant: &Grant, parsed: &ParsedPath, max: usize) -> Result<Resolved, FileError> {
    match fs::symlink_metadata(&grant.root) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(FileError::Symlink),
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(FileError::NotFound),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(FileError::NotFound);
        }
        Err(err) => return Err(FileError::Io(err.to_string())),
    }
    let rel = relative_parts(grant, parsed)?;
    if rel.is_empty() {
        return Err(FileError::BadName);
    }
    let mut cur = grant.root.clone();
    for part in &rel {
        cur.push(part);
        match fs::symlink_metadata(&cur) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(FileError::Symlink),
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FileError::NotFound);
            }
            Err(err) => return Err(FileError::Io(err.to_string())),
        }
    }
    let meta = fs::symlink_metadata(&cur).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            FileError::NotFound
        } else {
            FileError::Io(err.to_string())
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(FileError::Symlink);
    }
    if !meta.is_file() {
        return Err(FileError::NotFile);
    }
    if let Some(only) = &grant.only {
        if &cur != only {
            return Err(FileError::Outside);
        }
    }
    if meta.len() == 0 {
        return Err(FileError::Empty);
    }
    if meta.len() > max as u64 {
        return Err(FileError::TooLarge);
    }
    let name = cur
        .file_name()
        .and_then(|part| part.to_str())
        .ok_or(FileError::BadName)?;
    let name = sanitize_name(name)?;
    Ok(Resolved {
        path: cur,
        rel,
        name,
    })
}

fn relative_parts(grant: &Grant, parsed: &ParsedPath) -> Result<Vec<String>, FileError> {
    if let Some(only) = &grant.only {
        let candidate = if parsed.absolute {
            abs_path(&parsed.parts)
        } else {
            let mut path = grant.root.clone();
            for part in &parsed.parts {
                path.push(part);
            }
            path
        };
        if &candidate != only {
            return Err(FileError::Outside);
        }
        return normal_parts(
            only.strip_prefix(&grant.root)
                .map_err(|_| FileError::Outside)?,
        );
    }
    if parsed.absolute {
        let abs = abs_path(&parsed.parts);
        let suffix = abs
            .strip_prefix(&grant.root)
            .map_err(|_| FileError::Outside)?;
        normal_parts(suffix)
    } else {
        Ok(parsed.parts.clone())
    }
}

fn normal_parts(path: &Path) -> Result<Vec<String>, FileError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let Some(part) = part.to_str() else {
                    return Err(FileError::BadName);
                };
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(FileError::Traversal),
            Component::RootDir | Component::Prefix(_) => return Err(FileError::Outside),
        }
    }
    Ok(parts)
}

fn abs_path(parts: &[String]) -> PathBuf {
    let mut path = PathBuf::from("/");
    for part in parts {
        path.push(part);
    }
    path
}

fn rel_display(parts: &[String]) -> String {
    parts.join("/")
}

fn read_limited(path: &Path, max: usize) -> Result<Vec<u8>, FileError> {
    let mut file = open_nofollow(path)?;
    let mut bytes = Vec::new();
    Read::take(&mut file, (max as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| FileError::Io(err.to_string()))?;
    if bytes.len() > max {
        return Err(FileError::TooLarge);
    }
    if bytes.is_empty() {
        return Err(FileError::Empty);
    }
    Ok(bytes)
}

fn open_nofollow(path: &Path) -> Result<File, FileError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // libc::O_NOFOLLOW on Linux. A last-component swap cannot be followed.
        const O_NOFOLLOW: i32 = 0x20000;
        File::options()
            .read(true)
            .custom_flags(O_NOFOLLOW)
            .open(path)
            .map_err(|err| map_open_err(path, err))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let meta = fs::symlink_metadata(path).map_err(|err| map_open_err(path, err))?;
        if meta.file_type().is_symlink() {
            return Err(FileError::Symlink);
        }
        File::open(path).map_err(|err| map_open_err(path, err))
    }
}

fn map_open_err(path: &Path, err: std::io::Error) -> FileError {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return FileError::Symlink;
        }
    }
    if err.kind() == std::io::ErrorKind::NotFound {
        FileError::NotFound
    } else {
        FileError::Io(err.to_string())
    }
}

/// Extension allow-list plus the bytes that extension requires.
///
/// # Errors
/// [`FileError::Type`] or [`FileError::Magic`].
pub fn classify(name: &str, bytes: &[u8]) -> Result<&'static str, FileError> {
    let ext = extension(name).ok_or(FileError::Type)?;
    let spec = ALLOWED
        .iter()
        .find(|item| item.ext == ext)
        .ok_or(FileError::Type)?;
    if magic_ok(&ext, bytes) {
        Ok(spec.kind)
    } else {
        Err(FileError::Magic)
    }
}

fn extension(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let (_, ext) = base.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 8 || !ext.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

fn magic_ok(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "txt" | "md" | "json" | "csv" | "log" => {
            !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
        }
        "pdf" => bytes.starts_with(b"%PDF"),
        "jpg" | "jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn preview(bytes: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return String::new();
    };
    let mut end = text.len().min(MAX_PREVIEW);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Last path component, with controls and separators removed.
///
/// # Errors
/// [`FileError::BadName`] when nothing safe remains.
pub fn sanitize_name(raw: &str) -> Result<String, FileError> {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw).trim();
    let mut out = String::new();
    for ch in base.chars() {
        if out.chars().count() >= MAX_NAME {
            break;
        }
        if ch.is_control()
            || matches!(
                ch,
                '/' | '\\' | '"' | '<' | '>' | ':' | '*' | '?' | '|' | '{' | '}' | '\0'
            )
        {
            continue;
        }
        out.push(ch);
    }
    let out = out.trim().trim_start_matches('.').trim().to_string();
    if out.is_empty() || out == "." || out == ".." || out.contains('/') || out.contains('\\') {
        return Err(FileError::BadName);
    }
    Ok(out)
}

fn store(dir: &Path, id: &str, name: &str, kind: &str, bytes: &[u8]) -> Result<(), FileError> {
    if !id.bytes().all(|byte| byte.is_ascii_hexdigit()) || id.len() > 64 {
        return Err(FileError::Io("bad id".into()));
    }
    let path = dir.join(id);
    if path.parent() != Some(dir) {
        return Err(FileError::Outside);
    }
    write_private(&path, bytes)?;
    let line = format!(
        "{{\"id\":{},\"name\":{},\"kind\":{},\"size\":{}}}\n",
        json::string(id),
        json::string(name),
        json::string(kind),
        bytes.len()
    );
    if let Err(err) = append_private(&dir.join(INDEX_NAME), line.as_bytes()) {
        let _ = fs::remove_file(&path);
        return Err(err);
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let mut file = create_private(path)?;
    if let Err(err) = file.write_all(bytes) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(FileError::Io(err.to_string()));
    }
    Ok(())
}

fn create_private(path: &Path) -> Result<File, FileError> {
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|err| FileError::Io(err.to_string()))
}

fn append_private(path: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let mut options = File::options();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| FileError::Io(err.to_string()))?;
    file.write_all(bytes)
        .map_err(|err| FileError::Io(err.to_string()))
}

fn read_index(path: &Path) -> Vec<String> {
    let text = read_index_text(path);
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| {
            line.starts_with("{\"id\":")
                && line.len() <= 512
                && line.matches('{').count() == 1
                && line.matches('}').count() == 1
        })
        .map(str::to_string)
        .collect();
    if lines.len() > MAX_PICK {
        lines = lines.split_off(lines.len() - MAX_PICK);
    }
    lines
}

fn read_index_text(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let Ok(meta) = file.metadata() else {
        return String::new();
    };
    if meta.len() > MAX_INDEX {
        let skip = meta.len() - MAX_INDEX;
        if file.seek_read(skip).is_err() {
            return String::new();
        }
    }
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return String::new();
    }
    if meta.len() > MAX_INDEX {
        if let Some(pos) = buf.find('\n') {
            buf.drain(..=pos);
        }
    }
    buf
}

trait SeekRead {
    fn seek_read(&mut self, skip: u64) -> std::io::Result<()>;
}

impl SeekRead for File {
    fn seek_read(&mut self, skip: u64) -> std::io::Result<()> {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(skip))?;
        Ok(())
    }
}

/// Dial `addr` and `POST /files`, but only when `addr` is loopback.
///
/// # Errors
/// [`FileError::NotLoopback`] before any connect. [`FileError::Unreachable`]
/// when the intake does not answer. [`FileError::Pair`] when the body is a
/// pairing refusal. [`FileError::StackStatus`] for any other failure status.
pub fn forward(addr: SocketAddr, name: &str, bytes: &[u8]) -> Result<&'static str, FileError> {
    if !addr.ip().is_loopback() {
        return Err(FileError::NotLoopback);
    }
    let host = addr.to_string();
    let mut req = galion_common::http::ClientRequest::post(
        "/files",
        &host,
        "application/octet-stream",
        bytes,
    );
    req.headers
        .insert(1, ("X-File-Name".into(), name.to_string()));
    let resp = galion_common::http::send(addr, &req, Duration::from_secs(10), 64 * 1024)
        .map_err(|_| FileError::Unreachable)?;
    let body = resp.text();
    // Galion One answers 401 with this body when the console is not paired.
    // The pairing token is not sent and is not a command-line flag.
    if body.contains("\"error\":\"pair\"") {
        return Err(FileError::Pair);
    }
    if resp.status != 200 || body.contains("\"error\"") {
        return Err(FileError::StackStatus);
    }
    Ok("ok")
}

/// Pull `path` out of a JSON object. Other keys are ignored. Nested values
/// are refused.
///
/// # Errors
/// [`FileError::BadJson`] when `body` is not that object.
pub fn json_path(body: &[u8]) -> Result<String, FileError> {
    if body.len() > MAX_JSON {
        return Err(FileError::BadJson);
    }
    let text = std::str::from_utf8(body).map_err(|_| FileError::BadJson)?;
    let mut parser = Parser {
        bytes: text.as_bytes(),
        index: 0,
    };
    parser.skip_ws();
    parser.expect(b'{')?;
    parser.skip_ws();
    if parser.peek() == Some(b'}') {
        return Err(FileError::BadJson);
    }
    let mut found: Option<String> = None;
    loop {
        parser.skip_ws();
        let key = parser.string()?;
        parser.skip_ws();
        parser.expect(b':')?;
        parser.skip_ws();
        if key == "path" {
            if found.is_some() {
                return Err(FileError::BadJson);
            }
            found = Some(parser.string()?);
        } else {
            parser.skip_value()?;
        }
        parser.skip_ws();
        match parser.peek() {
            Some(b',') => {
                parser.bump();
            }
            Some(b'}') => {
                parser.bump();
                break;
            }
            _ => return Err(FileError::BadJson),
        }
    }
    parser.skip_ws();
    if parser.index != parser.bytes.len() {
        return Err(FileError::BadJson);
    }
    found.ok_or(FileError::BadJson)
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) {
        self.index += 1;
    }

    fn expect(&mut self, byte: u8) -> Result<(), FileError> {
        if self.peek() == Some(byte) {
            self.bump();
            Ok(())
        } else {
            Err(FileError::BadJson)
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    fn string(&mut self) -> Result<String, FileError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(FileError::BadJson);
            };
            self.bump();
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(esc) = self.peek() else {
                        return Err(FileError::BadJson);
                    };
                    self.bump();
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err(FileError::BadJson),
                    }
                }
                b if b < 0x20 => return Err(FileError::BadJson),
                _ => {
                    let width = utf8_width(byte).ok_or(FileError::BadJson)?;
                    if self.index + width - 1 > self.bytes.len() {
                        return Err(FileError::BadJson);
                    }
                    let start = self.index - 1;
                    let end = start + width;
                    if end > self.bytes.len() {
                        return Err(FileError::BadJson);
                    }
                    let text = std::str::from_utf8(&self.bytes[start..end])
                        .map_err(|_| FileError::BadJson)?;
                    out.push_str(text);
                    self.index = end;
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, FileError> {
        let mut hex = [0u8; 4];
        for slot in &mut hex {
            let Some(byte) = self.peek() else {
                return Err(FileError::BadJson);
            };
            self.bump();
            if !byte.is_ascii_hexdigit() {
                return Err(FileError::BadJson);
            }
            *slot = byte;
        }
        let text = std::str::from_utf8(&hex).map_err(|_| FileError::BadJson)?;
        let code = u32::from_str_radix(text, 16).map_err(|_| FileError::BadJson)?;
        char::from_u32(code).ok_or(FileError::BadJson)
    }

    fn skip_value(&mut self) -> Result<(), FileError> {
        match self.peek() {
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(FileError::BadJson),
        }
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), FileError> {
        for byte in word {
            self.expect(*byte)?;
        }
        Ok(())
    }

    fn number(&mut self) -> Result<(), FileError> {
        if self.peek() == Some(b'-') {
            self.bump();
        }
        let start = self.index;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        if self.index == start {
            return Err(FileError::BadJson);
        }
        if self.peek() == Some(b'.') {
            self.bump();
            let frac = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
            if self.index == frac {
                return Err(FileError::BadJson);
            }
        }
        Ok(())
    }
}

fn utf8_width(first: u8) -> Option<usize> {
    if first.is_ascii() {
        Some(1)
    } else if first & 0xE0 == 0xC0 {
        Some(2)
    } else if first & 0xF0 == 0xE0 {
        Some(3)
    } else if first & 0xF8 == 0xF0 {
        Some(4)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn scratch() -> PathBuf {
        let mut buf = [0u8; 8];
        let mut random = File::open("/dev/urandom").unwrap();
        random.read_exact(&mut buf).unwrap();
        let dir =
            std::env::temp_dir().join(format!("om-desk-{}", galion_common::hex::encode(&buf)));
        fs::create_dir(&dir).unwrap();
        dir
    }

    struct Tmp {
        path: PathBuf,
    }

    impl Tmp {
        fn new() -> Self {
            Self { path: scratch() }
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn limited(grants: &[PathBuf], inbox: Option<&Path>, stack: Option<&str>, max: usize) -> Files {
        Files::open_limited(grants, inbox, stack, max).unwrap()
    }

    #[test]
    fn allow_list_matches_magic() {
        assert!(classify("notes.txt", b"hello").is_ok());
        assert!(classify("NOTES.TXT", b"hello").is_ok());
        assert_eq!(classify("run.exe", b"hello").unwrap_err().code(), "type");
        assert_eq!(classify("page.html", b"<p>").unwrap_err().code(), "type");
        assert_eq!(classify("pic.svg", b"<svg/>").unwrap_err().code(), "type");
        assert_eq!(
            classify("notes.pdf", b"not a pdf").unwrap_err().code(),
            "magic"
        );
        assert!(classify("notes.pdf", b"%PDF-1.7\n").is_ok());
        assert_eq!(classify("a.txt", b"has\0nul").unwrap_err().code(), "magic");
        assert!(classify("a.jpg", &[0xFF, 0xD8, 0xFF, 0xD9]).is_ok());
        assert!(classify("a.png", b"\x89PNG\r\n\x1a\nrest").is_ok());
        assert!(classify("a.webp", b"RIFF\0\0\0\0WEBP").is_ok());
        assert_eq!(classify("a.jpg", b"not-jpeg").unwrap_err().code(), "magic");
        assert!(accept_list().contains(".pdf"));
        assert!(!accept_list().contains(".html"));
    }

    #[test]
    fn sanitize_drops_the_directory() {
        assert_eq!(sanitize_name("../../etc/passwd.txt").unwrap(), "passwd.txt");
        assert_eq!(sanitize_name("..\\..\\passwd.txt").unwrap(), "passwd.txt");
        assert_eq!(
            sanitize_name("passwd.txt/..").unwrap_err().code(),
            "bad-name"
        );
        assert_eq!(sanitize_name("").unwrap_err().code(), "bad-name");
        assert_eq!(sanitize_name("a\nb.txt").unwrap(), "ab.txt");
    }

    #[test]
    fn json_path_reads_one_string() {
        assert_eq!(json_path(br#"{"path":"notes.txt"}"#).unwrap(), "notes.txt");
        assert_eq!(
            json_path(br#"{"extra":"no","path":"sub\/notes.txt"}"#).unwrap(),
            "sub/notes.txt"
        );
        assert_eq!(json_path(br#"{"path":"a\u002eb"}"#).unwrap(), "a.b");
        assert_eq!(json_path(br#"{"path":1}"#).unwrap_err().code(), "bad-json");
        assert_eq!(
            json_path(br#"{"path":"a","path":"b"}"#).unwrap_err().code(),
            "bad-json"
        );
        assert_eq!(
            json_path(br#"{"path":{"a":1}}"#).unwrap_err().code(),
            "bad-json"
        );
    }

    #[test]
    fn read_stays_inside_the_chosen_directory() {
        let tmp = Tmp::new();
        let grant = tmp.path.join("chosen");
        fs::create_dir(&grant).unwrap();
        fs::write(grant.join("notes.txt"), b"inside").unwrap();
        fs::write(grant.join("skip.exe"), b"nope").unwrap();
        fs::write(tmp.path.join("secret.txt"), b"SENTINEL-OUTSIDE").unwrap();
        let nested = grant.join("sub");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("deep.txt"), b"deep").unwrap();
        let files = limited(std::slice::from_ref(&grant), None, None, 64);

        let got = files.read_path("notes.txt").unwrap();
        assert_eq!(got.text.as_deref(), Some("inside"));
        assert_eq!(got.path, "notes.txt");
        let deep = files.read_path("sub/deep.txt").unwrap();
        assert_eq!(deep.text.as_deref(), Some("deep"));

        for bad in ["../secret.txt", "sub/../../secret.txt", "/etc/passwd"] {
            let err = files.read_path(bad).unwrap_err();
            assert!(
                err.code() == "traversal" || err.code() == "outside",
                "{bad} -> {}",
                err.code()
            );
            assert!(!err.to_string().contains("SENTINEL"));
        }
        let abs_out = tmp.path.join("secret.txt");
        let err = files.read_path(&abs_out.display().to_string()).unwrap_err();
        assert_eq!(err.code(), "outside");
        assert_eq!(files.read_path("skip.exe").unwrap_err().code(), "type");
        assert_eq!(
            files.read_path("missing.txt").unwrap_err().code(),
            "not-found"
        );
        assert!(files.pick().iter().any(|item| item.name == "notes.txt"));
        assert!(files.pick().iter().all(|item| item.name != "skip.exe"));
        assert!(files.pick().iter().all(|item| item.path != "secret.txt"));
    }

    #[test]
    fn a_chosen_file_does_not_open_its_neighbour() {
        let tmp = Tmp::new();
        let chosen = tmp.path.join("only.txt");
        let other = tmp.path.join("other.txt");
        fs::write(&chosen, b"chosen").unwrap();
        fs::write(&other, b"SENTINEL-NEIGHBOUR").unwrap();
        let files = limited(std::slice::from_ref(&chosen), None, None, 64);
        assert_eq!(
            files.read_path("only.txt").unwrap().text.as_deref(),
            Some("chosen")
        );
        let err = files.read_path("other.txt").unwrap_err();
        assert_eq!(err.code(), "outside");
        assert!(!err.to_string().contains("SENTINEL"));
        let listed = files.pick();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "only.txt");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        use std::os::unix::fs::symlink;
        let tmp = Tmp::new();
        let grant = tmp.path.join("chosen");
        fs::create_dir(&grant).unwrap();
        let outside = tmp.path.join("secret.txt");
        fs::write(&outside, b"SENTINEL-LINK").unwrap();
        symlink(&outside, grant.join("link.txt")).unwrap();
        symlink(tmp.path.join("nope"), grant.join("dangling.txt")).unwrap();
        let via = tmp.path.join("via");
        symlink(&grant, &via).unwrap();
        assert!(Files::open_limited(std::slice::from_ref(&via), None, None, 64).is_err());
        let files = limited(&[grant], None, None, 64);
        for name in ["link.txt", "dangling.txt"] {
            let err = files.read_path(name).unwrap_err();
            assert_eq!(err.code(), "symlink", "{name}");
            assert!(!err.to_string().contains("SENTINEL"));
        }
        assert!(files.pick().iter().all(|item| item.name != "link.txt"));
    }

    #[test]
    fn oversize_is_refused_before_upload() {
        let tmp = Tmp::new();
        let grant = tmp.path.join("chosen");
        fs::create_dir(&grant).unwrap();
        fs::write(grant.join("big.txt"), b"0123456789").unwrap();
        let inbox = tmp.path.join("inbox");
        let files = limited(&[grant], Some(&inbox), Some("127.0.0.1:1"), 4);
        let err = files.upload_path("big.txt").unwrap_err();
        assert_eq!(err.code(), "too-large");
        assert!(!inbox.exists() || fs::read_dir(&inbox).unwrap().next().is_none());
    }

    #[test]
    fn upload_stores_a_safe_name_and_dials_loopback() {
        let tmp = Tmp::new();
        let inbox = tmp.path.join("inbox");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match sock.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(split) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..split]).to_ascii_lowercase();
                            let len = head
                                .split("content-length:")
                                .nth(1)
                                .and_then(|rest| {
                                    rest.trim_start()
                                        .chars()
                                        .take_while(|c| c.is_ascii_digit())
                                        .collect::<String>()
                                        .parse::<usize>()
                                        .ok()
                                })
                                .unwrap_or(0);
                            if buf.len() >= split + 4 + len {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = sock.write_all(
                b"HTTP/1.0 200 OK\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"id\":\"ok\"}",
            );
            let _ = tx.send(buf);
        });
        let files = limited(&[], Some(&inbox), Some(&addr.to_string()), 64);
        let receipt = files
            .upload_bytes("../../etc/passwd.txt", b"hello")
            .unwrap();
        assert!(receipt.stored);
        assert_eq!(receipt.stack, "ok");
        assert_eq!(receipt.name, "passwd.txt");
        let id = receipt.id.unwrap();
        assert_eq!(fs::read(inbox.join(&id)).unwrap(), b"hello");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = fs::metadata(inbox.join(&id)).unwrap().permissions().mode();
            assert_eq!(file_mode & 0o777, 0o600);
            let dir_mode = fs::metadata(&inbox).unwrap().permissions().mode();
            assert_eq!(dir_mode & 0o777, 0o700);
        }
        assert!(!inbox.join("passwd.txt").exists());
        assert!(!inbox.join("etc").exists());
        let wire = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let text = String::from_utf8_lossy(&wire);
        assert!(text.contains("POST /files "));
        assert!(text
            .to_ascii_lowercase()
            .contains("x-file-name: passwd.txt"));
        assert!(text.contains("hello"));
        let sent = files.sent_json();
        assert!(sent.contains("passwd.txt"));
        assert!(!sent.contains("SENTINEL"));
    }

    #[test]
    fn unpaired_intake_is_pair_even_on_401() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(
                b"HTTP/1.0 401 Unauthorized\r\ncontent-length: 16\r\nconnection: close\r\n\r\n{\"error\":\"pair\"}",
            );
        });
        let err = forward(addr, "notes.txt", b"hello").unwrap_err();
        assert_eq!(err.code(), "pair");
    }

    #[test]
    fn non_loopback_stack_is_refused_without_dialling() {
        let addr: SocketAddr = "192.0.2.1:9".parse().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = forward(addr, "a.txt", b"hi").map(|ok| ok.to_string());
            let _ = tx.send(result.map_err(|err| err.code().to_string()));
        });
        let got = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("forward hung on a non-loopback address");
        assert_eq!(got, Err("not-loopback".into()));
        assert!(Files::open(&[], None, Some("10.1.2.3:8091")).is_err());
        assert!(Files::open(&[], None, Some(DEFAULT_STACK)).is_ok());
    }

    #[test]
    fn no_destination_does_not_invent_one() {
        let files = Files::closed();
        assert_eq!(
            files.upload_bytes("a.txt", b"hi").unwrap_err().code(),
            "no-destination"
        );
        assert!(files.pick().is_empty());
        assert_eq!(files.sent_json(), "{\"sent\":[]}");
    }
}
