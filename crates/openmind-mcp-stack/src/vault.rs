// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only Obsidian vault access.
//!
//! Every path a caller gives is relative to the vault root. A path is refused
//! when it is absolute, has a `..` component, has any component that starts
//! with `.` (`.git`, `.obsidian`, hidden files), or crosses a symlink at any
//! level. After the checks the canonical path must still be inside the
//! canonical root, and the opened file must be the same inode that was
//! checked. Only regular `*.md` files are read. Nothing here writes: files are
//! opened with `File::open` (read-only) and no create/rename/remove call
//! exists in this module.

use serde_json::{json, Value};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

/// Max directory depth walked under the root.
pub const MAX_DEPTH: usize = 16;
/// Max directory entries looked at per walk (bounds a huge or hostile tree).
pub const MAX_ENTRIES: usize = 20_000;
/// Max bytes read from one note by `vault_read`.
pub const MAX_READ_BYTES: usize = 256 * 1024;
/// Default bytes read from one note by `vault_read`.
pub const DEFAULT_READ_BYTES: usize = 64 * 1024;
/// Notes bigger than this are skipped by `vault_search`.
pub const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;

/// Why a vault request was refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// The path breaks a rule (traversal, hidden, symlink, not a note).
    Refused(String),
    /// The path is fine but does not exist.
    NotFound(String),
    /// I/O failure.
    Io(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::Refused(m) => write!(f, "refused: {m}"),
            VaultError::NotFound(m) => write!(f, "not found: {m}"),
            VaultError::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for VaultError {}

/// One note in a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteInfo {
    /// Path relative to the root, `/`-separated.
    pub path: String,
    pub bytes: u64,
    /// Last modification, Europe/Zagreb RFC 3339.
    pub modified: String,
}

impl NoteInfo {
    pub fn to_json(&self) -> Value {
        json!({"path": self.path, "bytes": self.bytes, "modified": self.modified})
    }
}

/// One search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    /// 1-based line number; 0 = the file name matched.
    pub line: usize,
    pub snippet: String,
}

/// A read-only vault rooted at a canonical directory.
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

fn modified_zagreb(md: &fs::Metadata) -> String {
    zagreb_time::from_unix(md.mtime())
}

fn is_note(name: &str) -> bool {
    name.len() > 3 && name.to_ascii_lowercase().ends_with(".md")
}

impl Vault {
    /// Open a vault. The root must be an existing directory; it is
    /// canonicalised once here (a symlinked root is followed once, on purpose,
    /// by the operator who configured it).
    pub fn open(root: impl AsRef<Path>) -> Result<Vault, VaultError> {
        let root = root.as_ref();
        let canon = fs::canonicalize(root)
            .map_err(|e| VaultError::Io(format!("vault root {}: {e}", root.display())))?;
        if !canon.is_dir() {
            return Err(VaultError::Io(format!(
                "vault root {} is not a directory",
                root.display()
            )));
        }
        Ok(Vault { root: canon })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Check a caller path and return the absolute path inside the root.
    /// `""` and `"."` mean the root itself.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, VaultError> {
        if rel.len() > 1024 {
            return Err(VaultError::Refused("path longer than 1024 bytes".into()));
        }
        if rel.contains('\0') {
            return Err(VaultError::Refused("path contains NUL".into()));
        }
        if rel.contains('\\') {
            return Err(VaultError::Refused("backslash in path".into()));
        }
        let p = Path::new(rel);
        if p.is_absolute() {
            return Err(VaultError::Refused(
                "absolute paths are not allowed; give a path relative to the vault".into(),
            ));
        }
        let mut cur = self.root.clone();
        for c in p.components() {
            match c {
                Component::CurDir => continue,
                Component::ParentDir => {
                    return Err(VaultError::Refused("'..' is not allowed".into()))
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(VaultError::Refused("absolute paths are not allowed".into()))
                }
                Component::Normal(n) => {
                    let s = n.to_string_lossy();
                    if s.starts_with('.') {
                        return Err(VaultError::Refused(format!(
                            "hidden path component '{s}' (.git, .obsidian and dot files are not served)"
                        )));
                    }
                    cur.push(n);
                    match fs::symlink_metadata(&cur) {
                        Ok(md) if md.file_type().is_symlink() => {
                            return Err(VaultError::Refused(format!("'{s}' is a symlink")))
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            return Err(VaultError::NotFound(rel.to_string()))
                        }
                        Err(e) => return Err(VaultError::Io(e.to_string())),
                    }
                }
            }
        }
        let canon = fs::canonicalize(&cur).map_err(|e| VaultError::Io(e.to_string()))?;
        if !canon.starts_with(&self.root) {
            return Err(VaultError::Refused("path leaves the vault".into()));
        }
        Ok(canon)
    }

