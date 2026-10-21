// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Coding-tool configuration, read from the environment only (no Hermes
//! config.yaml key, so enabling the tools never needs a Hermes config change).
//!
//! The tools are OFF unless `HERMES_RS_CODING_WORKTREE` is set.

use std::path::{Path, PathBuf};

/// Paths a worktree may never be, contain, or sit inside. Production checkouts,
/// Hermes state, secrets and system directories.
pub const DENY_ROOTS: &[&str] = &[
    "/opt/galion/openmind",
    "/opt/galion/hermes",
    "/opt/galion/galion-light",
    "/opt/galion/bin",
    "/root/galion-secrets",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/boot",
    "/proc",
    "/sys",
    "/dev",
    "/var/lib",
];

#[derive(Debug, Clone)]
pub struct CodingConfig {
    /// Canonical allowlisted worktree. The only directory the tools touch.
    pub worktree: PathBuf,
    /// The only branch `git commit` may run on.
    pub branch: String,
    /// Hermes-owned scratch: staged writes, CARGO_HOME, TMPDIR, HOME.
    pub state_dir: PathBuf,
    /// CARGO_TARGET_DIR for the cargo tool (outside the worktree).
    pub target_dir: PathBuf,
    /// JSON Lines audit log (one line per tool decision).
    pub audit_path: PathBuf,
    /// Rust toolchain root (has bin/cargo). None = the cargo tool refuses.
    pub toolchain: Option<PathBuf>,
    /// Delegated cgroup v2 directory for per-run cgroups (memory, pids, cpu).
    pub cgroup_parent: Option<PathBuf>,
    /// `HERMES_RS_CODING_CGROUP=delegate`: set the parent up at start from
    /// the unit's own delegated cgroup (see `cgroup_self`).
    pub cgroup_delegate: bool,
    /// Refuse to run when the per-run cgroup cannot be used.
    pub require_cgroup: bool,
    /// Tool calls allowed per session (parent + subagents share it).
    pub max_calls_per_session: u64,
    pub cargo_jobs: u32,
    pub nice: i32,
    pub cargo_timeout_s: u64,
    pub cargo_cpu_s: u64,
    /// Per-process RLIMIT_AS and per-run cgroup memory.max for cargo.
    pub cargo_mem_mb: u64,
    /// Per-run cgroup cpu.max in % of one CPU (cargo).
    pub cargo_cpu_pct: u32,
    pub git_timeout_s: u64,
    pub fs_timeout_s: u64,
    pub max_read_bytes: u64,
    pub max_write_bytes: u64,
    pub author_name: String,
    pub author_email: String,
    /// Extra directories covered by an empty tmpfs inside the sandbox
    /// (`HERMES_HOME` and `HERMES_RS_CODING_HIDE`), on top of /root, /home,
    /// /run/secrets and /opt/galion. Landlock already denies reads outside
    /// the roots.
    pub hide: Vec<PathBuf>,
    /// Total size cap of the worktree during one tool run, in MiB
    /// (`HERMES_RS_CODING_WORKDIR_MB`, default 512, 0 = off). openmind-sandbox
    /// mounts a private tmpfs of this size over the worktree, copies the
    /// worktree in, and copies changes back after the run: a tool writing
    /// past it gets ENOSPC. Same number for fs, git and cargo (one worktree).
    pub workdir_quota_mb: u64,
    /// Inode cap of that tmpfs (`HERMES_RS_CODING_WORKDIR_INODES`, 0 = MiB * 64,
    /// i.e. one per 16 KiB, the openmind-sandbox default).
    pub workdir_quota_inodes: u64,
}

fn truthy(v: &str) -> bool {
    matches!(v, "1" | "true" | "yes" | "on")
}

/// Default worktree cap in MiB: the openmind-sandbox `build_tool` preset's
/// `workdir_tmpfs_bytes` (D-WDQ1/D-WDQ2, accepted 2026-10-21).
pub const WORKDIR_QUOTA_MB_DEFAULT: u64 = 512;
/// Smallest and largest accepted non-zero cap.
pub const WORKDIR_QUOTA_MB_MIN: u64 = 16;
pub const WORKDIR_QUOTA_MB_MAX: u64 = 4096;

