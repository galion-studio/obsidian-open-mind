// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `vault_note_create`: create a **new** note in one inbox folder of the
//! vault. **Off by default.**
//!
//! It is only listed when the stack config file sets
//! `vault_note_create = on` (and the owner has said yes). Even then:
//! - it can only create a new file (`create_new`, so an existing note is
//!   never opened for writing, truncated, renamed or deleted);
//! - only inside `<vault>/<folder>` (default `hermes-inbox`), file name
//!   made from a slug of the title plus a Europe/Zagreb timestamp, `.md`;
//! - the inbox folder must already exist and must not be a symlink;
//! - text is capped at [`MAX_NOTE_CHARS`] and is passed through
//!   [`crate::redact::text`] first so a secret cannot be written into the vault.
//!
//! The live hermes-rs unit also mounts the vault read-only and the
//! hermes deny list refuses vault-write tool names, so turning this on
//! needs those two changes as well (owner decision).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Longest note body accepted.
pub const MAX_NOTE_CHARS: usize = 16_000;
/// Default inbox folder (relative to the vault root).
pub const DEFAULT_FOLDER: &str = "hermes-inbox";

/// A switched-on note creator.
#[derive(Debug, Clone)]
pub struct NoteCreator {
    dir: PathBuf,
    folder: String,
}

impl NoteCreator {
    /// Inbox `<vault_root>/<folder>`; the folder must exist, be a real
    /// directory (not a symlink) and be one plain path component.
    ///
    /// # Errors
    /// Bad folder name, missing folder, or a symlink.
    pub fn new(vault_root: &Path, folder: &str) -> Result<Self, String> {
        let folder = folder.trim();
        if folder.is_empty()
            || folder.starts_with('.')
            || !folder
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("note folder must be one plain name (letters, digits, '-', '_')".into());
        }
        let dir = vault_root.join(folder);
        let meta = fs::symlink_metadata(&dir)
            .map_err(|_| format!("note folder {folder} does not exist"))?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(format!("note folder {folder} is not a real folder"));
        }
        Ok(NoteCreator {
            dir,
            folder: folder.into(),
        })
    }

    /// Create one new note. Returns its path relative to the vault root.
    ///
    /// # Errors
    /// Empty title/text, text too long, the file already exists, I/O error.
    pub fn create(&self, title: &str, text: &str, stamp: &str) -> Result<String, String> {
        let title = title.trim();
        let text = text.trim();
        if title.is_empty() || text.is_empty() {
            return Err("title and text are required".into());
        }
        if text.chars().count() > MAX_NOTE_CHARS {
            return Err(format!("text is longer than {MAX_NOTE_CHARS} characters"));
        }
        let slug = slug(title);
        let ts: String = stamp
            .chars()
            .take(19)
            .map(|c| if c.is_ascii_digit() { c } else { '-' })
            .collect();
        let name = format!("{ts}-{slug}.md");
        let path = self.dir.join(&name);
        let body = format!(
            "---\ncreated: {stamp}\nsource: hermes (openmind MCP vault_note_create)\n---\n# {}\n\n{}\n",
            crate::redact::text(title),
            crate::redact::text(text)
        );
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("cannot create {name}: {e}"))?;
        f.write_all(body.as_bytes())
            .and_then(|()| f.sync_all())
            .map_err(|e| format!("write {name}: {e}"))?;
        Ok(format!("{}/{name}", self.folder))
    }
}

fn slug(title: &str) -> String {
    let mut s = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
        if s.len() >= 48 {
            break;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "note".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_only_new_files_in_the_inbox() {
        let root = std::env::temp_dir().join(format!("omcp-notes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("hermes-inbox")).unwrap();
        assert!(NoteCreator::new(&root, "../x").is_err());
        assert!(NoteCreator::new(&root, "missing").is_err());
        let n = NoteCreator::new(&root, "hermes-inbox").unwrap();
        let stamp = "2026-10-30T12:00:00+01:00";
        let rel = n
            .create("Stack: check!", "token TOKEN=abc12345 here", stamp)
            .unwrap();
        assert_eq!(rel, "hermes-inbox/2026-10-30-12-00-00-stack-check.md");
        let body = fs::read_to_string(root.join(&rel)).unwrap();
        assert!(body.contains("TOKEN=[REDACTED]"));
        let again = n.create("Stack: check!", "other", stamp);
        assert!(again.is_err(), "an existing note must never be overwritten");
        assert_eq!(fs::read_to_string(root.join(&rel)).unwrap(), body);
        let _ = fs::remove_dir_all(&root);
    }
}
