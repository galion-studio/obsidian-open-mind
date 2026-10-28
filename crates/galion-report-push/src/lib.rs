// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-report-push: commit report files to a `swarm/*` branch in the
//! tool's OWN clone and optionally push it, with the openmind write guard
//! ([`guard`]) in front of every git call.
//!
//! Replaces the ad-hoc overnight report push that used the live VPS working
//! copy `/opt/galion/openmind` as its clone (F-391-1, 2026-10-27 19:42 CET).
//! Staged only: not installed or wired on the VPS.

pub mod guard;

use guard::{Guard, Refusal, GIT_REDIRECT_VARS};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Default own clone on the VPS (D-391-2 text). Under a simulated root it
/// becomes `<root>/opt/galion/swarm-reports-git`.
pub const DEFAULT_CLONE: &str = "/opt/galion/swarm-reports-git";

/// Commit identity (wave-4 common rules).
pub const AUTHOR_NAME: &str = "Grok Bot";
pub const AUTHOR_EMAIL: &str = "grokbot@galion.studio";

#[derive(Debug)]
pub enum Error {
    /// The guard refused (exit 3).
    Refused(Refusal),
    /// Bad input (exit 2).
    Usage(String),
    /// git or file I/O failed (exit 4).
    Git(String),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Usage(_) => 2,
            Error::Refused(_) => 3,
            Error::Git(_) => 4,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Refused(r) => write!(f, "{r}"),
            Error::Usage(s) => write!(f, "usage: {s}"),
            Error::Git(s) => write!(f, "git: {s}"),
        }
    }
}

impl From<Refusal> for Error {
    fn from(r: Refusal) -> Self {
        Error::Refused(r)
    }
}

/// One report file: `src` on disk -> `dest` path inside the repo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMap {
    pub src: PathBuf,
    pub dest: PathBuf,
}

/// Parse `SRC:DEST` (DEST repo-relative, no `..`, not absolute, not `.git`).
pub fn parse_file_map(s: &str) -> Result<FileMap, Error> {
    let (src, dest) = s
        .rsplit_once(':')
        .ok_or_else(|| Error::Usage(format!("--file wants SRC:DEST, got {s:?}")))?;
    if src.is_empty() || dest.is_empty() {
        return Err(Error::Usage(format!("--file wants SRC:DEST, got {s:?}")));
    }
    let d = Path::new(dest);
    let ok = d.components().all(|c| matches!(c, Component::Normal(_)))
        && d.components().next().is_some()
        && !d.components().any(|c| c.as_os_str() == ".git");
    if !ok {
        return Err(Error::Usage(format!(
            "DEST {dest:?} must be a plain relative path inside the repo (no '..', no '/', no .git)"
        )));
    }
    Ok(FileMap {
        src: PathBuf::from(src),
        dest: d.to_path_buf(),
    })
}

/// Only `swarm/*` branches, never main, sane ref characters.
pub fn check_branch(b: &str) -> Result<(), Error> {
    let rest = b
        .strip_prefix("swarm/")
        .ok_or_else(|| Error::Usage(format!("branch {b:?}: only swarm/* branches")))?;
    let bad = rest.is_empty()
        || rest.starts_with('-')
        || rest.contains("..")
        || rest.ends_with('/')
        || rest.ends_with(".lock")
        || rest
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || "-_./".contains(c)));
    if bad {
        return Err(Error::Usage(format!("branch {b:?}: invalid name")));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub clone: PathBuf,
    pub remote: String,
    pub branch: String,
    /// Ref to start a new branch from when the remote has none (default `HEAD`
    /// of the remote, i.e. `origin/HEAD`).
    pub base: Option<String>,
    pub files: Vec<FileMap>,
    pub message: String,
    pub push: bool,
}

/// What [`publish`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub commit: Option<String>,
    pub pushed: bool,
}

/// The tool's git runner: every call re-checks the guard and strips the git
/// redirect variables, and always runs as `git -C <own clone>`.
pub struct Git<'a> {
    guard: &'a Guard,
    cwd: PathBuf,
}

impl<'a> Git<'a> {
    pub fn new(guard: &'a Guard, cwd: PathBuf) -> Self {
        Git { guard, cwd }
    }

