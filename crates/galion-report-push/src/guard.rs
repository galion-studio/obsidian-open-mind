// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The openmind write guard (D-391-2, F-391-1/F-391-3).
//!
//! The live VPS working copy `/opt/galion/openmind` must never see a git
//! write again (the 2026-10-27 19:42 CET overnight report push checked out a
//! `swarm/*` branch and committed there). Every git call this crate makes goes
//! through [`Guard`] first:
//!
//! * the target path (and every path git itself reports for the repo: top
//!   level, git dir, common dir, alternates) must not be the protected tree
//!   or anything below it, after symlinks are resolved;
//! * `GIT_DIR` / `GIT_WORK_TREE` / `GIT_COMMON_DIR` / `GIT_INDEX_FILE` /
//!   `GIT_OBJECT_DIRECTORY` / `GIT_ALTERNATE_OBJECT_DIRECTORIES` may not point
//!   into it, and they are stripped from every git child anyway;
//! * the protected list always contains `/opt/galion/openmind`; tests add a
//!   simulated root with `GALION_SIM_ROOT` (it can only ADD paths, never
//!   remove the real one).

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// The live working copy. Always protected, whatever the environment says.
pub const LIVE_TREE: &str = "/opt/galion/openmind";

/// Env var naming a simulated root; `<root>/opt/galion/openmind` is protected
/// in addition to [`LIVE_TREE`].
pub const SIM_ROOT_ENV: &str = "GALION_SIM_ROOT";

/// Extra protected paths, `:`-separated (additive only).
pub const EXTRA_ENV: &str = "GALION_PROTECT_EXTRA";

/// Git env vars that redirect where git reads/writes the repo. They are
/// checked by [`Guard::check_env`] and removed from every git child.
pub const GIT_REDIRECT_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// Why the guard said no.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// What was checked (e.g. "clone dir", "GIT_DIR", "git common dir").
    pub what: String,
    /// The offending path as given.
    pub path: PathBuf,
    /// The protected root it falls under.
    pub protected: PathBuf,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "refused: {} {} is inside protected tree {} (D-391-2: no git writes there; use your own clone)",
            self.what,
            self.path.display(),
            self.protected.display()
        )
    }
}

impl std::error::Error for Refusal {}

/// Purely lexical cleanup: make absolute against `cwd`, drop `.`, apply `..`.
pub fn lexical(path: &Path, cwd: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(n) => out.push(n),
        }
    }
    out
}

/// Lexical cleanup, then resolve symlinks on the longest existing prefix and
/// re-append the part that does not exist yet. A path that does not exist at
/// all still normalizes, so a clone dir that is about to be created can be
/// checked before `git clone` runs.
pub fn resolve(path: &Path, cwd: &Path) -> PathBuf {
    let lex = lexical(path, cwd);
    let mut head = lex.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(&head) {
            let mut out = real;
            for t in tail.iter().rev() {
                out.push(t);
            }
            // The re-appended tail may contain a symlink created later; that
            // is caught by the re-check before every git call.
            return out;
        }
        match (head.file_name().map(|n| n.to_os_string()), head.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                head = parent.to_path_buf();
            }
            _ => return lex,
        }
    }
}

/// True when `p` equals `root` or lies below it (component-wise, so
/// `/opt/galion/openmind-old` is NOT inside `/opt/galion/openmind`).
pub fn within(p: &Path, root: &Path) -> bool {
    p.starts_with(root)
}

#[derive(Clone, Debug)]
pub struct Guard {
    protected: Vec<PathBuf>,
}

impl Guard {
    /// The live tree only.
    pub fn live() -> Guard {
        Guard {
            protected: vec![PathBuf::from(LIVE_TREE)],
        }
    }

    /// Live tree plus anything added by [`SIM_ROOT_ENV`] / [`EXTRA_ENV`].
    pub fn from_env() -> Guard {
        let mut g = Guard::live();
        if let Some(root) = std::env::var_os(SIM_ROOT_ENV).filter(|v| !v.is_empty()) {
            g.add(Path::new(&root).join("opt/galion/openmind"));
        }
        if let Some(extra) = std::env::var_os(EXTRA_ENV) {
            for p in std::env::split_paths(&extra) {
                if !p.as_os_str().is_empty() {
                    g.add(p);
                }
            }
        }
        g
    }

    /// Add one more protected root (the live tree can never be removed).
    pub fn add(&mut self, p: impl Into<PathBuf>) {
        let p = p.into();
        if !self.protected.contains(&p) {
            self.protected.push(p);
        }
    }

    pub fn protected(&self) -> &[PathBuf] {
        &self.protected
    }

