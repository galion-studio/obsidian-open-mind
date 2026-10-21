// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Startup index of the built console (`dist/client`).
//!
//! The tree is walked once. A file is listed only when it is a regular file
//! (symlinks are skipped, never followed), every path segment passes
//! [`crate::path::sanitize`] (so no dotfile or dot directory is ever indexed)
//! and its extension has a known content type. Requests are answered from
//! this map only; a request path is never joined onto the root.

use crate::mime;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Maximum number of files indexed (the console build has ~40).
pub const MAX_FILES: usize = 10_000;
/// Maximum directory depth walked.
pub const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone)]
pub struct Entry {
    pub file: PathBuf,
    pub len: u64,
    pub content_type: &'static str,
    pub cache_control: &'static str,
    pub etag: String,
}

#[derive(Debug, Default)]
pub struct Site {
    files: HashMap<String, Entry>,
    shell: Option<Entry>,
    skipped: usize,
}

fn etag_for(meta: &fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("\"{:x}-{:x}\"", meta.len(), mtime)
}

impl Site {
    /// Index `root`. `shell` is the HTML shell file name relative to root
    /// (e.g. `_shell.html`); it is served only for page routes, never at its
    /// own URL. Fails when root is missing or the shell is missing.
    pub fn load(root: &Path, shell: &str) -> io::Result<Site> {
        // The root itself may be a `current -> <rev>` symlink; resolve it once.
        // Symlinks *inside* the tree are never followed.
        let root = fs::canonicalize(root)?;
        let meta = fs::metadata(&root)?;
        if !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "root is not a directory",
            ));
        }
        let mut site = Site::default();
        site.walk(&root, String::new(), 0)?;
        let shell_key = format!("/{}", shell.trim_start_matches('/'));
        let shell_entry = site.files.remove(&shell_key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("shell {shell_key} not found under root"),
            )
        })?;
        if !shell_entry.content_type.starts_with("text/html") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "shell must be .html",
            ));
        }
        site.shell = Some(Entry {
            cache_control: "no-cache",
            ..shell_entry
        });
        Ok(site)
    }

    fn walk(&mut self, dir: &Path, prefix: String, depth: usize) -> io::Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        let mut entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = match e.file_name().into_string() {
                Ok(n) => n,
                Err(_) => {
                    self.skipped += 1;
                    continue;
                }
            };
            let url = format!("{prefix}/{name}");
            // One segment must be a clean, non-dot segment.
            if name.is_empty()
                || name.starts_with('.')
                || crate::path::sanitize(&url).as_deref() != Ok(url.as_str())
            {
                self.skipped += 1;
                continue;
            }
            let ft = match e.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                self.skipped += 1;
            } else if ft.is_dir() {
                self.walk(&e.path(), url, depth + 1)?;
            } else if ft.is_file() {
                let Some(ct) = mime::content_type(&name) else {
                    self.skipped += 1;
                    continue;
                };
                if self.files.len() >= MAX_FILES {
                    return Err(io::Error::other("too many files under root"));
                }
                let meta = e.metadata()?;
                let entry = Entry {
                    file: e.path(),
                    len: meta.len(),
                    content_type: ct,
                    cache_control: mime::cache_control(&url, ct),
                    etag: etag_for(&meta),
                };
                self.files.insert(url, entry);
            } else {
                self.skipped += 1;
            }
        }
        Ok(())
    }

    /// File for an exact sanitized URL path.
    pub fn file(&self, url_path: &str) -> Option<&Entry> {
        self.files.get(url_path)
    }

    /// The HTML shell (always present after a successful load).
    pub fn shell(&self) -> Option<&Entry> {
        self.shell.as_ref()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Entries skipped at load (dotfiles, symlinks, unknown types).
    pub fn skipped(&self) -> usize {
        self.skipped
    }
}