    fn cmd(&self, dir: &Path, args: &[&str]) -> Result<Command, Error> {
        self.guard.check_path("git -C dir", dir, &self.cwd)?;
        let mut c = Command::new("git");
        c.arg("-C").arg(dir).args(args);
        for v in GIT_REDIRECT_VARS {
            c.env_remove(v);
        }
        // No pager, no prompts, no optional index writes from status-like
        // calls, no user/system config surprises for hooks paths.
        c.env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_PAGER", "cat")
            .current_dir(&self.cwd);
        Ok(c)
    }

    /// Run and return trimmed stdout.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Result<String, Error> {
        let out = self
            .cmd(dir, args)?
            .output()
            .map_err(|e| Error::Git(format!("spawn git: {e}")))?;
        if !out.status.success() {
            return Err(Error::Git(format!(
                "git {} failed ({}): {}",
                args.first().copied().unwrap_or(""),
                out.status,
                redact(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Exit status only (for probes such as `rev-parse --verify`).
    pub fn ok(&self, dir: &Path, args: &[&str]) -> Result<bool, Error> {
        let out = self
            .cmd(dir, args)?
            .output()
            .map_err(|e| Error::Git(format!("spawn git: {e}")))?;
        Ok(out.status.success())
    }
}

/// Mask token-shaped words in git's stderr (never print a secret).
pub fn redact(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let tokenish = ["ghp_", "gho_", "ghs_", "ghu_", "ghr_", "github_pat_"]
                .iter()
                .any(|p| w.contains(p))
                || (w.contains("://") && w.contains('@'));
            if tokenish {
                "***"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// After the clone exists: ask git where the repo really lives (top level,
/// git dir, common dir; covers gitfiles and `git worktree` links) and where
/// it borrows objects from (alternates); refuse if any is protected.
pub fn check_repo(guard: &Guard, git: &Git<'_>, clone: &Path, cwd: &Path) -> Result<(), Error> {
    let out = git.run(
        clone,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
    )?;
    let names = ["work tree", "git dir", "git common dir"];
    for (name, line) in names.iter().zip(out.lines()) {
        guard.check_path(name, Path::new(line), cwd)?;
    }
    let common = out.lines().nth(2).unwrap_or("").to_string();
    let alt = Path::new(&common).join("objects/info/alternates");
    if let Ok(text) = std::fs::read_to_string(&alt) {
        for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            guard.check_path(
                "alternates",
                Path::new(l),
                Path::new(&common).join("objects").as_path(),
            )?;
        }
    }
    Ok(())
}

/// Commit `plan.files` on `plan.branch` in the own clone, then optionally
/// push (fast-forward only; never `--force`).
pub fn publish(guard: &Guard, plan: &Plan, cwd: &Path) -> Result<Outcome, Error> {
    check_branch(&plan.branch)?;
    if plan.files.is_empty() {
        return Err(Error::Usage("at least one --file SRC:DEST".into()));
    }
    if plan.message.trim().is_empty() {
        return Err(Error::Usage("--message must not be empty".into()));
    }
    guard.check_env(std::env::vars_os(), cwd)?;
    guard.check_path("clone dir", &plan.clone, cwd)?;
    // Running FROM inside the live tree is not a write by itself (git never
    // uses cwd here), but it is the F-391-1 smell: refuse it too.
    guard.check_path("cwd", cwd, Path::new("/"))?;
    guard.check_remote(&plan.remote, cwd)?;
    for f in &plan.files {
        guard.check_path("report source", &f.src, cwd)?;
    }
    let git = Git::new(guard, cwd.to_path_buf());
    let clone = guard::lexical(&plan.clone, cwd);

    if !clone.join(".git").exists() {
        if clone.exists()
            && std::fs::read_dir(&clone)
                .map(|mut d| d.next().is_some())
                .unwrap_or(true)
        {
            return Err(Error::Usage(format!(
                "clone dir {} exists but is not a git clone; refusing to reuse it",
                clone.display()
            )));
        }
        let parent = clone
            .parent()
            .ok_or_else(|| Error::Usage("clone dir has no parent".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| Error::Git(format!("mkdir: {e}")))?;
        let name = clone
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Usage("clone dir name".into()))?;
        git.run(parent, &["clone", "-q", "--", &plan.remote, name])?;
    }
    check_repo(guard, &git, &clone, cwd)?;

    let remote_url = git.run(&clone, &["remote", "get-url", "origin"])?;
    if remote_url != plan.remote {
        return Err(Error::Usage(format!(
            "own clone {} points at a different origin; use a separate clone dir per remote",
            clone.display()
        )));
    }
    git.run(&clone, &["fetch", "-q", "--prune", "origin"])?;
    let remote_ref = format!("refs/remotes/origin/{}", plan.branch);
    let start = if git.ok(&clone, &["rev-parse", "-q", "--verify", &remote_ref])? {
        remote_ref
    } else {
        plan.base.clone().unwrap_or_else(|| "origin/HEAD".into())
    };
    // A leftover dirty tree from an aborted run must not leak into this commit.
    git.run(&clone, &["reset", "-q", "--hard"])?;
    git.run(
        &clone,
        &["checkout", "-q", "--no-track", "-B", &plan.branch, &start],
    )?;

    let mut dests: Vec<String> = Vec::new();
    for f in &plan.files {
        let to = clone.join(&f.dest);
        guard.check_path("report dest", &to, cwd)?;
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p).map_err(|e| Error::Git(format!("mkdir: {e}")))?;
        }
        std::fs::copy(&f.src, &to)
            .map_err(|e| Error::Git(format!("copy {}: {e}", f.src.display())))?;
        dests.push(f.dest.to_string_lossy().into_owned());
    }
    let mut add = vec!["add", "--"];
    add.extend(dests.iter().map(String::as_str));
    git.run(&clone, &add)?;
    if git.ok(&clone, &["diff", "--cached", "--quiet"])? {
        return Ok(Outcome {
            commit: None,
            pushed: false,
        });
    }
    let msg = format!("{} ({})", plan.message.trim(), zagreb_time::now_rfc3339());
    let name = format!("user.name={AUTHOR_NAME}");
    let email = format!("user.email={AUTHOR_EMAIL}");
    git.run(
        &clone,
        &[
            "-c",
            &name,
            "-c",
            &email,
            "commit",
            "-q",
            "--no-verify",
            "-m",
            &msg,
        ],
    )?;
    let sha = git.run(&clone, &["rev-parse", "HEAD"])?;
    let mut pushed = false;
    if plan.push {
        let spec = format!("refs/heads/{0}:refs/heads/{0}", plan.branch);
        git.run(&clone, &["push", "-q", "origin", &spec])?;
        pushed = true;
    }
    Ok(Outcome {
        commit: Some(sha),
        pushed,
    })
}

/// Default clone dir, honouring the simulated root.
pub fn default_clone() -> PathBuf {
    match std::env::var_os(guard::SIM_ROOT_ENV).filter(|v| !v.is_empty()) {
        Some(root) => Path::new(&root).join(DEFAULT_CLONE.trim_start_matches('/')),
        None => PathBuf::from(DEFAULT_CLONE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_maps() {
        let m = parse_file_map("/tmp/r.md:swarm/reports/r.md").unwrap();
        assert_eq!(m.dest, PathBuf::from("swarm/reports/r.md"));
        for bad in [
            "/tmp/r.md:",
            ":x",
            "nocolon",
            "/a:../x",
            "/a:/abs",
            "/a:.git/config",
            "/a:d/.git/x",
        ] {
            assert!(parse_file_map(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn branches() {
        assert!(check_branch("swarm/overnight-loop-20261027").is_ok());
        for bad in [
            "main",
            "swarm/",
            "swarm/-x",
            "swarm/a..b",
            "swarm/a b",
            "refs/heads/main",
            "swarm/x.lock",
        ] {
            assert!(check_branch(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn redacts() {
        let tok = concat!("gh", "p_", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let s = format!("fatal: auth {tok} https://x:{tok}@github.com/a");
        let r = redact(&s);
        assert!(!r.contains(tok));
        assert!(r.contains("***"));
    }
}
