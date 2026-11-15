// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only Obsidian vault access.
//!
//! The vault root is canonicalised once at start. Every note path from a
//! tool call is a *relative* path that must stay inside that root:
//! absolute paths, `..`, `.` and hidden components (`.git`, `.obsidian`)
//! are refused before the file system is touched, and the canonical path
//! of the target must still start with the canonical root (so a symlink
//! that points outside is refused too). Only note files are read
//! (`.md`, `.markdown`, `.txt`). There is no write, rename or delete code
//! in this module.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Default and hard maximum bytes returned by `vault_read`.
pub const READ_DEFAULT_BYTES: usize = 32 * 1024;
/// Hard maximum bytes returned by `vault_read`.
pub const READ_MAX_BYTES: usize = 64 * 1024;
/// Most entries `vault_list` returns.
pub const LIST_MAX: usize = 1000;
/// Most files `vault_search` opens in one call.
pub const SEARCH_MAX_FILES: usize = 5000;
/// Largest single file `vault_search` scans (bigger files are skipped).
pub const SEARCH_MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Total bytes `vault_search` scans in one call.
pub const SEARCH_MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
/// Most hits `vault_search` returns.
pub const SEARCH_MAX_HITS: usize = 50;
/// Deepest directory level walked.
pub const MAX_DEPTH: usize = 12;
/// Longest relative path accepted.
pub const MAX_PATH_LEN: usize = 512;

const NOTE_EXTS: [&str; 3] = ["md", "markdown", "txt"];

/// A read-only vault rooted at a canonical directory.
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

/// One listed note: relative path, size in bytes, modified unix seconds.
pub type NoteEntry = (String, u64, u64);

/// One search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Note path relative to the vault root, `/` separated.
    pub path: String,
    /// 1-based line number.
    pub line: usize,
    /// The matching line, trimmed and cut to 240 chars.
    pub text: String,
}

/// Result of a search.
#[derive(Debug, Clone, Default)]
pub struct SearchResult {
    /// Hits in walk order (sorted paths).
    pub hits: Vec<Hit>,
    /// Files opened.
    pub files_scanned: usize,
    /// True when a limit (hits, files, bytes) stopped the scan early.
    pub truncated: bool,
}

