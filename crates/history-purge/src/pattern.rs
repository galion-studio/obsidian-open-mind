// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! Path patterns for the purge list.
//!
//! One pattern per line; blank lines and lines starting with `#` are ignored.
//!
//! * `dir/` (trailing slash): the directory `dir` at the repo root and
//!   everything under it.
//! * a pattern with a `/` inside: an anchored glob against the full path. A
//!   path also matches when one of its parent directories matches, so
//!   `stack/kms` drops `stack/kms/a/b`.
//! * a pattern without `/`: a glob against the last path component, at any
//!   depth (`*.redacted`, `id_ed25519`).
//!
//! Globs: `*` any run of bytes except `/`, `?` one byte except `/`,
//! `**` any run of bytes including `/` (`a/**/b` also matches `a/b`).
//! A leading `/` is accepted and ignored (patterns are always rooted).

/// One compiled pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pat {
    /// Anchored directory prefix (from `dir/`).
    Dir(Vec<u8>),
    /// Anchored glob over the full path (or any parent directory of it).
    Full(Vec<u8>),
    /// Glob over the basename at any depth.
    Base(Vec<u8>),
}

/// A set of path patterns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathSet {
    pats: Vec<Pat>,
    source: Vec<String>,
}

impl PathSet {
    /// Parse a purge list (file contents).
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut set = PathSet::default();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            set.add(line).map_err(|e| format!("line {}: {e}", n + 1))?;
        }
        Ok(set)
    }

    /// Add one pattern.
    pub fn add(&mut self, pattern: &str) -> Result<(), String> {
        let p = pattern.trim().trim_start_matches('/');
        if p.is_empty() || p == "**" || p == "*" {
            return Err(format!("pattern {pattern:?} would drop every file"));
        }
        if p.split('/').any(|s| s == "..") {
            return Err(format!("pattern {pattern:?} contains .."));
        }
        let pat = if let Some(d) = p.strip_suffix('/') {
            if d.contains(['*', '?']) {
                Pat::Full(d.as_bytes().to_vec())
            } else {
                Pat::Dir(d.as_bytes().to_vec())
            }
        } else if p.contains('/') {
            Pat::Full(p.as_bytes().to_vec())
        } else {
            Pat::Base(p.as_bytes().to_vec())
        };
        self.pats.push(pat);
        self.source.push(pattern.trim().to_string());
        Ok(())
    }

    /// Number of patterns.
    pub fn len(&self) -> usize {
        self.pats.len()
    }

    /// True when no pattern is set.
    pub fn is_empty(&self) -> bool {
        self.pats.is_empty()
    }

    /// The patterns as written.
    pub fn patterns(&self) -> &[String] {
        &self.source
    }

    /// Does `path` (repo-relative, `/`-separated, unquoted) match?
    pub fn matches(&self, path: &[u8]) -> bool {
        self.pats.iter().any(|p| match p {
            Pat::Dir(d) => {
                path == d.as_slice()
                    || (path.len() > d.len() && path.starts_with(d) && path[d.len()] == b'/')
            }
            Pat::Full(g) => {
                if glob(g, path) {
                    return true;
                }
                // any parent directory
                path.iter()
                    .enumerate()
                    .filter(|(_, &b)| b == b'/')
                    .any(|(i, _)| glob(g, &path[..i]))
            }
            Pat::Base(g) => {
                let base = match path.iter().rposition(|&b| b == b'/') {
                    Some(i) => &path[i + 1..],
                    None => path,
                };
                glob(g, base)
            }
        })
    }
}

/// Glob match over bytes. `*` and `?` stop at `/`; `**` crosses it.
pub fn glob(pat: &[u8], s: &[u8]) -> bool {
    if pat.is_empty() {
        return s.is_empty();
    }
    if pat.starts_with(b"**") {
        let rest = &pat[2..];
        // `**/` may also match zero directories
        if rest.first() == Some(&b'/') && glob(&rest[1..], s) {
            return true;
        }
        return (0..=s.len()).any(|i| glob(rest, &s[i..]));
    }
    match pat[0] {
        b'*' => {
            let rest = &pat[1..];
            for i in 0..=s.len() {
                if glob(rest, &s[i..]) {
                    return true;
                }
                if i < s.len() && s[i] == b'/' {
                    break;
                }
            }
            false
        }
        b'?' => !s.is_empty() && s[0] != b'/' && glob(&pat[1..], &s[1..]),
        c => !s.is_empty() && s[0] == c && glob(&pat[1..], &s[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(p: &[&str]) -> PathSet {
        let mut s = PathSet::default();
        for x in p {
            s.add(x).unwrap();
        }
        s
    }

    #[test]
    fn dir_prefix() {
        let s = set(&["contrib/"]);
        assert!(s.matches(b"contrib/a"));
        assert!(s.matches(b"contrib/x/y.md"));
        assert!(!s.matches(b"contribx/a"));
        assert!(!s.matches(b"a/contrib/b"));
    }

    #[test]
    fn full_and_parent() {
        let s = set(&["stack/kms", "stack/INDEX.md", "/dist/bin/"]);
        assert!(s.matches(b"stack/kms/root/x.redacted"));
        assert!(s.matches(b"stack/INDEX.md"));
        assert!(!s.matches(b"stack/INDEX.md.bak"));
        assert!(s.matches(b"dist/bin/tuner"));
        assert!(!s.matches(b"stack/kmsx"));
    }

    #[test]
    fn basename_glob() {
        let s = set(&["*.redacted", "id_ed25519", ".env"]);
        assert!(s.matches(b"stack/hermes/.env.redacted"));
        assert!(s.matches(b"a.redacted"));
        assert!(s.matches(b"x/.ssh/id_ed25519"));
        assert!(!s.matches(b"x/.ssh/id_ed25519.pub"));
        assert!(s.matches(b"galion-backend/.env"));
        assert!(!s.matches(b"galion-backend/.env.example"));
    }

    #[test]
    fn double_star() {
        let s = set(&["edge/**/dist/*.bin"]);
        assert!(s.matches(b"edge/dist/a.bin"));
        assert!(s.matches(b"edge/x/y/dist/a.bin"));
        assert!(!s.matches(b"edge/x/dist/sub/a.bin"));
        assert!(glob(b"a/**", b"a/b/c"));
        assert!(!glob(b"a/*", b"a/b/c"));
        assert!(glob(b"a/?c", b"a/bc"));
        assert!(!glob(b"a?c", b"a/c"));
    }

    #[test]
    fn parse_file_and_reject_everything() {
        let s = PathSet::parse("# c\n\ncontrib/\n *.redacted \n").unwrap();
        assert_eq!(s.len(), 2);
        assert!(PathSet::parse("**\n").is_err());
        assert!(PathSet::parse("/\n").is_err());
        assert!(PathSet::parse("a/../b\n").is_err());
    }
}
