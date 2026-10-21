// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! hermes-tools: sandboxed coding tools for hermes-rs.
//!
//! - `fs_read` / `fs_write`: one allowlisted worktree, relative paths only,
//!   no `..`, no symlinks, no `.git`; the actual I/O runs in the sandbox.
//! - `git`: status / diff / commit on one allowlisted branch. Never push.
//! - `cargo`: build / check / test / clippy, `--offline`, `nice -n 19`,
//!   `-j 2` by default.
//!
//! Every process goes through `openmind_sandbox::run` (no network, read-only
//! root, Landlock, rlimits, wall timeout, optional per-run cgroup with
//! memory.max / pids.max / cpu.max). Every decision is written to a JSON
//! Lines audit log before the run. The hermes spend caps are checked before
//! every tool call, on top of the agent loop's check before every model call.
//!
//! Off by default: nothing is registered unless `HERMES_RS_CODING_WORKTREE`
//! is set. See docs/hermes-tools.md.

pub mod audit;
pub mod cgroup_self;
pub mod config;
pub mod exec;
pub mod guard;
pub mod tools;

pub use config::CodingConfig;

use hermes_rs::config::SpendCfg;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::Registry;
use std::sync::Arc;

/// Names of the tools `register` adds.
pub const TOOL_NAMES: [&str; 4] = ["fs_read", "fs_write", "git", "cargo"];

/// Startup checks that must hold before any coding tool is offered.
pub fn preflight(cfg: &mut CodingConfig, spend: &SpendCfg) -> Result<Vec<String>, String> {
    let mut notes = vec![];
    if spend.session_usd_cap <= 0.0 || spend.daily_usd_cap <= 0.0 {
        return Err(format!(
            "coding tools need finite spend caps (session ${:.2}, daily ${:.2}); set HERMES_RS_SESSION_USD_CAP and HERMES_RS_DAILY_USD_CAP",
            spend.session_usd_cap, spend.daily_usd_cap
        ));
    }
    let f = openmind_sandbox::features();
    let allow_weak = std::env::var("HERMES_RS_CODING_ALLOW_NO_LANDLOCK").as_deref() == Ok("1");
    if f.landlock_abi < 1 && !allow_weak {
        return Err(
            "the kernel has no Landlock; refusing coding tools (HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1 overrides)"
                .into(),
        );
    }
    if f.landlock_abi < 1 {
        notes.push("WARNING: no Landlock; relying on the mount namespace only".into());
    }
    if !f.userns {
        notes.push(
            "no unprivileged user namespaces: network isolation needs Landlock ABI >= 4".into(),
        );
        if f.landlock_abi < 4 {
            return Err("neither a network namespace nor Landlock TCP deny is available".into());
        }
    }
    if cfg.cgroup_delegate {
        let gw_mem = std::env::var("HERMES_RS_CODING_GATEWAY_MEM_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(64);
        let parent = cgroup_self::enter_delegated(gw_mem << 20)?;
        notes.push(format!(
            "cgroup: gateway moved to {}/gateway (memory.max {gw_mem}M); per-run cgroups under it",
            parent.display()
        ));
        cfg.cgroup_parent = Some(parent);
    }
    if cfg.cgroup_parent.is_none() {
        notes.push(
            "no HERMES_RS_CODING_CGROUP: per-run memory/cpu/pids caps fall back to rlimits (RLIMIT_AS, RLIMIT_CPU) and nice"
                .into(),
        );
    }
    if cfg.toolchain.is_none() {
        notes.push("no HERMES_RS_CODING_TOOLCHAIN: the cargo tool will refuse every call".into());
    }
    cfg.prepare_dirs()?;
    notes.push(format!(
        "landlock_abi={} userns={} worktree={} branch={} audit={}",
        f.landlock_abi,
        f.userns,
        cfg.worktree.display(),
        cfg.branch,
        cfg.audit_path.display()
    ));
    Ok(notes)
}

/// Add the four coding tools to `reg`. Children of `delegate_task` get them
/// too (they share the session spend cap and the per-session call cap).
pub fn register(
    reg: &mut Registry,
    cfg: CodingConfig,
    spend: Option<Arc<SpendTracker>>,
) -> Arc<tools::Shared> {
    let sh = Arc::new(tools::Shared::new(cfg, spend));
    reg.add(Arc::new(tools::FsRead(sh.clone())));
    reg.add(Arc::new(tools::FsWrite(sh.clone())));
    reg.add(Arc::new(tools::Git(sh.clone())));
    reg.add(Arc::new(tools::Cargo(sh.clone())));
    sh
}