/// `HERMES_RS_CODING_WORKDIR_MB`: unset = 512, `0`/`off` = no cap, else
/// 16..=4096.
pub fn quota_mb(v: Option<String>) -> Result<u64, String> {
    let Some(v) = v else {
        return Ok(WORKDIR_QUOTA_MB_DEFAULT);
    };
    if matches!(v.as_str(), "0" | "off") {
        return Ok(0);
    }
    let n: u64 = v
        .parse()
        .map_err(|_| format!("bad HERMES_RS_CODING_WORKDIR_MB: {v}"))?;
    if !(WORKDIR_QUOTA_MB_MIN..=WORKDIR_QUOTA_MB_MAX).contains(&n) {
        return Err(format!(
            "HERMES_RS_CODING_WORKDIR_MB={n} is outside {WORKDIR_QUOTA_MB_MIN}..={WORKDIR_QUOTA_MB_MAX} (0 = off)"
        ));
    }
    Ok(n)
}

/// A branch name we accept for commits. Never main/master, never an option.
pub fn branch_ok(b: &str) -> Result<(), String> {
    if b.is_empty() || b.len() > 100 {
        return Err("branch name must be 1-100 characters".into());
    }
    if !b
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
    {
        return Err(format!("branch name has disallowed characters: {b}"));
    }
    if b.starts_with(['-', '/', '.'])
        || b.ends_with(['/', '.'])
        || b.contains("..")
        || b.contains("//")
        || b.ends_with(".lock")
    {
        return Err(format!("branch name is not a safe ref name: {b}"));
    }
    if matches!(b, "main" | "master" | "HEAD" | "production" | "prod") {
        return Err(format!("branch {b} is protected; use a work branch"));
    }
    Ok(())
}

/// Refuse a worktree that is, contains or lives inside a denied root.
pub fn worktree_ok(wt: &Path, extra_deny: &[PathBuf]) -> Result<(), String> {
    if wt.parent().is_none() || wt.components().count() < 3 {
        return Err(format!(
            "worktree {} is too close to / (need at least two levels)",
            wt.display()
        ));
    }
    let deny: Vec<PathBuf> = DENY_ROOTS
        .iter()
        .map(PathBuf::from)
        .chain(extra_deny.iter().cloned())
        .collect();
    for d in &deny {
        if wt.starts_with(d) || d.starts_with(wt) {
            return Err(format!(
                "worktree {} overlaps the denied path {}",
                wt.display(),
                d.display()
            ));
        }
    }
    let git = wt.join(".git");
    match std::fs::symlink_metadata(&git) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "{} is not a directory (git worktree links and symlinks point outside the allowlisted tree)",
            git.display()
        )),
        Err(e) => Err(format!("{}: {e} (not a git checkout)", git.display())),
    }
}