impl Vault {
    /// Open a vault. The root must be an existing directory and must not be `/`.
    ///
    /// # Errors
    /// The root does not exist, is not a directory, or is the file-system root.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = fs::canonicalize(root.as_ref())
            .map_err(|e| format!("vault root {}: {e}", root.as_ref().display()))?;
        if !root.is_dir() {
            return Err(format!("vault root {} is not a directory", root.display()));
        }
        if root.parent().is_none() {
            return Err("vault root must not be /".into());
        }
        Ok(Vault { root })
    }

    /// Canonical root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Check a relative path from a tool call and return its canonical
    /// absolute path inside the vault.
    ///
    /// # Errors
    /// The path is empty, too long, absolute, has `..`/`.`/hidden parts,
    /// does not exist, or resolves outside the root.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        let rel = rel.trim();
        if rel.is_empty() {
            return Err("path is empty".into());
        }
        if rel.len() > MAX_PATH_LEN {
            return Err("path is too long".into());
        }
        if rel.contains('\0') || rel.contains('\\') {
            return Err("path has a forbidden character".into());
        }
        let p = Path::new(rel);
        if p.is_absolute() {
            return Err("path must be relative to the vault root".into());
        }
        for c in p.components() {
            match c {
                Component::Normal(s) => {
                    if s.to_string_lossy().starts_with('.') {
                        return Err("hidden files and folders are not readable".into());
                    }
                }
                _ => return Err("path must not contain '..', '.' or a root".into()),
            }
        }
        let joined = self.root.join(p);
        let canon = fs::canonicalize(&joined).map_err(|_| "no such note".to_string())?;
        if !canon.starts_with(&self.root) {
            return Err("path leaves the vault".into());
        }
        Ok(canon)
    }

    /// Read one note, at most `max_bytes` (clamped to [`READ_MAX_BYTES`]).
    /// Returns the text (lossy UTF-8), the file size and whether it was cut.
    ///
    /// # Errors
    /// Bad path, not a regular note file, or an I/O error.
    pub fn read(&self, rel: &str, max_bytes: usize) -> Result<(String, u64, bool), String> {
        let path = self.resolve(rel)?;
        let meta = fs::metadata(&path).map_err(|e| format!("stat: {e}"))?;
        if !meta.is_file() {
            return Err("not a file".into());
        }
        if !is_note(&path) {
            return Err("only .md, .markdown and .txt notes can be read".into());
        }
        let cap = max_bytes.clamp(1, READ_MAX_BYTES);
        let mut buf = Vec::with_capacity(cap.min(meta.len() as usize) + 1);
        File::open(&path)
            .map_err(|e| format!("open: {e}"))?
            .take(cap as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("read: {e}"))?;
        let cut = buf.len() > cap;
        buf.truncate(cap);
        Ok((String::from_utf8_lossy(&buf).into_owned(), meta.len(), cut))
    }

    /// List notes under `dir` (relative, empty = root), sorted, at most `limit`.
    /// Returns (path, size, modified unix seconds) and whether it was cut.
    ///
    /// # Errors
    /// Bad directory path.
    pub fn list(&self, dir: &str, limit: usize) -> Result<(Vec<NoteEntry>, bool), String> {
        let start = if dir.trim().is_empty() {
            self.root.clone()
        } else {
            self.resolve(dir)?
        };
        if !start.is_dir() {
            return Err("not a folder".into());
        }
        let limit = limit.clamp(1, LIST_MAX);
        let mut out = Vec::new();
        let mut truncated = false;
        self.walk(&start, 0, &mut |path, meta| {
            if out.len() >= limit {
                truncated = true;
                return false;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            out.push((self.rel(path), meta.len(), mtime));
            true
        });
        Ok((out, truncated))
    }

    /// Case-insensitive substring search over note text.
    ///
    /// # Errors
    /// The query is shorter than 2 or longer than 200 characters.
    pub fn search(&self, query: &str, limit: usize) -> Result<SearchResult, String> {
        let q = query.trim().to_lowercase();
        let n = q.chars().count();
        if !(2..=200).contains(&n) {
            return Err("query must be 2 to 200 characters".into());
        }
        let limit = limit.clamp(1, SEARCH_MAX_HITS);
        let mut res = SearchResult::default();
        let mut total: u64 = 0;
        self.walk(&self.root.clone(), 0, &mut |path, meta| {
            if res.hits.len() >= limit
                || res.files_scanned >= SEARCH_MAX_FILES
                || total >= SEARCH_MAX_TOTAL_BYTES
            {
                res.truncated = true;
                return false;
            }
            if meta.len() > SEARCH_MAX_FILE_BYTES {
                return true;
            }
            let mut buf = Vec::new();
            let ok = File::open(path)
                .and_then(|f| f.take(SEARCH_MAX_FILE_BYTES).read_to_end(&mut buf))
                .is_ok();
            if !ok {
                return true;
            }
            res.files_scanned += 1;
            total += buf.len() as u64;
            let text = String::from_utf8_lossy(&buf);
            let rel = self.rel(path);
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&q) {
                    res.hits.push(Hit {
                        path: rel.clone(),
                        line: i + 1,
                        text: line.trim().chars().take(240).collect(),
                    });
                    if res.hits.len() >= limit {
                        res.truncated = true;
                        return false;
                    }
                }
            }
            true
        });
        Ok(res)
    }

    /// Count of notes in the whole vault (bounded by [`SEARCH_MAX_FILES`]).
    #[must_use]
    pub fn note_count(&self) -> usize {
        let mut n = 0;
        self.walk(&self.root.clone(), 0, &mut |_, _| {
            n += 1;
            n < SEARCH_MAX_FILES
        });
        n
    }

    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Depth-first walk in sorted order over note files. Hidden entries and
    /// symlinks are skipped (never followed). `f` returns false to stop.
    fn walk(
        &self,
        dir: &Path,
        depth: usize,
        f: &mut dyn FnMut(&Path, &fs::Metadata) -> bool,
    ) -> bool {
        if depth > MAX_DEPTH {
            return true;
        }
        let Ok(rd) = fs::read_dir(dir) else {
            return true;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(e.path()) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            let path = e.path();
            if meta.is_dir() {
                if !self.walk(&path, depth + 1, f) {
                    return false;
                }
            } else if meta.is_file() && is_note(&path) && !f(&path, &meta) {
                return false;
            }
        }
        true
    }
}

fn is_note(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| NOTE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}
