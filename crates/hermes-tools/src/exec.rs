// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The only place a coding tool starts a process: always through
//! `openmind_sandbox::run`, with no network, a read-only root, Landlock
//! limited to the worktree plus hermes scratch dirs, rlimits, a wall timeout
//! and (when configured) a per-run cgroup with memory.max, pids.max and
//! cpu.max, and a total worktree cap (a private tmpfs over the worktree,
//! copied in and back; docs/sandbox/workdir-quota.md). Runs are
//! serialised: one sandboxed process tree at a time (also what makes the
//! change-only copy back safe).

use crate::config::CodingConfig;
use openmind_sandbox::{Outcome, Preset, SandboxPolicy, Status};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fs,
    Git,
    Cargo,
}

/// First existing absolute path for a system program. Never searches the
/// worktree or a model-influenced PATH.
pub fn system_bin(name: &str) -> Option<PathBuf> {
    [
        "/usr/bin",
        "/bin",
        "/usr/local/bin",
        "/data/data/com.termux/files/usr/bin",
    ]
    .iter()
    .map(|d| Path::new(d).join(name))
    .find(|p| p.is_file())
}

/// Build the sandbox policy for one kind of run.
pub fn policy(cfg: &CodingConfig, kind: Kind) -> SandboxPolicy {
    let preset = match kind {
        Kind::Cargo => Preset::build_tool(),
        Kind::Fs | Kind::Git => Preset::read_only_tool(),
    };
    let mut p = preset.policy(&cfg.worktree);
    let s = |p: &Path| p.display().to_string();
    let state = &cfg.state_dir;
    let tmp = state.join("tmp");
    let home = state.join("home");
    let mut path = String::from("/usr/bin:/bin");
    if kind == Kind::Cargo {
        if let Some(tc) = &cfg.toolchain {
            path = format!("{}:{path}", s(&tc.join("bin")));
        }
    }
    // Nothing inherited from hermes except locale/TZ; everything else is set.
    p.env_allowlist = vec!["LANG".into(), "LC_ALL".into(), "TZ".into()];
    p.env_set = vec![
        ("PATH".into(), path),
        ("HOME".into(), s(&home)),
        ("TMPDIR".into(), s(&tmp)),
    ];
    p.fs_read.push(PathBuf::from("/dev/null"));
    p.fs_write = vec![tmp, home, PathBuf::from("/dev/null")];
    p.hide_paths.extend(cfg.hide.iter().cloned());
    p.net = false;
    p.require_net_isolation = true;
    p.cgroup_parent = cfg.cgroup_parent.clone();
    // Total worktree cap (D-WDQ2, accepted 2026-10-21): the private work dir
    // tmpfs of openmind-sandbox (row 151) over the worktree. One number for
    // fs, git and cargo, because all three run on the same worktree and a
    // worktree that does not fit fails the run closed: with the read-only
    // preset's 64 MiB, fs_read would refuse on a worktree cargo can build.
    // Default 512 MiB = the build-tool preset (the coding preset).
    p.workdir_tmpfs_bytes = cfg.workdir_quota_mb << 20;
    p.workdir_tmpfs_inodes = cfg.workdir_quota_inodes;
    match kind {
        Kind::Fs => {
            p.fs_read.push(state.join("stage"));
            p.wall_timeout = Duration::from_secs(cfg.fs_timeout_s);
            p.cpu_secs = cfg.fs_timeout_s;
            p.output_cap = cfg.max_read_bytes as usize + 1;
            p.fsize_bytes = cfg.max_write_bytes + 4096;
            p.cgroup_mem_bytes = 128 << 20;
            p.cgroup_pids_max = 16;
            p.cgroup_cpu_pct = 100;
        }
        Kind::Git => {
            p.fs_read.push(state.join("stage"));
            p.env_set.extend([
                ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
                ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
                ("GIT_TERMINAL_PROMPT".into(), "0".into()),
                ("GIT_EDITOR".into(), "true".into()),
                ("GIT_PAGER".into(), "cat".into()),
                ("GIT_DIR".into(), s(&cfg.worktree.join(".git"))),
                ("GIT_WORK_TREE".into(), s(&cfg.worktree)),
                (
                    "GIT_CEILING_DIRECTORIES".into(),
                    cfg.worktree.parent().map(s).unwrap_or_default(),
                ),
            ]);
            p.wall_timeout = Duration::from_secs(cfg.git_timeout_s);
            p.cpu_secs = cfg.git_timeout_s;
            p.fsize_bytes = 256 << 20;
            p.output_cap = 128 * 1024;
            p.cgroup_mem_bytes = 256 << 20;
            p.cgroup_pids_max = 32;
            p.cgroup_cpu_pct = 100;
        }
        Kind::Cargo => {
            let ch = state.join("cargo-home");
            if let Some(tc) = &cfg.toolchain {
                p.fs_read.push(tc.clone());
            }
            p.fs_write.push(ch.clone());
            p.fs_write.push(cfg.target_dir.clone());
            p.env_set.extend([
                ("CARGO_HOME".into(), s(&ch)),
                ("CARGO_TARGET_DIR".into(), s(&cfg.target_dir)),
                ("CARGO_NET_OFFLINE".into(), "true".into()),
                ("CARGO_INCREMENTAL".into(), "0".into()),
                ("CARGO_TERM_COLOR".into(), "never".into()),
                ("CARGO_BUILD_JOBS".into(), cfg.cargo_jobs.to_string()),
            ]);
            p.wall_timeout = Duration::from_secs(cfg.cargo_timeout_s);
            p.cpu_secs = cfg.cargo_cpu_s;
            p.mem_bytes = cfg.cargo_mem_mb << 20;
            p.cgroup_mem_bytes = cfg.cargo_mem_mb << 20;
            p.cgroup_pids_max = 256;
            p.cgroup_cpu_pct = cfg.cargo_cpu_pct;
        }
    }
    // tmpfs pages are memory and are charged to the per-run cgroup (VPS
    // q9/S9e of row 151: a full tmpfs at memory.max is an OOM kill of the
    // whole run and nothing is copied back). The cap is added on top of each
    // kind's own budget, so a full worktree still ends in ENOSPC, never OOM.
    if p.cgroup_mem_bytes > 0 {
        p.cgroup_mem_bytes += p.workdir_tmpfs_bytes;
    }
    p
}