    fn rel_of(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.root)
            .unwrap_or(abs)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Walk notes under `dir` (relative), sorted by path. Skips dot entries
    /// and symlinks; never follows a link.
    pub fn list(&self, dir: &str, limit: usize) -> Result<(Vec<NoteInfo>, bool), VaultError> {
        let start = self.resolve(dir)?;
        let md = fs::symlink_metadata(&start).map_err(|e| VaultError::Io(e.to_string()))?;
        if !md.is_dir() {
            return Err(VaultError::Refused(format!("{dir} is not a folder")));
        }
        let mut out = Vec::new();
        let mut seen = 0usize;
        let mut truncated = false;
        let mut stack = vec![(start, 0usize)];
        while let Some((d, depth)) = stack.pop() {
            let Ok(rd) = fs::read_dir(&d) else { continue };
            let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                seen += 1;
                if seen > MAX_ENTRIES {
                    truncated = true;
                    break;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(md) = fs::symlink_metadata(e.path()) else {
                    continue;
                };
                let ft = md.file_type();
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    if depth + 1 < MAX_DEPTH {
                        stack.push((e.path(), depth + 1));
                    }
                } else if ft.is_file() && is_note(&name) {
                    out.push(NoteInfo {
                        path: self.rel_of(&e.path()),
                        bytes: md.len(),
                        modified: modified_zagreb(&md),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        if out.len() > limit {
            out.truncate(limit);
            truncated = true;
        }
        Ok((out, truncated))
    }

    /// Read one note (read-only open). Returns (info, text, truncated).
    pub fn read(
        &self,
        rel: &str,
        max_bytes: usize,
    ) -> Result<(NoteInfo, String, bool), VaultError> {
        if !is_note(rel.rsplit('/').next().unwrap_or("")) {
            return Err(VaultError::Refused("only .md notes can be read".into()));
        }
        let abs = self.resolve(rel)?;
        let md = fs::symlink_metadata(&abs).map_err(|e| VaultError::Io(e.to_string()))?;
        if !md.file_type().is_file() {
            return Err(VaultError::Refused("not a regular file".into()));
        }
        let mut f = File::open(&abs).map_err(|e| VaultError::Io(e.to_string()))?;
        let fmd = f.metadata().map_err(|e| VaultError::Io(e.to_string()))?;
        if fmd.dev() != md.dev() || fmd.ino() != md.ino() {
            return Err(VaultError::Refused("file changed while opening".into()));
        }
        let max = max_bytes.clamp(1, MAX_READ_BYTES);
        let mut buf = Vec::with_capacity(max.min(fmd.len() as usize + 1));
        (&mut f)
            .take(max as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| VaultError::Io(e.to_string()))?;
        let truncated = buf.len() > max;
        buf.truncate(max);
        let text = String::from_utf8_lossy(&buf).into_owned();
        Ok((
            NoteInfo {
                path: self.rel_of(&abs),
                bytes: fmd.len(),
                modified: modified_zagreb(&fmd),
            },
            text,
            truncated,
        ))
    }

    /// Case-insensitive substring search over note names and lines.
    pub fn search(&self, query: &str, limit: usize) -> Result<(Vec<Hit>, bool), VaultError> {
        let q = query.trim().to_lowercase();
        if q.chars().count() < 2 {
            return Err(VaultError::Refused(
                "query needs at least 2 characters".into(),
            ));
        }
        let (notes, mut truncated) = self.list("", usize::MAX)?;
        let mut hits = Vec::new();
        'notes: for n in notes {
            if n.path.to_lowercase().contains(&q) {
                hits.push(Hit {
                    path: n.path.clone(),
                    line: 0,
                    snippet: n.path.clone(),
                });
                if hits.len() >= limit {
                    truncated = true;
                    break;
                }
            }
            if n.bytes > MAX_SEARCH_FILE_BYTES {
                continue;
            }
            let Ok((_, text, _)) = self.read(&n.path, MAX_SEARCH_FILE_BYTES as usize) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&q) {
                    let snippet: String = line.trim().chars().take(240).collect();
                    hits.push(Hit {
                        path: n.path.clone(),
                        line: i + 1,
                        snippet,
                    });
                    if hits.len() >= limit {
                        truncated = true;
                        break 'notes;
                    }
                }
            }
        }
        Ok((hits, truncated))
    }
}
