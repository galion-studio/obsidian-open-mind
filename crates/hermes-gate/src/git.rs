// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Host git, hardened: cleared env, no system/global config, no hooks, no
//! pager, no credential helper. Only local operations are offered here;
//! there is no push, fetch or remote function in this crate.

use std::path::Path;
use std::process::{Command, Stdio};

/// Ops this crate may run. Anything else is a programming error.
pub const OPS: &[&str] = &[
    "rev-parse",
    "worktree",
    "apply",
    "diff",
    "write-tree",
    "add",
    "status",
    "init",
    "commit",
    "config",
    "log",
];

pub fn git(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<String, String> {
    let op = args.first().copied().unwrap_or("");
    if !OPS.contains(&op) {
        return Err(format!("git {op}: not allowed in hermes-gate"));
    }
    if args
        .iter()
        .any(|a| *a == "push" || *a == "fetch" || *a == "remote")
    {
        return Err("git network ops are never allowed in hermes-gate".into());
    }
    // Owner rule: never run git in a live tree (e.g. /opt/galion/openmind).
    // Checked before any spawn, on the path as given and as resolved.
    let resolved = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if crate::stage::under_live_root(dir) || crate::stage::under_live_root(&resolved) {
        return Err(format!(
            "{}: git in a live root is never run by hermes-gate",
            dir.display()
        ));
    }
    let mut c = Command::new("git");
    c.current_dir(dir).env_clear();
    c.env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("HOME", "/nonexistent")
        .env("GIT_AUTHOR_NAME", "hermes-gate")
        .env("GIT_AUTHOR_EMAIL", "hermes-gate@localhost")
        .env("GIT_COMMITTER_NAME", "hermes-gate")
        .env("GIT_COMMITTER_EMAIL", "hermes-gate@localhost");
    c.args([
        "--no-pager",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "credential.helper=",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "commit.gpgsign=false",
    ]);
    c.args(args);
    c.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut ch = c.spawn().map_err(|e| format!("spawn git: {e}"))?;
    if let Some(data) = stdin {
        use std::io::Write;
        let mut si = ch.stdin.take().ok_or("no stdin")?;
        si.write_all(data).map_err(|e| e.to_string())?;
    }
    let o = ch.wait_with_output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn head(dir: &Path) -> Result<String, String> {
    Ok(git(dir, &["rev-parse", "HEAD"], None)?.trim().to_string())
}
