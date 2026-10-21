// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The only place a coding tool starts a process: always through
//! `openmind_sandbox::run`, with no network, a read-only root, Landlock
//! limited to the worktree plus hermes scratch dirs, rlimits, a wall timeout
//! and (when configured) a per-run cgroup with memory.max, pids.max and
//! cpu.max. Runs are serialised: one sandboxed process tree at a time.

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
    v
}

pub fn ok(out: &Outcome) -> bool {
    out.status == Status::Exited(0) && !out.timed_out
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