/// Run `cmd` in the sandbox. `Err` = refused before start (config).
pub fn run(cfg: &CodingConfig, kind: Kind, cmd: &[String]) -> Result<Outcome, String> {
    if cfg.require_cgroup && cfg.cgroup_parent.is_none() {
        return Err(
            "HERMES_RS_CODING_REQUIRE_CGROUP is on but no HERMES_RS_CODING_CGROUP is set".into(),
        );
    }
    let p = policy(cfg, kind);
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let out = openmind_sandbox::run(cmd, &p);
    if out.status == Status::Error {
        return Err(format!(
            "sandbox refused to start: {}",
            out.error.clone().unwrap_or_default()
        ));
    }
    layered_check(cfg, &out)?;
    Ok(out)
}

/// Compact, model-facing summary of a run (also logged).
pub fn summary(out: &Outcome) -> Value {
    let (exit, signal) = match out.status {
        Status::Exited(c) => (json!(c), Value::Null),
        Status::Signaled(s) => (Value::Null, json!(s)),
        Status::Error => (Value::Null, Value::Null),
    };
    let mut on = vec![];
    let mut off = Map::new();
    for (name, e) in &out.enforced {
        match e {
            openmind_sandbox::Enforcement::Enforced => on.push(json!(name)),
            openmind_sandbox::Enforcement::NotEnforced(why) => {
                off.insert(name.clone(), json!(why));
            }
        }
    }
    let mut v = json!({
        "exit": exit,
        "signal": signal,
        "timed_out": out.timed_out,
        "wall_ms": out.wall.as_millis() as u64,
        "sandbox": {"enforced": on, "not_enforced": off},
    });
    if let Some(c) = &out.cgroup {
        v["cgroup"] = json!({
            "oom_kills": c.oom_kills, "mem_max_hits": c.mem_max_hits,
            "pids_max_hits": c.pids_max_hits, "mem_peak": c.mem_peak,
        });
    }
    if let Some(q) = &out.workdir_quota {
        v["workdir_quota"] = json!({
            "limit_mib": q.limit_bytes >> 20, "used_bytes": q.used_bytes,
            "in_bytes": q.in_bytes, "out_bytes": q.out_bytes,
            "out_entries": q.out_entries, "copied_out": q.copied_out,
            "out_complete": q.out_complete, "errno": q.errno,
        });
    }
    v
}

