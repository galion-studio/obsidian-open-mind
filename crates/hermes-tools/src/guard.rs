// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! First line of defence, in-process and before anything runs: argument
//! allowlists and worktree-relative path resolution. The sandbox (Landlock,
//! read-only mount namespace, no network) is the second line, so a bug here
//! still cannot reach outside the worktree.

use serde_json::Value;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// Directory names the model may never read or write, at any depth.
pub const NO_GO_DIRS: &[&str] = &[".git"];
/// Extra names refused for writes only (build config can name a linker or a
/// rustc wrapper; CI config can publish).
pub const NO_WRITE_DIRS: &[&str] = &[".cargo", ".github", ".gitlab-ci.yml", ".gitmodules"];

/// Resolve a model-supplied path to a clean path relative to `worktree`
/// (which must be canonical). Refuses absolute paths outside the worktree,
/// `..`, `.git`, any symlink on the way, and (for reads) non-regular files.
pub fn resolve(worktree: &Path, input: &str, access: Access) -> Result<PathBuf, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("empty path".into());
    }
    if raw.len() > 1024 || raw.contains('\0') || raw.chars().any(char::is_control) {
        return Err("path is too long or has control characters".into());
    }
    let p = Path::new(raw);
    let rel: &Path = if p.is_absolute() {
        p.strip_prefix(worktree)
            .map_err(|_| format!("absolute path outside the allowlisted worktree refused: {raw}"))?
    } else {
        p
    };
    let mut clean = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_str().ok_or("path is not UTF-8")?;
                if NO_GO_DIRS.contains(&s) {
                    return Err(format!("{s} is off-limits to the coding tools: {raw}"));
                }
                if access == Access::Write && NO_WRITE_DIRS.contains(&s) {
                    return Err(format!("writing under {s} is not allowed: {raw}"));
                }
                clean.push(s);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("path traversal (..) refused: {raw}"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("path must be relative to the worktree: {raw}"));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("path names the worktree root, not a file".into());
    }
    // No symlinks anywhere on the way: one could point outside the tree.
    let mut cur = worktree.to_path_buf();
    let mut exists = true;
    for c in clean.components() {
        cur.push(c);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!(
                    "symlink in path refused (it could point outside the worktree): {raw}"
                ));
            }
            Ok(_) => {}
            Err(_) => {
                exists = false;
                break;
            }
        }
    }
    match access {
        Access::Read => {
            if !exists {
                return Err(format!("no such file in the worktree: {raw}"));
            }
            let m = std::fs::symlink_metadata(worktree.join(&clean))
                .map_err(|e| format!("{raw}: {e}"))?;
            if !m.is_file() {
                return Err(format!("not a regular file: {raw}"));
            }
        }
        Access::Write => {
            if exists {
                let m = std::fs::symlink_metadata(worktree.join(&clean))
                    .map_err(|e| format!("{raw}: {e}"))?;
                if !m.is_file() {
                    return Err(format!("not a regular file: {raw}"));
                }
            }
        }
    }
    Ok(clean)
}

/// Refuse any key not in `allowed` (no hidden extra arguments).
pub fn only_keys(args: &Value, allowed: &[&str]) -> Result<(), String> {
    let Some(o) = args.as_object() else {
        return Err("arguments must be a JSON object".into());
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!(
                "argument {k:?} is not allowed (allowed: {})",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

/// Cargo package / target name.
pub fn ident_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// libtest name filter (no spaces, no options).
pub fn test_filter_ok(s: &str) -> bool {
    s.len() <= 128
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

/// Commit message: printable, bounded, and never parsed as an option (it is
/// passed with `-F` from a file, not argv).
pub fn commit_message_ok(m: &str) -> Result<(), String> {
    let t = m.trim();
    if t.is_empty() {
        return Err("empty commit message".into());
    }
    if m.len() > 4000 {
        return Err("commit message longer than 4000 bytes".into());
    }
    if m.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("commit message has control characters".into());
    }
    Ok(())
}