    /// Refuse `path` (relative to `cwd`) if it is a protected tree or below
    /// one. Compared both lexically and after symlink resolution, against the
    /// protected roots both as written and resolved.
    pub fn check_path(&self, what: &str, path: &Path, cwd: &Path) -> Result<(), Refusal> {
        let forms = [lexical(path, cwd), resolve(path, cwd)];
        for root in &self.protected {
            let roots = [lexical(root, Path::new("/")), resolve(root, Path::new("/"))];
            for f in &forms {
                for r in &roots {
                    if within(f, r) {
                        return Err(Refusal {
                            what: what.to_string(),
                            path: path.to_path_buf(),
                            protected: root.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Refuse when any git redirect variable in `env` points into a protected
    /// tree. `env` is `(name, value)`; pass `std::env::vars_os()` in real use.
    pub fn check_env<I, K, V>(&self, env: I, cwd: &Path) -> Result<(), Refusal>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<std::ffi::OsStr>,
        V: AsRef<std::ffi::OsStr>,
    {
        for (k, v) in env {
            let Some(name) = k.as_ref().to_str() else {
                continue;
            };
            if !GIT_REDIRECT_VARS.contains(&name) {
                continue;
            }
            // Alternates and ceilings are path lists.
            for p in std::env::split_paths(v.as_ref()) {
                if p.as_os_str().is_empty() {
                    continue;
                }
                self.check_path(name, &p, cwd)?;
            }
        }
        Ok(())
    }

    /// Refuse a remote URL that is a local path (or `file://` URL) into a
    /// protected tree: cloning or fetching from the live tree is not a
    /// report path either.
    pub fn check_remote(&self, url: &str, cwd: &Path) -> Result<(), Refusal> {
        let local = if let Some(rest) = url.strip_prefix("file://") {
            Some(rest)
        } else if url.contains("://") || is_scp_like(url) {
            None
        } else {
            Some(url)
        };
        match local {
            Some(p) => self.check_path("remote", Path::new(p), cwd),
            None => Ok(()),
        }
    }
}

/// `user@host:path` / `host:path` (git's scp-like syntax): a colon before the
/// first slash.
fn is_scp_like(url: &str) -> bool {
    match (url.find(':'), url.find('/')) {
        (Some(c), Some(s)) => c < s,
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> Guard {
        Guard::live()
    }

    #[test]
    fn live_tree_and_children_refused() {
        let cwd = Path::new("/");
        assert!(g().check_path("t", Path::new(LIVE_TREE), cwd).is_err());
        assert!(g()
            .check_path("t", Path::new("/opt/galion/openmind/docs/x"), cwd)
            .is_err());
        assert!(g()
            .check_path("t", Path::new("/opt/galion/openmind/"), cwd)
            .is_err());
        assert!(g()
            .check_path("t", Path::new("/opt/galion/./openmind"), cwd)
            .is_err());
        assert!(g()
            .check_path("t", Path::new("/opt/galion/x/../openmind/.git"), cwd)
            .is_err());
        assert!(g()
            .check_path("t", Path::new("openmind"), Path::new("/opt/galion"))
            .is_err());
    }

    #[test]
    fn siblings_allowed() {
        let cwd = Path::new("/");
        for p in [
            "/opt/galion/openmind-old",
            "/opt/galion/swarm-reports-git",
            "/opt/galion",
            "/workspace/swarm/wt/x",
        ] {
            assert!(g().check_path("t", Path::new(p), cwd).is_ok(), "{p}");
        }
    }

    #[test]
    fn env_redirects_refused() {
        let cwd = Path::new("/tmp");
        let bad = [("GIT_DIR", "/opt/galion/openmind/.git")];
        assert!(g().check_env(bad, cwd).is_err());
        let bad2 = [("GIT_WORK_TREE", "/opt/galion/openmind")];
        assert!(g().check_env(bad2, cwd).is_err());
        let alt = [(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "/tmp/a:/opt/galion/openmind/.git/objects",
        )];
        assert!(g().check_env(alt, cwd).is_err());
        let ok = [
            ("GIT_DIR", "/tmp/own/.git"),
            ("HOME", "/opt/galion/openmind"),
        ];
        assert!(g().check_env(ok, cwd).is_ok());
    }

    #[test]
    fn remotes() {
        let cwd = Path::new("/");
        assert!(g().check_remote("/opt/galion/openmind", cwd).is_err());
        assert!(g()
            .check_remote("file:///opt/galion/openmind/.git", cwd)
            .is_err());
        assert!(g()
            .check_remote(
                "https://github.com/galion-studio/obsidian-open-mind.git",
                cwd
            )
            .is_ok());
        assert!(g()
            .check_remote("git@github.com:galion-studio/x.git", cwd)
            .is_ok());
        assert!(g().check_remote("/srv/reports.git", cwd).is_ok());
    }

    #[test]
    fn extra_roots_are_additive() {
        let mut gg = Guard::live();
        gg.add("/tmp/sim/opt/galion/openmind");
        assert_eq!(gg.protected().len(), 2);
        assert!(gg
            .check_path(
                "t",
                Path::new("/tmp/sim/opt/galion/openmind/a"),
                Path::new("/")
            )
            .is_err());
        assert!(gg
            .check_path("t", Path::new(LIVE_TREE), Path::new("/"))
            .is_err());
    }

    #[test]
    fn lexical_cleans() {
        assert_eq!(
            lexical(Path::new("a/./b/../c"), Path::new("/x")),
            PathBuf::from("/x/a/c")
        );
        assert_eq!(
            lexical(Path::new("/../.."), Path::new("/")),
            PathBuf::from("/")
        );
    }
}
