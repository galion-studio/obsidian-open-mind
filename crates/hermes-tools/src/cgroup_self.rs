// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Opt-in setup for a systemd unit with `Delegate=yes`
//! (`HERMES_RS_CODING_CGROUP=delegate`): move hermes-gateway itself into a
//! `gateway` leaf with its own memory.max (keeps today's 64M budget for the
//! server), enable memory/pids/cpu for children of the unit cgroup, and use
//! the unit cgroup as the parent of per-run sandbox cgroups.
//!
//!   <unit>/                 subtree_control: +memory +pids +cpu
//!   <unit>/gateway          hermes-gateway, memory.max = 64M
//!   <unit>/om-sbx-<pid>-<n> one per sandboxed tool run (openmind-sandbox)
//!
//! Only files under the process's own cgroup are written. Not exercised on
//! the dev box (its cgroups are root-owned); see docs/hermes-tools.md.

use std::fs;
use std::path::PathBuf;

/// Own cgroup directory from /proc/self/cgroup (v2 only).
pub fn own_cgroup() -> Result<PathBuf, String> {
    let txt =
        fs::read_to_string("/proc/self/cgroup").map_err(|e| format!("/proc/self/cgroup: {e}"))?;
    let rel = txt
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or("no cgroup v2 entry in /proc/self/cgroup")?;
    Ok(PathBuf::from("/sys/fs/cgroup").join(rel.trim_start_matches('/')))
}

/// Do the move. Returns the parent to use for per-run cgroups.
pub fn enter_delegated(gateway_mem_bytes: u64) -> Result<PathBuf, String> {
    let unit = own_cgroup()?;
    let (unit, leaf) = if unit.file_name().is_some_and(|n| n == "gateway") {
        let u = unit
            .parent()
            .ok_or("gateway cgroup has no parent")?
            .to_path_buf();
        (u.clone(), unit)
    } else {
        (unit.clone(), unit.join("gateway"))
    };
    let w = |p: PathBuf, v: &str| {
        fs::write(&p, v).map_err(|e| format!("write {}={v}: {e}", p.display()))
    };
    if !leaf.is_dir() {
        fs::create_dir(&leaf)
            .map_err(|e| format!("mkdir {}: {e} (is Delegate=yes set?)", leaf.display()))?;
    }
    // Move the whole process (all threads) into the leaf first: a cgroup
    // with member processes cannot hand controllers to its children.
    w(leaf.join("cgroup.procs"), &std::process::id().to_string())?;
    w(unit.join("cgroup.subtree_control"), "+memory +pids +cpu")?;
    w(leaf.join("memory.max"), &gateway_mem_bytes.to_string())?;
    if leaf.join("memory.swap.max").exists() {
        w(leaf.join("memory.swap.max"), "0")?;
    }
    Ok(unit)
}