/// Why the changes of a run did not fully reach the host worktree, if they
/// did not (D-WDQ3, accepted 2026-10-21: partial files stay on the host and
/// the caller reports it; nothing is rolled back). `None` = no cap, or the
/// copy back finished.
pub fn copy_back_problem(out: &Outcome) -> Option<String> {
    let q = out.workdir_quota.as_ref()?;
    if q.limit_bytes == 0 || (q.copied_out && q.out_complete) {
        return None;
    }
    if !q.copied_out {
        return Some(format!(
            "worktree changes of this run were NOT copied back (run killed{}); the host worktree is as before the run",
            if out.timed_out { " at the wall timeout" } else { "" }
        ));
    }
    let why = match q.errno {
        libc_errno::ENOSPC => "the host file system is full".to_string(),
        libc_errno::EDQUOT => format!(
            "the copy-back budget ({} MiB / {} entries) was exceeded (sparse files count at full size)",
            q.limit_bytes >> 20,
            q.limit_inodes
        ),
        e => format!("errno {e}"),
    };
    Some(format!(
        "worktree copy back is INCOMPLETE: {why}; {} entries / {} bytes reached the host, partial files may be left there (check `git status` before committing)",
        q.out_entries, q.out_bytes
    ))
}

/// Model-facing error text: the copy-back problem (if any) first, then the
/// tail of stderr.
pub fn failure(out: &Outcome, n: usize) -> String {
    match copy_back_problem(out) {
        Some(p) => format!("{p}\n{}", tail(&out.stderr, n)),
        None => tail(&out.stderr, n),
    }
}

/// Exit 0, no timeout, and (with the worktree cap on) every change copied
/// back to the host.
pub fn ok(out: &Outcome) -> bool {
    out.status == Status::Exited(0) && !out.timed_out && copy_back_problem(out).is_none()
}

/// The errno values this module names (no libc dependency in this crate).
mod libc_errno {
    pub const ENOSPC: i32 = 28;
    pub const EDQUOT: i32 = 122;
}

/// Tail of a byte stream as text (tool output keeps the end: errors are there).
pub fn tail(b: &[u8], n: usize) -> String {
    let s = String::from_utf8_lossy(b);
    let count = s.chars().count();
    if count <= n {
        return s.into_owned();
    }
    let t: String = s.chars().skip(count - n).collect();
    format!("[... {} chars cut ...]\n{t}", count - n)
}

/// What a worktree needs on the tmpfs, counted without following symlinks:
/// regular files rounded up to whole 4 KiB pages, one entry per file, dir
/// and symlink (the root's own inode included). Stops counting once either
/// limit is passed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Footprint {
    pub bytes: u64,
    pub entries: u64,
}

pub fn footprint(root: &Path, limit_bytes: u64, limit_entries: u64) -> Footprint {
    let mut fp = Footprint {
        bytes: 0,
        entries: 1,
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            let t = m.file_type();
            if t.is_dir() {
                stack.push(e.path());
            } else if t.is_file() {
                fp.bytes += m.len().div_ceil(4096) * 4096;
            } else if !t.is_symlink() {
                continue; // FIFOs, sockets, devices are not copied in
            }
            fp.entries += 1;
            if fp.bytes > limit_bytes || fp.entries > limit_entries {
                return fp;
            }
        }
    }
    fp
}

/// D-320-1 option c (`HERMES_RS_CODING_NO_LANDLOCK=layered`, slot 334): when
/// Landlock was not enforced on a run, every layer in
/// [`crate::config::LAYERED_REQUIRED`] must have been, else the run fails
/// closed (its output is withheld from the model). Other modes: no check.
/// The run has already happened at this point; preflight's probe is what
/// keeps an unsuitable kernel from starting at all.
pub fn layered_check(cfg: &CodingConfig, out: &Outcome) -> Result<(), String> {
    if cfg.no_landlock != crate::config::NoLandlock::Layered {
        return Ok(());
    }
    let on = |name: &str| {
        out.enforced
            .iter()
            .any(|(n, e)| n == name && matches!(e, openmind_sandbox::Enforcement::Enforced))
    };
    if on("landlock") {
        return Ok(());
    }
    let miss: Vec<&str> = crate::config::LAYERED_REQUIRED
        .iter()
        .copied()
        .filter(|l| !on(l))
        .collect();
    if miss.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "layered no-Landlock mode (D-320-1 option c): run failed closed, layers not enforced: {}",
            miss.join(", ")
        ))
    }
}