impl CodingConfig {
    /// `Ok(None)` when `HERMES_RS_CODING_WORKTREE` is unset (tools off).
    pub fn from_env(
        env: &dyn Fn(&str) -> Option<String>,
        hermes_home: &Path,
    ) -> Result<Option<CodingConfig>, String> {
        let get = |k: &str| {
            env(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let Some(wt) = get("HERMES_RS_CODING_WORKTREE") else {
            return Ok(None);
        };
        let wt = PathBuf::from(wt);
        if !wt.is_absolute() {
            return Err("HERMES_RS_CODING_WORKTREE must be an absolute path".into());
        }
        let wt = wt
            .canonicalize()
            .map_err(|e| format!("HERMES_RS_CODING_WORKTREE {}: {e}", wt.display()))?;
        let extra_deny: Vec<PathBuf> = get("HERMES_RS_CODING_DENY")
            .map(|s| {
                s.split(':')
                    .filter(|p| !p.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        worktree_ok(&wt, &extra_deny)?;
        let branch = get("HERMES_RS_CODING_BRANCH")
            .ok_or("HERMES_RS_CODING_BRANCH is required when the coding tools are on")?;
        branch_ok(&branch)?;
        let abs = |k: &str, default: PathBuf| -> Result<PathBuf, String> {
            let p = get(k).map(PathBuf::from).unwrap_or(default);
            if !p.is_absolute() {
                return Err(format!("{k} must be an absolute path"));
            }
            Ok(p)
        };
        let home = if hermes_home.is_absolute() {
            hermes_home.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("cwd: {e}"))?
                .join(hermes_home)
        };
        let state_dir = abs("HERMES_RS_CODING_STATE", home.join("coding"))?;
        let target_dir = abs("HERMES_RS_CODING_TARGET", state_dir.join("target"))?;
        let audit_path = abs("HERMES_RS_CODING_AUDIT", state_dir.join("audit.jsonl"))?;
        for (name, p) in [
            ("HERMES_RS_CODING_STATE", &state_dir),
            ("HERMES_RS_CODING_TARGET", &target_dir),
            ("HERMES_RS_CODING_AUDIT", &audit_path),
        ] {
            if p.starts_with(&wt) {
                return Err(format!(
                    "{name} ({}) must live outside the worktree, or the model could rewrite it",
                    p.display()
                ));
            }
        }
        let toolchain = match get("HERMES_RS_CODING_TOOLCHAIN") {
            None => None,
            Some(t) => {
                let t = PathBuf::from(t);
                let t = t
                    .canonicalize()
                    .map_err(|e| format!("HERMES_RS_CODING_TOOLCHAIN {}: {e}", t.display()))?;
                if !t.join("bin/cargo").is_file() {
                    return Err(format!(
                        "HERMES_RS_CODING_TOOLCHAIN {} has no bin/cargo (point it at a toolchain root, not the rustup proxy)",
                        t.display()
                    ));
                }
                Some(t)
            }
        };
        let num = |k: &str, d: u64, lo: u64, hi: u64| -> Result<u64, String> {
            match get(k) {
                None => Ok(d),
                Some(v) => {
                    let n: u64 = v.parse().map_err(|_| format!("bad {k}: {v}"))?;
                    if n < lo || n > hi {
                        return Err(format!("{k}={n} is outside {lo}..={hi}"));
                    }
                    Ok(n)
                }
            }
        };
        let cg_raw = get("HERMES_RS_CODING_CGROUP");
        let cgroup_delegate = cg_raw.as_deref() == Some("delegate");
        let cgroup_parent = cg_raw.filter(|v| v != "delegate").map(PathBuf::from);
        if cgroup_parent.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err("HERMES_RS_CODING_CGROUP must be an absolute path or delegate".into());
        }
        Ok(Some(CodingConfig {
            worktree: wt,
            branch,
            state_dir,
            target_dir,
            audit_path,
            toolchain,
            require_cgroup: get("HERMES_RS_CODING_REQUIRE_CGROUP")
                .map(|v| truthy(&v))
                .unwrap_or(cgroup_parent.is_some() || cgroup_delegate),
            cgroup_parent,
            cgroup_delegate,
            max_calls_per_session: num("HERMES_RS_CODING_MAX_CALLS", 200, 1, 10_000)?,
            cargo_jobs: num("HERMES_RS_CODING_JOBS", 2, 1, 4)? as u32,
            nice: num("HERMES_RS_CODING_NICE", 19, 10, 19)? as i32,
            cargo_timeout_s: num("HERMES_RS_CODING_CARGO_TIMEOUT", 600, 10, 1800)?,
            cargo_cpu_s: num("HERMES_RS_CODING_CARGO_CPU", 900, 10, 3600)?,
            cargo_mem_mb: num("HERMES_RS_CODING_CARGO_MEM_MB", 2048, 256, 4096)?,
            cargo_cpu_pct: num("HERMES_RS_CODING_CARGO_CPU_PCT", 100, 10, 400)? as u32,
            git_timeout_s: num("HERMES_RS_CODING_GIT_TIMEOUT", 30, 5, 120)?,
            fs_timeout_s: num("HERMES_RS_CODING_FS_TIMEOUT", 10, 2, 60)?,
            max_read_bytes: num("HERMES_RS_CODING_MAX_READ", 256 * 1024, 1024, 1 << 20)?,
            max_write_bytes: num("HERMES_RS_CODING_MAX_WRITE", 512 * 1024, 1024, 4 << 20)?,
            workdir_quota_mb: quota_mb(get("HERMES_RS_CODING_WORKDIR_MB"))?,
            workdir_quota_inodes: num("HERMES_RS_CODING_WORKDIR_INODES", 0, 0, 4_000_000)?,
            author_name: get("HERMES_RS_CODING_AUTHOR").unwrap_or_else(|| "hermes-rs".into()),
            author_email: get("HERMES_RS_CODING_EMAIL")
                .unwrap_or_else(|| "hermes-rs@galion.invalid".into()),
            hide: std::iter::once(home.clone())
                .chain(
                    get("HERMES_RS_CODING_HIDE")
                        .unwrap_or_default()
                        .split(':')
                        .filter(|p| p.starts_with('/'))
                        .map(PathBuf::from),
                )
                .collect(),
        }))
    }

    /// Create the hermes-owned scratch directories (mode 0700).
    pub fn prepare_dirs(&self) -> Result<(), String> {
        use std::os::unix::fs::DirBuilderExt;
        for d in [
            self.state_dir.clone(),
            self.state_dir.join("stage"),
            self.state_dir.join("tmp"),
            self.state_dir.join("home"),
            self.state_dir.join("cargo-home"),
            self.target_dir.clone(),
        ] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&d)
                .map_err(|e| format!("mkdir {}: {e}", d.display()))?;
        }
        if let Some(p) = self.audit_path.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("mkdir {}: {e}", p.display()))?;
        }
        Ok(())
    }
}
